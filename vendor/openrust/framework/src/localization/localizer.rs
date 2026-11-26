use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use crate::contracts::localization::{Loader, Localizer as Contract};

use super::{
    FileLoader, Line, Lines, MessageSelector, PluralCount, Replacement, Selector, TranslationError,
    TranslationResource, TranslationResult,
    file_loader::{
        merge::{indexed, merge},
        valid_group, valid_segment,
    },
    locale::canonical_locale,
    lookup::{LookupHooks, MissingTranslation},
    replacement::{Stringables, replace},
};

type Replacements = BTreeMap<String, String>;
type SourceCatalog = HashMap<CatalogKey, Arc<Lines>>;

#[derive(Clone, Eq, Hash, PartialEq)]
struct CatalogKey {
    locale: String,
    group: String,
    namespace: Option<String>,
}

#[derive(Default)]
struct Catalog {
    lines: Arc<Lines>,
    runtime: Option<Arc<Lines>>,
    inherits_file_on_release: bool,
}

struct ResolvedLine {
    line: Line,
    locale: String,
    literal: bool,
}

impl ResolvedLine {
    fn exists(&self, key: &str) -> bool {
        self.literal || !matches!(&self.line, Line::Text(text) if text == key)
    }
}

tokio::task_local! {
    static ACTIVE_LOCALES: std::cell::RefCell<HashMap<usize, String>>;
}

/// 将临时语言作用域传入阻塞模板渲染，不写入应用默认值或请求容器状态。
pub(crate) struct LocaleSnapshot(HashMap<usize, String>);

impl LocaleSnapshot {
    pub(crate) fn capture() -> Self {
        Self(
            ACTIVE_LOCALES
                .try_with(|locales| locales.borrow().clone())
                .unwrap_or_default(),
        )
    }

    pub(crate) fn run<R>(self, operation: impl FnOnce() -> R) -> R {
        ACTIVE_LOCALES.sync_scope(std::cell::RefCell::new(self.0), operation)
    }
}

#[derive(Clone)]
struct Locales {
    locale: String,
    fallback: String,
}

/// 每个请求／队列任务保留自己的语言设置，应用启动配置作为初值。
#[derive(Default)]
pub(crate) struct LocaleState(Mutex<HashMap<usize, (Arc<()>, Locales)>>);

/// Resolves translations using one application's catalog and locale defaults.
pub struct Localizer {
    loader: Arc<FileLoader>,
    selector: RwLock<Arc<dyn Selector>>,
    defaults: Mutex<Locales>,
    owner: std::sync::Weak<crate::container::Container>,
    loaded: Mutex<HashMap<CatalogKey, Catalog>>,
    default_catalog: Mutex<Arc<SourceCatalog>>,
    identity: Arc<()>,
    hooks: LookupHooks,
    stringables: Stringables,
}

impl Localizer {
    pub fn new(loader: FileLoader, locale: String, fallback: String) -> TranslationResult<Self> {
        Self::with_loader(Arc::new(loader), locale, fallback)
    }

    pub(super) fn with_loader(
        loader: Arc<FileLoader>,
        locale: String,
        fallback: String,
    ) -> TranslationResult<Self> {
        Ok(Self {
            loader,
            selector: RwLock::new(Arc::new(MessageSelector)),
            defaults: Mutex::new(Locales {
                locale: canonical_locale(&locale)?,
                fallback: canonical_locale(&fallback)?,
            }),
            owner: std::sync::Weak::new(),
            loaded: Mutex::default(),
            default_catalog: Mutex::default(),
            identity: Arc::new(()),
            hooks: LookupHooks::default(),
            stringables: Stringables::default(),
        })
    }

    pub(crate) fn add_package_namespace(
        &self,
        namespace: &str,
        path: PathBuf,
    ) -> TranslationResult<()> {
        if !valid_segment(namespace) {
            return Err(TranslationError::InvalidKey);
        }
        let mut loaded = self.loaded.lock().unwrap();
        self.loader
            .register_namespace_path(namespace.to_owned(), path);
        loaded.retain(|key, catalog| {
            catalog.runtime.is_some() || key.namespace.as_deref() != Some(namespace)
        });
        Ok(())
    }

