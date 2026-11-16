#![allow(non_snake_case)]

use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use crate::{
    contracts::{
        container::Container as _,
        view::{
            Factory as ContractFactory, View as ContractView, ViewCallback as ContractCallback,
        },
    },
    support::facades::current_container,
};

use super::{
    Component, ComponentOutput, FileViewFinder, View, ViewBinding, ViewData, ViewError, ViewFinder,
    ViewResult, ViewValue,
    compilers::BladeCompiler,
    component::ResolvedComponent,
    engines::EngineResolver,
    file_view_finder::{ViewSource, relative_name, valid_segment},
    rendering::{RenderContext, render_error},
    request_data::ViewRequestData,
};

/// Creates named views from the application's finder and engines.
#[derive(Clone)]
pub struct Factory {
    finder: Arc<FileViewFinder>,
    engines: Arc<EngineResolver>,
    events: Arc<RwLock<ViewEvents>>,
    embedded: HashMap<String, &'static str>,
    pub(super) public_path: Option<PathBuf>,
}

enum ResolvedSource<'a> {
    File(ViewSource),
    Embedded { name: &'a str, source: &'static str },
}

type ComponentResolver = Arc<dyn Fn(&ViewData) -> ViewResult<ResolvedComponent> + Send + Sync>;

type ViewCallback = Arc<dyn Fn(&mut View) -> ViewResult<()> + Send + Sync>;
type ViewService = Arc<dyn Fn() -> ViewResult<ViewValue> + Send + Sync>;

#[derive(Default)]
struct ViewEvents {
    shared: ViewData,
    creators: Vec<(String, ViewCallback)>,
    composers: Vec<(String, ViewCallback)>,
    components: HashMap<String, ComponentResolver>,
    anonymous_namespaces: HashMap<String, String>,
    anonymous_paths: HashMap<Option<String>, Vec<String>>,
    services: HashMap<String, ViewService>,
}

