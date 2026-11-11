#![allow(non_snake_case, reason = "Matches Laravel's translator API")]

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::{Arc, Mutex},
};

use crate::contracts::translation::Translator as Contract;

use super::{
    FileLoader, Line, Lines, MessageSelector, PluralCount, TranslationError, TranslationResult,
    file_loader::valid_segment,
};

type Replacements = BTreeMap<String, String>;

#[derive(Eq, Hash, PartialEq)]
struct CatalogKey {
    locale: String,
    group: String,
    namespace: Option<String>,
}

tokio::task_local! {
    static ACTIVE_LOCALES: HashMap<usize, String>;
}

/// Resolves translations using one application's catalog and locale defaults.
pub struct Translator {
    loader: FileLoader,
    locale: String,
    fallback: String,
    loaded: Mutex<HashMap<CatalogKey, Arc<Lines>>>,
    identity: Arc<()>,
}

impl Translator {
    pub fn new(loader: FileLoader, locale: String, fallback: String) -> TranslationResult<Self> {
        if !valid_segment(&locale) || !valid_segment(&fallback) {
            return Err(TranslationError::InvalidLocale);
        }
        Ok(Self {
            loader,
            locale,
            fallback,
            loaded: Mutex::default(),
            identity: Arc::new(()),
        })
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
        if !valid_segment(locale) {
            return Err(TranslationError::InvalidLocale);
        }
        let mut locales = ACTIVE_LOCALES.try_with(Clone::clone).unwrap_or_default();
        locales.insert(self.identity(), locale.to_owned());
        Ok(ACTIVE_LOCALES.scope(locales, operation).await)
    }

    fn identity(&self) -> usize {
        Arc::as_ptr(&self.identity) as usize
    }

    pub fn string(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let locale = self.resolve_locale(locale)?;
        let text = match self.lookup(key, &locale, true)? {
            Some((Line::Text(text), _)) => text,
            Some((Line::Group(_) | Line::List(_), _)) => {
                return Err(TranslationError::UnexpectedType);
            }
            None => key.to_owned(),
        };
        Ok(replace(&text, replacements))
    }

    pub fn array(
        &self,
        key: &str,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<Lines> {
        let locale = self.resolve_locale(locale)?;
        let mut lines = match self.lookup(key, &locale, true)? {
            Some((Line::Group(lines), _)) => lines,
            Some((Line::List(lines), _)) => lines
                .into_iter()
                .enumerate()
                .map(|(index, line)| (index.to_string(), line))
                .collect(),
            _ => return Err(TranslationError::UnexpectedType),
        };
        replace_lines(&mut lines, replacements);
        Ok(lines)
    }

    pub fn choose(
        &self,
        key: &str,
        count: impl Into<PluralCount>,
        replacements: &Replacements,
        locale: Option<&str>,
    ) -> TranslationResult<String> {
        let locale = self.resolve_locale(locale)?;
        let count = count.into();
        let found = match self.lookup(key, &locale, false)? {
            Some(line) => Some(line),
            None if locale != self.fallback => self.lookup(key, &self.fallback, false)?,
            None => None,
        };
        let (line, selected) = match found {
            Some((Line::Text(line), selected)) => (line, selected),
            Some((Line::Group(_) | Line::List(_), _)) => {
                return Err(TranslationError::UnexpectedType);
            }
            None => (key.to_owned(), self.fallback.clone()),
        };
        let chosen = MessageSelector.choose(&line, count, &selected)?;
        let mut replacements = replacements.clone();
        replacements
            .entry("count".into())
            .or_insert_with(|| count.to_string());
        Ok(replace(chosen, &replacements))
    }

    fn resolve_locale(&self, locale: Option<&str>) -> TranslationResult<String> {
        let locale = locale.map(str::to_owned).unwrap_or_else(|| self.locale());
        if !valid_segment(&locale) {
            return Err(TranslationError::InvalidLocale);
        }
        Ok(locale)
    }

    fn lookup(
        &self,
        key: &str,
        locale: &str,
        fallback: bool,
    ) -> TranslationResult<Option<(Line, String)>> {
        if let Some(line) = self.catalog(locale, "*", Some("*"))?.get(key).cloned() {
            let line = match line {
                Line::Text(text) if text.is_empty() || text == "0" => Line::Text(key.to_owned()),
                line => line,
            };
            return Ok(Some((line, locale.to_owned())));
        }
        let (namespace, name) = key
            .split_once("::")
            .map_or((None, key), |(namespace, name)| (Some(namespace), name));
        let (group, item) = name
            .split_once('.')
            .map_or((name, None), |(group, item)| (group, Some(item)));
        if !group.split('/').all(valid_segment)
            || namespace.is_some_and(|namespace| !valid_segment(namespace))
        {
            return Ok(None);
        }
        if let Some(line) = self.group_line(locale, group, namespace, item)? {
            return Ok(Some((line, locale.to_owned())));
        }
        if fallback && locale != self.fallback {
            return Ok(self
                .group_line(&self.fallback, group, namespace, item)?
                .map(|line| (line, self.fallback.clone())));
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
            None if !lines.is_empty() => Some(Line::Group((*lines).clone())),
            None => None,
        })
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
        if let Some(lines) = self.loaded.lock().unwrap().get(&key).cloned() {
            return Ok(lines);
        }
        let lines = Arc::new(self.loader.load_lines(locale, group, namespace)?);
        Ok(self
            .loaded
            .lock()
            .unwrap()
            .entry(key)
            .or_insert(lines)
            .clone())
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
        if valid_segment(locale) {
            self.locale = locale.to_owned();
        }
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

fn replace_lines(lines: &mut Lines, replacements: &Replacements) {
    for line in lines.values_mut() {
        replace_line(line, replacements);
    }
}

fn replace_line(line: &mut Line, replacements: &Replacements) {
    match line {
        Line::Text(text) => *text = replace(text, replacements),
        Line::Group(group) => replace_lines(group, replacements),
        Line::List(list) => list
            .iter_mut()
            .for_each(|line| replace_line(line, replacements)),
    }
}

fn replace(text: &str, replacements: &Replacements) -> String {
    if replacements.is_empty() {
        return text.to_owned();
    }
    let mut tokens = BTreeMap::new();
    for (key, value) in replacements {
        if !key.is_empty() {
            tokens.insert(format!(":{}", key.to_uppercase()), value.to_uppercase());
            tokens.insert(format!(":{}", title(key)), title(value));
            tokens.insert(format!(":{key}"), value.clone());
        }
    }
    let mut tokens: Vec<_> = tokens.into_iter().collect();
    tokens.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(':') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some((token, value)) = tokens.iter().find(|(token, _)| rest.starts_with(token)) {
            output.push_str(value);
            rest = &rest[token.len()..];
        } else {
            output.push(':');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    output
}

fn title(value: &str) -> String {
    super::choice_case::titlecase_first(value)
}