    pub(crate) fn add_package_json_path(&self, path: PathBuf) {
        let mut loaded = self.loaded.lock().unwrap();
        self.loader.add_json_path(path);
        loaded.retain(|key, catalog| {
            catalog.runtime.is_some() || key.group != "*" || key.namespace.as_deref() != Some("*")
        });
    }

    pub(crate) fn add_bundled_package_namespace(
        &self,
        namespace: &str,
        files: &'static [(&'static str, &'static str)],
    ) -> TranslationResult<()> {
        if !valid_segment(namespace) {
            return Err(TranslationError::InvalidKey);
        }
        let mut loaded = self.loaded.lock().unwrap();
        self.loader
            .register_bundled_namespace(namespace.to_owned(), files);
        loaded.retain(|key, catalog| {
            catalog.runtime.is_some() || key.namespace.as_deref() != Some(namespace)
        });
        Ok(())
    }

    /// Adds a language directory for subsequent group and literal JSON loads.
    /// 返回当前选择器句柄，替换后已取出的句柄仍可继续使用。
    pub fn get_selector(&self) -> Arc<dyn Selector> {
        self.selector
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn set_selector(&self, selector: impl Selector + 'static) {
        *self
            .selector
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(selector);
    }

    pub fn get_loader(&self) -> Arc<FileLoader> {
        self.loader.clone()
    }

    pub fn add_path(&self, path: PathBuf) {
        self.loader.add_path(path);
    }

    /// Adds a directory for subsequent anonymous JSON catalog loads.
    pub fn add_json_path(&self, path: PathBuf) {
        self.loader.add_json_path(path);
    }

    /// Adds or replaces a package namespace for subsequent loads.
    pub fn add_namespace(&self, namespace: &str, path: PathBuf) -> TranslationResult<()> {
        if !valid_segment(namespace) {
            return Err(TranslationError::InvalidKey);
        }
        self.loader
            .register_namespace_path(namespace.to_owned(), path);
        Ok(())
    }

    /// Loads and caches a group so later runtime lines can extend it.
    pub fn load(
        &self,
        namespace: Option<&str>,
        group: &str,
        locale: &str,
    ) -> TranslationResult<()> {
        let locale = canonical_locale(locale)?;
        let namespace = catalog_namespace(group, namespace)?;
        let sources = self.default_catalog.lock().unwrap().clone();
        self.catalog(&sources, &locale, group, namespace)?;
        Ok(())
    }

    /// Materializes exact groups from an asynchronous loader before publishing them together.
    pub async fn load_from(
        &self,
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        let keys = catalog_keys(resources.iter().copied())?;
        let versions: Vec<_> = {
            let loaded = self.loaded.lock().unwrap();
            keys.iter()
                .map(|key| runtime_lines(&loaded, key).cloned())
                .collect()
        };
        let incoming = materialize(loader, &keys).await?;
        let mut loaded = self.loaded.lock().unwrap();
        for (key, previous) in keys.iter().zip(&versions) {
            match (runtime_lines(&loaded, key), previous.as_ref()) {
                (None, None) => (),
                (Some(current), Some(previous)) if Arc::ptr_eq(current, previous) => (),
                _ => return Err(TranslationError::CatalogChanged),
            }
        }
        for (key, lines) in keys.into_iter().zip(incoming) {
            loaded.insert(
                key,
                Catalog {
                    lines: lines.clone(),
                    runtime: Some(lines),
                    inherits_file_on_release: false,
                },
            );
        }
        Ok(())
    }

    /// 按当前应用配置刷新默认来源，保留运行时覆盖。
    pub async fn refresh_default_source(&self) -> TranslationResult<()> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(TranslationError::DefaultSourceUnavailable)?;
        let resources = super::default_source::resources(&owner)?;
        self.refresh_configured(&owner, &resources).await
    }