impl Factory {
    /// Exposes a typed application service as template data.
    pub fn registerViewService<T>(
        &self,
        name: &str,
        expose: impl Fn(&T) -> ViewResult<ViewValue> + Send + Sync + 'static,
    ) -> ViewResult<()>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        if name.trim().is_empty() {
            return Err(ViewError::InvalidName);
        }
        self.events.write().unwrap().services.insert(
            name.to_owned(),
            Arc::new(move || {
                let container = current_container().ok_or_else(|| {
                    render_error("Blade service injection requires an application container.")
                })?;
                let service = container
                    .make::<T>()
                    .map_err(|error| ViewError::RenderFailed(Box::new(error)))?;
                expose(service.as_ref())
            }),
        );
        Ok(())
    }

    pub(super) fn view_service(&self, name: &str) -> ViewResult<ViewValue> {
        let service = self
            .events
            .read()
            .unwrap()
            .services
            .get(name)
            .cloned()
            .ok_or_else(|| render_error("Blade view service was not registered."))?;
        service()
    }

    pub fn new(mut finder: FileViewFinder, compiled: PathBuf) -> Self {
        finder
            .addExtension("blade.html")
            .expect("valid Blade extension");
        Self {
            finder: Arc::new(finder),
            engines: Arc::new(EngineResolver::new(compiled)),
            events: Arc::default(),
            embedded: HashMap::new(),
            public_path: None,
        }
    }

    pub(crate) fn with_embedded_namespace(
        mut self,
        namespace: &str,
        views: &[(&str, &'static str)],
    ) -> Self {
        let paths: Vec<_> = self
            .finder
            .paths()
            .iter()
            .map(|root| root.join("vendor").join(namespace))
            .collect();
        let hints: Vec<_> = paths.iter().map(PathBuf::as_path).collect();
        self.finder
            .append_hints(namespace, &hints)
            .expect("valid embedded namespace");
        self.embedded.extend(
            views
                .iter()
                .map(|(name, source)| (format!("{namespace}::{name}"), *source)),
        );
        self
    }

    fn source(&self, name: &str) -> ViewResult<ResolvedSource<'_>> {
        match self.finder.source(name) {
            Ok(source) => Ok(ResolvedSource::File(source)),
            Err(ViewError::NotFound) => self
                .embedded
                .get_key_value(name.trim())
                .map(|(name, &source)| ResolvedSource::Embedded { name, source })
                .ok_or(ViewError::NotFound),
            Err(error) => Err(error),
        }
    }

    pub fn make(&self, name: impl Into<Cow<'static, str>>, data: ViewData) -> ViewResult<View> {
        let name = name.into();
        self.create(name, data)
    }

    pub fn first(&self, names: &[&str], data: ViewData) -> ViewResult<View> {
        for name in names {
            match self.source(name) {
                Ok(_) => return self.create((*name).to_owned().into(), data),
                Err(ViewError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Err(ViewError::NotFound)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.source(name).is_ok()
    }

    pub fn share(&self, key: impl Into<String>, value: impl Into<ViewValue>) {
        self.events
            .write()
            .unwrap()
            .shared
            .insert(key.into(), value.into());
    }

    pub fn creator(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&mut View) -> ViewResult<()> + Send + Sync + 'static,
    ) {
        self.events
            .write()
            .unwrap()
            .creators
            .push((name.into(), Arc::new(callback)));
    }

    pub fn composer(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&mut View) -> ViewResult<()> + Send + Sync + 'static,
    ) {
        self.events
            .write()
            .unwrap()
            .composers
            .push((name.into(), Arc::new(callback)));
    }

    pub fn component<C: Component>(
        &self,
        name: impl Into<String>,
        resolve: impl Fn(&ViewData) -> ViewResult<C> + Send + Sync + 'static,
    ) {
        self.events.write().unwrap().components.insert(
            name.into(),
            Arc::new(move |data| {
                let component = resolve(data)?;
                if !component.shouldRender() {
                    return Ok(ResolvedComponent::Hidden);
                }
                let output = component.render()?;
                Ok(ResolvedComponent::Visible {
                    output: Box::new(output),
                    data: component.data(),
                })
            }),
        );
    }

    pub(super) fn resolve_component(
        &self,
        name: &str,
        properties: &ViewData,
    ) -> ViewResult<Option<ResolvedComponent>> {
        let resolver = self.events.read().unwrap().components.get(name).cloned();
        resolver.map(|resolver| resolver(properties)).transpose()
    }

    /// Registers a filesystem directory for anonymous Blade components.
    pub fn anonymousComponentPath(&self, path: &Path, prefix: Option<&str>) -> ViewResult<()> {
        if prefix.is_some_and(|prefix| !valid_segment(prefix)) {
            return Err(ViewError::InvalidName);
        }
        let mount = format!("anonymous-components-{:032x}", fastrand::u128(..));
        self.finder.append_hints(&mount, &[path])?;
        self.events
            .write()
            .unwrap()
            .anonymous_paths
            .entry(prefix.map(str::to_owned))
            .or_default()
            .push(mount);
        Ok(())
    }

    /// Maps an anonymous component prefix to a directory within the view roots.
    pub fn anonymousComponentNamespace(
        &self,
        directory: &str,
        prefix: Option<&str>,
    ) -> ViewResult<()> {
        let prefix = prefix.unwrap_or(directory);
        let directory = directory.trim().trim_matches('/');
        if !valid_segment(prefix) {
            return Err(ViewError::InvalidName);
        }
        relative_name(directory)?;
        self.events
            .write()
            .unwrap()
            .anonymous_namespaces
            .insert(prefix.to_owned(), directory.replace('/', "."));
        Ok(())
    }

    fn anonymous_component_candidates(&self, name: &str) -> Vec<String> {
        let (prefix, name) = name
            .split_once("::")
            .map_or((None, name), |(prefix, name)| (Some(prefix), name));
        let mut bases = vec![prefix.map_or_else(
            || format!("components.{name}"),
            |prefix| format!("{prefix}::components.{name}"),
        )];
        let events = self.events.read().unwrap();
        if let Some(directory) = prefix.and_then(|prefix| events.anonymous_namespaces.get(prefix)) {
            bases.push(format!("{directory}.{name}"));
        }
        if let Some(mounts) = events.anonymous_paths.get(&prefix.map(str::to_owned)) {
            bases.extend(mounts.iter().map(|mount| format!("{mount}::{name}")));
        }
        let leaf = name.rsplit('.').next().unwrap_or(name);
        bases
            .into_iter()
            .flat_map(|base| {
                let index = format!("{base}.index");
                let directory = format!("{base}.{leaf}");
                [base, index, directory]
            })
            .collect()
    }

    pub(super) fn render_component(
        &self,
        name: &str,
        properties: &ViewData,
        component: Option<ResolvedComponent>,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        match component {
            Some(ResolvedComponent::Visible {
                output,
                data: component_data,
            }) => {
                let mut data = properties.clone();
                data.extend(component_data);
                match *output {
                    ComponentOutput::View(mut view) => {
                        data.extend(std::mem::take(&mut view.data));
                        view.data = data;
                        self.render_view(view, context)
                    }
                    ComponentOutput::InlineTemplate(source) => {
                        self.compiler().render_inline(name, &source, &data, context)
                    }
                }
            }
            Some(ResolvedComponent::Hidden) => Ok(String::new()),
            None => {
                let candidates = self.anonymous_component_candidates(name);
                let candidates: Vec<_> = candidates.iter().map(String::as_str).collect();
                let view = self.first(&candidates, properties.clone())?;
                self.render_view(view, context)
            }
        }
    }

    fn create(&self, name: Cow<'static, str>, data: ViewData) -> ViewResult<View> {
        let mut view = View {
            name,
            data,
            binding: None,
            created: false,
            creation_error: None,
            fragments: None,
        };
        self.callCreator(&mut view)?;
        Ok(view)
    }

    pub(super) fn callCreator(&self, view: &mut View) -> ViewResult<()> {
        if !view.created {
            self.view_source(view)?;
            self.call_events(view, |events| &events.creators)?;
            view.created = true;
        }
        Ok(())
    }

    fn call_events(
        &self,
        view: &mut View,
        select: impl FnOnce(&ViewEvents) -> &[(String, ViewCallback)],
    ) -> ViewResult<()> {
        let callbacks: Vec<_> = select(&self.events.read().unwrap())
            .iter()
            .filter(|(name, _)| matches_view(name, &view.name))
            .map(|(_, callback)| callback.clone())
            .collect();
        for callback in callbacks {
            callback(view)?;
        }
        Ok(())
    }

    pub fn render(&self, name: &str, data: &ViewData) -> ViewResult<String> {
        self.renderView(self.make(name.to_owned(), data.clone())?)
    }

    pub fn renderView(&self, view: View) -> ViewResult<String> {
        self.render_view(view, &mut RenderContext::new(Some(self)))
    }

    /// Renders a trusted inline Blade template using this application's views.
    pub fn renderInline(&self, source: &str, data: &ViewData) -> ViewResult<String> {
        self.compiler()
            .render_inline("", source, data, &mut RenderContext::new(Some(self)))
    }

    pub(super) fn render_in_request(
        &self,
        view: View,
        request: ViewRequestData,
    ) -> ViewResult<String> {
        let mut context = RenderContext::new(Some(self));
        context.request = Arc::new(request);
        self.render_view(view, &mut context)
    }

    pub(super) fn render_nested(
        &self,
        name: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        self.render_view(self.make(name.to_owned(), data.clone())?, context)
    }

    pub(super) fn render_view(
        &self,
        mut view: View,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let mut data = self.prepare_view(&mut view)?;
        let source = self.view_source(&view)?;
        data.extend(view.data);
        let html = self.render_contents(source, &data, context)?;
        Ok(match view.fragments {
            Some(fragments) => context.selected_fragments(fragments),
            None => html,
        })
    }

    fn render_view_ref(
        &self,
        view: &mut View,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let mut data = self.prepare_view(view)?;
        data.extend(
            view.data
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        self.render_contents(self.view_source(view)?, &data, context)
    }

    fn prepare_view(&self, view: &mut View) -> ViewResult<ViewData> {
        if let Some(error) = view.creation_error.take() {
            return Err(error);
        }
        self.callCreator(view)?;
        self.call_events(view, |events| &events.composers)?;
        let mut data = self.events.read().unwrap().shared.clone();
        if let Some(errors) = super::middleware::shared_errors() {
            data.insert("errors".into(), errors);
        }
        Ok(data)
    }

    fn view_source(&self, view: &View) -> ViewResult<ResolvedSource<'_>> {
        if let Some(path) = view
            .binding
            .as_ref()
            .and_then(|binding| binding.file.as_deref())
        {
            self.finder.source_file(path).map(ResolvedSource::File)
        } else {
            self.source(&view.name)
        }
    }

    fn render_contents(
        &self,
        source: ResolvedSource<'_>,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let source = match source {
            ResolvedSource::File(source) => source,
            ResolvedSource::Embedded { name, source } => {
                return self.compiler().render_with(
                    &Path::new("embedded").join(name),
                    source,
                    data,
                    context,
                );
            }
        };
        let engine = if is_blade_path(&source.path) {
            "blade"
        } else {
            "file"
        };
        self.engines.resolve(engine)?.render(source, data, context)
    }

    pub fn getFinder(&self) -> &FileViewFinder {
        &self.finder
    }

    pub fn getEngineResolver(&self) -> &EngineResolver {
        &self.engines
    }

    pub(crate) fn compiler(&self) -> &BladeCompiler {
        self.engines.blade().getCompiler()
    }

    pub(super) fn cache_paths(&self) -> Vec<PathBuf> {
        self.finder.cache_paths()
    }

    pub(crate) fn render_error(&self, name: &str, status: u16, message: &str) -> Option<String> {
        let data = ViewData::from_iter([
            ("status".to_owned(), status.into()),
            ("message".to_owned(), message.into()),
        ]);
        self.render(name, &data).ok()
    }
}

