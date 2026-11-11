#![allow(non_snake_case)]

use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use super::{
    Component, ComponentOutput, FileViewFinder, View, ViewData, ViewError, ViewFinder, ViewResult,
    ViewValue, compilers::BladeCompiler, engines::EngineResolver, rendering::RenderContext,
};

/// Creates named views from the application's finder and engines.
pub struct Factory {
    finder: FileViewFinder,
    engines: EngineResolver,
    events: RwLock<ViewEvents>,
}

type ComponentRenderer = Arc<dyn Fn(&ViewData) -> ViewResult<ComponentOutput> + Send + Sync>;

type ViewCallback = Arc<dyn Fn(&mut View) -> ViewResult<()> + Send + Sync>;

#[derive(Default)]
struct ViewEvents {
    shared: ViewData,
    creators: Vec<(String, ViewCallback)>,
    composers: Vec<(String, ViewCallback)>,
    components: HashMap<String, ComponentRenderer>,
}

impl Factory {
    pub fn new(mut finder: FileViewFinder, compiled: PathBuf) -> Self {
        finder
            .addExtension("blade.html")
            .expect("valid Blade extension");
        Self {
            finder,
            engines: EngineResolver::new(compiled),
            events: RwLock::default(),
        }
    }

    pub fn make(&self, name: impl Into<Cow<'static, str>>, data: ViewData) -> ViewResult<View> {
        let name = name.into();
        self.create(name, data)
    }

    pub fn first(&self, names: &[&str], data: ViewData) -> ViewResult<View> {
        for name in names {
            match self.finder.find(name) {
                Ok(_) => return self.create((*name).to_owned().into(), data),
                Err(ViewError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Err(ViewError::NotFound)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.finder.find(name).is_ok()
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
        self.events
            .write()
            .unwrap()
            .components
            .insert(name.into(), Arc::new(move |data| resolve(data)?.render()));
    }

    pub(super) fn render_component(
        &self,
        name: &str,
        properties: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let component = self.events.read().unwrap().components.get(name).cloned();
        match component {
            Some(component) => match component(properties)? {
                ComponentOutput::View(mut view) => {
                    let mut data = properties.clone();
                    data.extend(std::mem::take(&mut view.data));
                    view.data = data;
                    self.render_view(view, context)
                }
                ComponentOutput::InlineTemplate(source) => {
                    self.compiler().render_inline(&source, properties, context)
                }
            },
            None => self.render_nested(&format!("components.{name}"), properties, context),
        }
    }

    fn create(&self, name: Cow<'static, str>, data: ViewData) -> ViewResult<View> {
        let mut view = View {
            name,
            data,
            created: false,
            creation_error: None,
        };
        self.callCreator(&mut view)?;
        Ok(view)
    }

    pub(super) fn callCreator(&self, view: &mut View) -> ViewResult<()> {
        if !view.created {
            self.finder.find(&view.name)?;
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

    pub(super) fn render_nested(
        &self,
        name: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        self.render_view(self.make(name.to_owned(), data.clone())?, context)
    }

    fn render_view(&self, mut view: View, context: &mut RenderContext<'_>) -> ViewResult<String> {
        if let Some(error) = view.creation_error.take() {
            return Err(error);
        }
        self.callCreator(&mut view)?;
        self.call_events(&mut view, |events| &events.composers)?;
        let mut data = self.events.read().unwrap().shared.clone();
        data.extend(view.data);
        self.render_contents(&view.name, &data, context)
    }

    fn render_contents(
        &self,
        name: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let source = self.finder.source(name)?;
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

    pub(crate) fn paths(&self) -> &[PathBuf] {
        self.finder.paths()
    }

    pub(crate) fn render_error(&self, name: &str, status: u16, message: &str) -> Option<String> {
        let data = ViewData::from_iter([
            ("status".to_owned(), status.into()),
            ("message".to_owned(), message.into()),
        ]);
        self.render(name, &data).ok()
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