    pub(super) async fn refresh_configured(
        &self,
        container: &crate::container::Container,
        resources: &[super::default_source::ConfiguredResource],
    ) -> TranslationResult<()> {
        use crate::contracts::container::Container as _;

        let previous = self.default_catalog.lock().unwrap().clone();
        let keys = catalog_keys(
            resources
                .iter()
                .map(super::default_source::ConfiguredResource::as_resource),
        )?;
        let incoming = if keys.is_empty() {
            HashMap::new()
        } else {
            let loader = crate::container::ScopeSnapshot::without(container)
                .run(|| container.get::<dyn Loader>("lang.loader"))?;
            let lines = materialize(loader.as_ref(), &keys).await?;
            keys.into_iter().zip(lines).collect()
        };

        let mut published = self.default_catalog.lock().unwrap();
        if !Arc::ptr_eq(&published, &previous) {
            return Err(TranslationError::CatalogChanged);
        }
        *published = Arc::new(incoming);
        Ok(())
    }

    /// Adds dotted keys to the loaded groups of the given locale and namespace.
    pub fn add_lines(
        &self,
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<()> {
        let locale = canonical_locale(locale)?;
        let entries = lines
            .into_iter()
            .map(|(key, value)| {
                let (group, item) = key.split_once('.').ok_or(TranslationError::InvalidKey)?;
                if item.is_empty() {
                    return Err(TranslationError::InvalidKey);
                }
                let namespace = catalog_namespace(group, namespace)?;
                Ok((
                    CatalogKey {
                        locale: locale.clone(),
                        group: group.to_owned(),
                        namespace: namespace.map(str::to_owned),
                    },
                    item.to_owned(),
                    value,
                ))
            })
            .collect::<TranslationResult<Vec<_>>>()?;
        let sources = self.default_catalog.lock().unwrap().clone();
        let mut loaded = self.loaded.lock().unwrap();
        for (key, item, line) in entries {
            let inherits_file_on_release = sources.contains_key(&key);
            let catalog = loaded.entry(key).or_insert_with(|| Catalog {
                inherits_file_on_release,
                ..Catalog::default()
            });
            set_line(Arc::make_mut(&mut catalog.lines), &item, line.clone());
            set_line(
                Arc::make_mut(catalog.runtime.get_or_insert_with(Arc::default)),
                &item,
                line,
            );
        }
        Ok(())
    }

    /// 合并运行时JSON译文；完整句子按字面键处理，保留同语言的其他文件译文。
    pub fn add_json_lines(
        &self,
        lines: impl IntoIterator<Item = (String, Line)>,
        locale: &str,
    ) -> TranslationResult<()> {
        let locale = canonical_locale(locale)?;
        let lines: Lines = lines.into_iter().collect();
        let key = CatalogKey {
            locale,
            group: "*".into(),
            namespace: Some("*".into()),
        };
        let sources = self.default_catalog.lock().unwrap().clone();
        let inherits_file_on_release = sources.contains_key(&key);
        if !inherits_file_on_release {
            self.catalog(&sources, &key.locale, &key.group, key.namespace.as_deref())?;
        }
        let mut loaded = self.loaded.lock().unwrap();
        let catalog = loaded.entry(key).or_insert_with(|| Catalog {
            inherits_file_on_release,
            ..Catalog::default()
        });
        for (item, line) in lines {
            Arc::make_mut(&mut catalog.lines).insert(item.clone(), line.clone());
            Arc::make_mut(catalog.runtime.get_or_insert_with(Arc::default)).insert(item, line);
        }
        Ok(())
    }

    pub fn locale(&self) -> String {
        ACTIVE_LOCALES
            .try_with(|locales| locales.borrow().get(&self.identity()).cloned())
            .ok()
            .flatten()
            .unwrap_or_else(|| self.locales().locale)
    }

    pub async fn with_locale<T>(
        &self,
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        let locale = canonical_locale(locale)?;
        let mut locales = ACTIVE_LOCALES
            .try_with(|locales| locales.borrow().clone())
            .unwrap_or_default();
        locales.insert(self.identity(), locale);
        Ok(ACTIVE_LOCALES
            .scope(std::cell::RefCell::new(locales), operation)
            .await)
    }

    fn identity(&self) -> usize {
        Arc::as_ptr(&self.identity) as usize
    }

    /// Checks the selected locale and, when enabled, the configured fallback order.
    pub fn has(&self, key: &str, locale: Option<&str>, fallback: bool) -> TranslationResult<bool> {
        let locale = self.resolve_locale(locale)?;
        Ok(self
            .lookup(key, &locale, fallback)?
            .is_some_and(|line| line.exists(key)))
    }

    pub fn has_for_locale(&self, key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        self.has(key, locale, false)
    }

    /// Installs an application-level handler for missing keys.
    pub fn handle_missing_keys_using(
        &self,
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.hooks.set_missing(Some(Arc::new(handler)));
    }

    pub fn clear_missing_key_handler(&self) {
        self.hooks.set_missing(None);
    }

    /// Replaces the search order for grouped translations with fallback enabled.
    pub fn determine_locales_using(
        &self,
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) {
        self.hooks.set_locales(Some(Arc::new(resolver)));
    }

    pub fn clear_locale_resolver(&self) {
        self.hooks.set_locales(None);
    }

    fn missing(&self, key: &str, replacements: &Replacements, locale: &str) -> String {
        self.hooks.missing(MissingTranslation {
            key,
            replacements,
            mixed_replacements: &[],
            locale,
            fallback: true,
        })
    }

    fn missing_with(&self, key: &str, args: &[(&str, Replacement<'_>)], locale: &str) -> String {
        let replacements = args
            .iter()
            .filter_map(|(name, value)| Some(((*name).into(), value.as_text()?.into())))
            .collect();
        self.hooks.missing(MissingTranslation {
            key,
            replacements: &replacements,
            mixed_replacements: args,
            locale,
            fallback: true,
        })
    }

    /// 模板助手保留分组／列表原值，并共享译器的缺键与回退处理。
    pub(crate) fn value(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
        fallback: bool,
    ) -> TranslationResult<Line> {
        let locale = self.resolve_locale(locale)?;
        let line = self
            .lookup(key, &locale, fallback)?
            .map(|found| found.line)
            .unwrap_or_else(|| {
                Line::Text(self.hooks.missing(MissingTranslation {
                    key,
                    replacements,
                    mixed_replacements: &[],
                    locale: &locale,
                    fallback,
                }))
            });
        map_line(line, &|text| Ok(replace(&text, replacements)))
    }

    pub fn string(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let text = self.string_line(key, locale, |locale| {
            self.missing(key, replacements, locale)
        })?;
        Ok(replace(&text, replacements))
    }

    /// Translates text with ordered scalar, tag, and object replacements.
    pub fn string_with(
        &self,
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let text = self.string_line(key, locale, |locale| self.missing_with(key, args, locale))?;
        self.stringables.replace(text, args, None)
    }

    fn string_line(
        &self,
        key: &str,
        locale: Option<&str>,
        missing: impl FnOnce(&str) -> String,
    ) -> TranslationResult<String> {
        let locale = self.resolve_locale(locale)?;
        match self.lookup(key, &locale, true)?.map(|found| found.line) {
            Some(Line::Text(text)) => Ok(text),
            Some(_) => Err(TranslationError::UnexpectedType),
            None => Ok(missing(&locale)),
        }
    }

    pub fn array(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        let lines = self.array_lines(key, locale, |locale| {
            self.missing(key, replacements, locale)
        })?;
        map_lines(lines, &|text| Ok(replace(&text, replacements)))
    }

    /// Applies ordered replacements to every text leaf in a group or list.
    pub fn array_with(
        &self,
        key: &str,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        let lines = self.array_lines(key, locale, |locale| self.missing_with(key, args, locale))?;
        map_lines(lines, &|text| self.stringables.replace(text, args, None))
    }

    fn array_lines(
        &self,
        key: &str,
        locale: Option<&str>,
        missing: impl FnOnce(&str) -> String,
    ) -> TranslationResult<Lines> {
        let locale = self.resolve_locale(locale)?;
        match self.lookup(key, &locale, true)?.map(|found| found.line) {
            Some(Line::Group(lines)) => Ok(lines),
            Some(Line::List(lines)) => Ok(indexed(lines)),
            Some(_) => Err(TranslationError::UnexpectedType),
            None => {
                missing(&locale);
                Err(TranslationError::UnexpectedType)
            }
        }
    }

    pub fn choose(
        &self,
        key: &str,
        count: impl Into<PluralCount>,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let count = count.into();
        let chosen = self.select_choice(key, count, locale)?;
        let mut replacements = replacements.clone();
        replacements
            .entry("count".into())
            .or_insert_with(|| count.to_string());
        Ok(replace(&chosen, &replacements))
    }

    /// Selects a plural form before invoking replacement callbacks.
    pub fn choice_with(
        &self,
        key: &str,
        count: impl Into<PluralCount>,
        args: &[(&str, Replacement<'_>)],
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let count = count.into();
        let chosen = self.select_choice(key, count, locale)?;
        self.stringables
            .replace(chosen, args, Some(count.to_string()))
    }

    /// Registers a formatter for one concrete Rust type on this localizer.
    pub fn stringable<T: Any>(&self, formatter: impl Fn(&T) -> String + Send + Sync + 'static) {
        self.stringables.register(formatter);
    }

    fn select_choice(
        &self,
        key: &str,
        count: PluralCount,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let locale = self.resolve_locale(locale)?;
        let found = match self
            .lookup(key, &locale, false)?
            .filter(|found| found.exists(key))
        {
            Some(line) => Some(line),
            None => self.lookup(key, &self.get_fallback(), true)?,
        };
        let (line, selected) = match found {
            Some(ResolvedLine {
                line: Line::Text(line),
                locale,
                ..
            }) => (line, locale),
            Some(_) => return Err(TranslationError::UnexpectedType),
            None => (
                self.missing(key, &Replacements::new(), &self.get_fallback()),
                self.get_fallback(),
            ),
        };
        self.get_selector().choose(&line, count, &selected)
    }

    fn resolve_locale(&self, locale: Option<&str>) -> TranslationResult<String> {
        match locale {
            Some(locale) => canonical_locale(locale),
            None => Ok(self.locale()),
        }
    }

    fn lookup(
        &self,
        key: &str,
        locale: &str,
        fallback: bool,
    ) -> TranslationResult<Option<ResolvedLine>> {
        let sources = self.default_catalog.lock().unwrap().clone();
        if let Some(line) = self
            .catalog(&sources, locale, "*", Some("*"))?
            .get(key)
            .cloned()
        {
            let line = match line {
                Line::Text(text) if text.is_empty() || text == "0" => Line::Text(key.to_owned()),
                line => line,
            };
            return Ok(Some(ResolvedLine {
                line,
                locale: locale.to_owned(),
                literal: true,
            }));
        }
        let (namespace, name) = key
            .split_once("::")
            .map_or((None, key), |(namespace, name)| (Some(namespace), name));
        let (group, item) = name
            .split_once('.')
            .map_or((name, None), |(group, item)| (group, Some(item)));
        if !valid_group(group) || namespace.is_some_and(|namespace| !valid_segment(namespace)) {
            return Ok(None);
        }
        if !fallback {
            return Ok(self
                .group_line(&sources, locale, group, namespace, item)?
                .map(|line| ResolvedLine {
                    line,
                    locale: locale.to_owned(),
                    literal: false,
                }));
        }
        for locale in self.hooks.locales(locale, &self.get_fallback())? {
            if let Some(line) = self.group_line(&sources, &locale, group, namespace, item)? {
                return Ok(Some(ResolvedLine {
                    line,
                    locale,
                    literal: false,
                }));
            }
        }
        Ok(None)
    }

    fn group_line(
        &self,
        sources: &SourceCatalog,
        locale: &str,
        group: &str,
        namespace: Option<&str>,
        item: Option<&str>,
    ) -> TranslationResult<Option<Line>> {
        let lines = self.catalog(sources, locale, group, namespace)?;
        Ok(match item {
            Some(item) => find(&lines, item).cloned(),
            None => Some(Line::Group((*lines).clone())),
        }
        .filter(|line| match line {
            Line::Group(lines) => !lines.is_empty(),
            Line::List(lines) => !lines.is_empty(),
            Line::Text(_) => true,
        }))
    }

    fn catalog(
        &self,
        sources: &SourceCatalog,
        locale: &str,
        group: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<Arc<Lines>> {
        let key = CatalogKey {
            locale: locale.to_owned(),
            group: group.to_owned(),
            namespace: namespace.map(str::to_owned),
        };
        let mut loaded = self.loaded.lock().unwrap();
        if let Some(lines) = sources.get(&key) {
            if let Some(runtime) = runtime_lines(&loaded, &key) {
                let mut lines = (**lines).clone();
                if group == "*" {
                    lines.extend((**runtime).clone());
                } else {
                    merge(&mut lines, (**runtime).clone());
                }
                return Ok(Arc::new(lines));
            }
            return Ok(lines.clone());
        }
        match loaded.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let catalog = entry.into_mut();
                if catalog.inherits_file_on_release {
                    let mut lines = self.loader.load_lines(locale, group, namespace)?;
                    merge(&mut lines, (*catalog.lines).clone());
                    catalog.lines = Arc::new(lines);
                    catalog.inherits_file_on_release = false;
                }
                Ok(catalog.lines.clone())
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let lines = Arc::new(self.loader.load_lines(locale, group, namespace)?);
                entry.insert(Catalog {
                    lines: lines.clone(),
                    ..Catalog::default()
                });
                Ok(lines)
            }
        }
    }
}

impl Localizer {
    pub(crate) fn with_owner(
        mut self,
        owner: std::sync::Weak<crate::container::Container>,
    ) -> Self {
        self.owner = owner;
        self
    }
    fn state(&self) -> Option<Arc<LocaleState>> {
        use crate::contracts::container::Container as _;
        self.owner
            .upgrade()
            .filter(|owner| {
                owner.has_active_scope() && !crate::foundation::Application::is_bootstrapping(owner)
            })
            .and_then(|owner| owner.make().ok())
    }
    fn locales(&self) -> Locales {
        let defaults = self.defaults.lock().expect("语言默认值锁中毒").clone();
        self.state().map_or(defaults.clone(), |state| {
            state
                .0
                .lock()
                .expect("语言作用域锁中毒")
                .entry(self.identity())
                .or_insert_with(|| (self.identity.clone(), defaults))
                .1
                .clone()
        })
    }
    fn change_locales(&self, change: impl FnOnce(&mut Locales)) {
        if let Some(state) = self.state() {
            let defaults = self.defaults.lock().expect("语言默认值锁中毒").clone();
            change(
                &mut state
                    .0
                    .lock()
                    .expect("语言作用域锁中毒")
                    .entry(self.identity())
                    .or_insert_with(|| (self.identity.clone(), defaults))
                    .1,
            );
        } else {
            change(&mut self.defaults.lock().expect("语言默认值锁中毒"));
        }
    }
    pub fn get_locale(&self) -> String {
        self.locale()
    }
    pub fn set_locale(&self, locale: &str) -> TranslationResult<()> {
        <Self as Contract>::set_locale(self, locale)
    }
    pub fn get_fallback(&self) -> String {
        self.locales().fallback
    }
    pub fn set_fallback(&self, fallback: &str) -> TranslationResult<()> {
        <Self as Contract>::set_fallback(self, fallback)
    }
}

impl Contract for Localizer {
    fn get_fallback(&self) -> String {
        self.get_fallback()
    }
    fn set_fallback(&self, fallback: &str) -> TranslationResult<()> {
        let fallback = canonical_locale(fallback)?;
        self.change_locales(|locales| locales.fallback = fallback);
        Ok(())
    }

    fn get(&self, key: &str, replacements: &Replacements, locale: Option<&str>) -> String {
        self.string(key, replacements, locale)
            .unwrap_or_else(|_| replace(key, replacements))
    }

    fn choice(
        &self,
        key: &str,
        count: u64,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> String {
        i64::try_from(count)
            .ok()
            .and_then(|count| self.choose(key, count, replacements, locale).ok())
            .unwrap_or_else(|| replace(key, replacements))
    }

    fn array(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        self.array(key, replacements, locale)
    }

    fn get_locale(&self) -> String {
        self.locale()
    }

    fn set_locale(&self, locale: &str) -> TranslationResult<()> {
        let locale = canonical_locale(locale)?;
        let active = ACTIVE_LOCALES
            .try_with(|locales| {
                let mut locales = locales.borrow_mut();
                if let Some(active) = locales.get_mut(&self.identity()) {
                    active.clone_from(&locale);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if !active {
            self.change_locales(|locales| locales.locale = locale);
        }
        Ok(())
    }
}

fn catalog_keys<'a>(
    resources: impl IntoIterator<Item = TranslationResource<'a>>,
) -> TranslationResult<Vec<CatalogKey>> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for resource in resources {
        let locale = canonical_locale(resource.locale)?;
        let namespace = catalog_namespace(resource.group, resource.namespace)?;
        let key = CatalogKey {
            locale,
            group: resource.group.to_owned(),
            namespace: namespace.map(str::to_owned),
        };
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    Ok(keys)
}

async fn materialize(
    loader: &dyn Loader,
    keys: &[CatalogKey],
) -> TranslationResult<Vec<Arc<Lines>>> {
    let mut incoming = Vec::with_capacity(keys.len());
    for key in keys {
        let lines = loader
            .load(&key.locale, &key.group, key.namespace.as_deref())
            .await?;
        incoming.push(Arc::new(lines));
    }
    Ok(incoming)
}

fn runtime_lines<'a>(
    loaded: &'a HashMap<CatalogKey, Catalog>,
    key: &CatalogKey,
) -> Option<&'a Arc<Lines>> {
    loaded.get(key).and_then(|catalog| catalog.runtime.as_ref())
}

fn catalog_namespace<'a>(
    group: &str,
    namespace: Option<&'a str>,
) -> TranslationResult<Option<&'a str>> {
    let namespace = namespace.filter(|namespace| *namespace != "*");
    if group == "*" && namespace.is_none() {
        return Ok(Some("*"));
    }
    if !valid_group(group) || namespace.is_some_and(|namespace| !valid_segment(namespace)) {
        return Err(TranslationError::InvalidKey);
    }
    Ok(namespace)
}

fn set_line(lines: &mut Lines, key: &str, value: Line) {
    if let Some((first, rest)) = key.split_once('.') {
        let child = lines
            .entry(first.to_owned())
            .or_insert_with(|| Line::Group(Lines::new()));
        set_child(child, rest, value);
    } else {
        lines.insert(key.to_owned(), value);
    }
}

fn set_child(current: &mut Line, key: &str, value: Line) {
    let (first, rest) = key
        .split_once('.')
        .map_or((key, None), |(first, rest)| (first, Some(rest)));
    if let Line::List(items) = current
        && let Ok(index) = first.parse::<usize>()
        && index.to_string() == first
        && index <= items.len()
    {
        if index == items.len() {
            items.push(Line::Group(Lines::new()));
        }
        match rest {
            Some(rest) => set_child(&mut items[index], rest, value),
            None => items[index] = value,
        }
        return;
    }
    if !matches!(current, Line::Group(_)) {
        let previous = std::mem::replace(current, Line::Group(Lines::new()));
        if let Line::List(items) = previous {
            *current = Line::Group(indexed(items));
        }
    }
    if let Line::Group(lines) = current {
        set_line(lines, key, value);
    }
}

fn find<'a>(lines: &'a Lines, key: &str) -> Option<&'a Line> {
    if let Some(line) = lines.get(key) {
        return Some(line);
    }
    let (first, rest) = key.split_once('.')?;
    find_child(lines.get(first)?, rest)
}

fn find_child<'a>(line: &'a Line, key: &str) -> Option<&'a Line> {
    match line {
        Line::Group(lines) => find(lines, key),
        Line::Text(_) => None,
        Line::List(lines) => {
            let (index, rest) = key
                .split_once('.')
                .map_or((key, None), |(index, rest)| (index, Some(rest)));
            let child = lines.get(index.parse::<usize>().ok()?)?;
            match rest {
                Some(rest) => find_child(child, rest),
                None => Some(child),
            }
        }
    }
}

fn map_lines(
    lines: Lines,
    render: &impl Fn(String) -> TranslationResult<String>,
) -> TranslationResult<Lines> {
    lines
        .into_iter()
        .map(|(key, line)| Ok((key, map_line(line, render)?)))
        .collect()
}

fn map_line(
    line: Line,
    render: &impl Fn(String) -> TranslationResult<String>,
) -> TranslationResult<Line> {
    match line {
        Line::Text(text) => render(text).map(Line::Text),
        Line::Group(group) => map_lines(group, render).map(Line::Group),
        Line::List(list) => list
            .into_iter()
            .map(|line| map_line(line, render))
            .collect::<TranslationResult<_>>()
            .map(Line::List),
    }
}