impl ContractFactory for Factory {
    fn exists(&self, view: &str) -> bool {
        self.exists(view)
    }

    fn file(
        &self,
        path: &Path,
        data: ViewData,
        mut merge_data: ViewData,
    ) -> ViewResult<Box<dyn ContractView>> {
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("html" | "css")
        ) {
            return Err(ViewError::UnknownEngine);
        }
        merge_data.extend(data);
        let mut view = View {
            name: path
                .to_str()
                .ok_or(ViewError::InvalidName)?
                .to_owned()
                .into(),
            data: merge_data,
            binding: Some(Box::new(ViewBinding {
                factory: self.clone(),
                file: Some(path.to_owned()),
            })),
            created: false,
            creation_error: None,
            fragments: None,
        };
        self.callCreator(&mut view)?;
        Ok(Box::new(view))
    }

    fn make(
        &self,
        name: &str,
        data: ViewData,
        mut merge_data: ViewData,
    ) -> ViewResult<Box<dyn ContractView>> {
        merge_data.extend(data);
        let mut view = self.make(name.to_owned(), merge_data)?;
        view.binding = Some(Box::new(ViewBinding {
            factory: self.clone(),
            file: None,
        }));
        Ok(Box::new(view))
    }

    fn share(&self, key: &str, value: ViewValue) -> ViewValue {
        self.share(key, value.clone());
        value
    }

    fn composer(&self, views: &[&str], callback: Arc<ContractCallback>) {
        for &view in views {
            let callback = callback.clone();
            self.composer(view, move |view| callback(view));
        }
    }

    fn creator(&self, views: &[&str], callback: Arc<ContractCallback>) {
        for &view in views {
            let callback = callback.clone();
            self.creator(view, move |view| callback(view));
        }
    }

    fn addNamespace(&self, namespace: &str, hints: &[&Path]) -> ViewResult<()> {
        self.finder.append_hints(namespace, hints)
    }

    fn replaceNamespace(&self, namespace: &str, hints: &[&Path]) -> ViewResult<()> {
        self.finder.replace_hints(namespace, hints)
    }
}

