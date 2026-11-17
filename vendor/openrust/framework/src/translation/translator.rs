#![allow(non_snake_case, reason = "Matches Laravel's translator API")]

use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::contracts::translation::{Loader, Translator as Contract};

use super::{
    FileLoader, Line, Lines, MessageSelector, PluralCount, Replacement, TranslationError,
    TranslationResource, TranslationResult,
    file_loader::{merge::indexed, valid_group, valid_segment},
    locale::canonical_locale,
    lookup::{LookupHooks, MissingTranslation},
    replacement::{Stringables, replace},
};

type Replacements = BTreeMap<String, String>;

#[derive(Clone, Eq, Hash, PartialEq)]
struct CatalogKey {
    locale: String,
    group: String,
    namespace: Option<String>,
}

#[derive(Default)]
struct Catalog {
    lines: Arc<Lines>,
    runtime: bool,
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
    static ACTIVE_LOCALES: HashMap<usize, String>;
}

/// Resolves translations using one application's catalog and locale defaults.
pub struct Translator {
    loader: Arc<FileLoader>,
    locale: String,
    fallback: String,
    loaded: Mutex<HashMap<CatalogKey, Catalog>>,
    identity: Arc<()>,
    hooks: LookupHooks,
    stringables: Stringables,
}

impl Translator {
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
            locale: canonical_locale(&locale)?,
            fallback: canonical_locale(&fallback)?,
            loaded: Mutex::default(),
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
        self.loader.add_namespace(namespace.to_owned(), path);
        loaded
            .retain(|key, catalog| catalog.runtime || key.namespace.as_deref() != Some(namespace));
        Ok(())
    }

    pub(crate) fn add_package_json_path(&self, path: PathBuf) {
        let mut loaded = self.loaded.lock().unwrap();
        self.loader.addJsonPath(path);
        loaded.retain(|key, catalog| {
            catalog.runtime || key.group != "*" || key.namespace.as_deref() != Some("*")
        });
    }

    /// Adds a language directory for subsequent group and literal JSON loads.
    pub fn addPath(&self, path: PathBuf) {
        self.loader.addPath(path);
    }

    /// Adds a directory for subsequent anonymous JSON catalog loads.
    pub fn addJsonPath(&self, path: PathBuf) {
        self.loader.addJsonPath(path);
    }

    /// Adds or replaces a package namespace for subsequent loads.
    pub fn addNamespace(&self, namespace: &str, path: PathBuf) -> TranslationResult<()> {
        if !valid_segment(namespace) {
            return Err(TranslationError::InvalidKey);
        }
        self.loader.add_namespace(namespace.to_owned(), path);
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
        self.catalog(&locale, group, namespace)?;
        Ok(())
    }

    /// Materializes exact groups from an asynchronous loader before publishing them together.
    pub async fn loadFrom(
        &self,
        loader: &dyn Loader,
        resources: &[TranslationResource<'_>],
    ) -> TranslationResult<()> {
        let mut seen = HashSet::with_capacity(resources.len());
        let mut keys = Vec::with_capacity(resources.len());
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
        let versions: Vec<_> = {
            let loaded = self.loaded.lock().unwrap();
            keys.iter()
                .map(|key| runtime_lines(&loaded, key).cloned())
                .collect()
        };
        let mut incoming = Vec::with_capacity(keys.len());
        for key in &keys {
            incoming.push(
                loader
                    .load(&key.locale, &key.group, key.namespace.as_deref())
                    .await?,
            );
        }
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
                    lines: Arc::new(lines),
                    runtime: true,
                },
            );
        }
        Ok(())
    }

    /// Adds dotted keys to the loaded groups of the given locale and namespace.
    pub fn addLines(
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
        let mut loaded = self.loaded.lock().unwrap();
        for (key, item, line) in entries {
            let catalog = loaded.entry(key).or_default();
            set_line(Arc::make_mut(&mut catalog.lines), &item, line);
            catalog.runtime = true;
        }
        Ok(())
    }

    pub fn locale(&self) -> String {
        ACTIVE_LOCALES
            .try_with(|locales| locales.get(&self.identity()).cloned())
            .ok()
            .flatten()
            .unwrap_or_else(|| self.locale.clone())
    }

    pub async fn withLocale<T>(
        &self,
        locale: &str,
        operation: impl Future<Output = T>,
    ) -> TranslationResult<T> {
        let locale = canonical_locale(locale)?;
        let mut locales = ACTIVE_LOCALES.try_with(Clone::clone).unwrap_or_default();
        locales.insert(self.identity(), locale);
        Ok(ACTIVE_LOCALES.scope(locales, operation).await)
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

    pub fn hasForLocale(&self, key: &str, locale: Option<&str>) -> TranslationResult<bool> {
        self.has(key, locale, false)
    }

    /// Installs an application-level handler for missing keys.
    pub fn handleMissingKeysUsing(
        &self,
        handler: impl Fn(MissingTranslation<'_>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.hooks.set_missing(Some(Arc::new(handler)));
    }

    pub fn clearMissingKeyHandler(&self) {
        self.hooks.set_missing(None);
    }

    /// Replaces the search order for grouped translations with fallback enabled.
    pub fn determineLocalesUsing(
        &self,
        resolver: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) {
        self.hooks.set_locales(Some(Arc::new(resolver)));
    }

    pub fn clearLocaleResolver(&self) {
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
    pub fn stringWith(
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
    pub fn arrayWith(
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
    pub fn choiceWith(
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

    /// Registers a formatter for one concrete Rust type on this translator.
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
            None => self.lookup(key, &self.fallback, true)?,
        };
        let (line, selected) = match found {
            Some(ResolvedLine {
                line: Line::Text(line),
                locale,
                ..
            }) => (line, locale),
            Some(_) => return Err(TranslationError::UnexpectedType),
            None => (
                self.missing(key, &Replacements::new(), &self.fallback),
                self.fallback.clone(),
            ),
        };
        Ok(MessageSelector.choose(&line, count, &selected)?.to_owned())
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
        if let Some(line) = self.catalog(locale, "*", Some("*"))?.get(key).cloned() {
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
                .group_line(locale, group, namespace, item)?
                .map(|line| ResolvedLine {
                    line,
                    locale: locale.to_owned(),
                    literal: false,
                }));
        }
        for locale in self.hooks.locales(locale, &self.fallback)? {
            if let Some(line) = self.group_line(&locale, group, namespace, item)? {
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
        locale: &str,
        group: &str,
        namespace: Option<&str>,
        item: Option<&str>,
    ) -> TranslationResult<Option<Line>> {
        let lines = self.catalog(locale, group, namespace)?;
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
        if let Some(lines) = loaded.get(&key) {
            return Ok(Arc::clone(&lines.lines));
        }
        let lines = Arc::new(self.loader.load_lines(locale, group, namespace)?);
        loaded.insert(
            key,
            Catalog {
                lines: Arc::clone(&lines),
                runtime: false,
            },
        );
        Ok(lines)
    }
}

impl Contract for Translator {
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

    fn get_locale(&self) -> &str {
        &self.locale
    }

    fn set_locale(&mut self, locale: &str) {
        if let Ok(locale) = canonical_locale(locale) {
            self.locale = locale;
        }
    }
}

fn runtime_lines<'a>(
    loaded: &'a HashMap<CatalogKey, Catalog>,
    key: &CatalogKey,
) -> Option<&'a Arc<Lines>> {
    loaded
        .get(key)
        .filter(|catalog| catalog.runtime)
        .map(|catalog| &catalog.lines)
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