impl ContractView for View {
    fn name(&self) -> &str {
        self.name()
    }

    fn with(&mut self, key: &str, value: ViewValue) -> &mut dyn ContractView {
        self.data_mut().insert(key.to_owned(), value);
        self
    }

    fn getData(&self) -> &ViewData {
        self.data()
    }

    fn render(&mut self) -> ViewResult<String> {
        use crate::support::facades::{Facade as _, View as ViewFacade};
        let factory = match &self.binding {
            Some(binding) => binding.factory.clone(),
            None => ViewFacade::getFacadeRoot()
                .map_err(ViewError::Unavailable)?
                .as_ref()
                .clone(),
        };
        factory.render_view_ref(self, &mut RenderContext::new(Some(&factory)))
    }
}

pub(super) fn is_blade_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".blade.html"))
}

fn matches_view(pattern: &str, name: &str) -> bool {
    let Some((prefix, rest)) = pattern.split_once('*') else {
        return pattern == name;
    };
    let Some(name) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some((middle, suffix)) = rest.rsplit_once('*') else {
        return name.ends_with(rest);
    };
    let Some(name) = name.strip_suffix(suffix) else {
        return false;
    };
    middle
        .split('*')
        .try_fold(name, |remaining, part| {
            remaining.split_once(part).map(|(_, tail)| tail)
        })
        .is_some()
}
