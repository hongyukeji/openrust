#![allow(non_snake_case, reason = "Matches Laravel's translation loader API")]

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use cap_std::{ambient_authority, fs::Dir};

use crate::contracts::translation::Loader;

use super::{Line, Lines, TranslationError, TranslationFuture, TranslationResult};

/// Loads application and package translations from configured directories.
#[derive(Clone)]
pub struct FileLoader {
    paths: Vec<PathBuf>,
    namespaces: HashMap<String, PathBuf>,
}

impl FileLoader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            paths: vec![path],
            namespaces: HashMap::new(),
        }
    }

    pub fn addPath(&mut self, path: PathBuf) {
        self.paths.push(path);
    }

    pub fn addNamespace(&mut self, namespace: impl Into<String>, path: PathBuf) {
        self.namespaces.insert(namespace.into(), path);
    }

    pub(super) fn load_lines(
        &self,
        locale: &str,
        group: &str,
        namespace: Option<&str>,
    ) -> TranslationResult<Lines> {
        if !valid_segment(locale) {
            return Err(TranslationError::InvalidLocale);
        }
        let mut lines = Lines::new();
        if group == "*" && namespace == Some("*") {
            for root in &self.paths {
                merge(
                    &mut lines,
                    read(root, Path::new(&format!("{locale}.json")))?,
                );
            }
            return Ok(lines);
        }
        if !group.split('/').all(valid_segment)
            || namespace.is_some_and(|namespace| !valid_segment(namespace))
        {
            return Err(TranslationError::InvalidKey);
        }
        let relative = Path::new(locale).join(format!("{group}.json"));
        if let Some(namespace) = namespace {
            let Some(package) = self.namespaces.get(namespace) else {
                return Ok(lines);
            };
            lines = read(package, &relative)?;
            for root in &self.paths {
                merge(
                    &mut lines,
                    read(root, &Path::new("vendor").join(namespace).join(&relative))?,
                );
            }
        } else {
            for root in &self.paths {
                merge(&mut lines, read(root, &relative)?);
            }
        }
        Ok(lines)
    }
}

impl Loader for FileLoader {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> TranslationFuture<'a, TranslationResult<Lines>> {
        Box::pin(async move {
            let loader = self.clone();
            let locale = locale.to_owned();
            let group = group.to_owned();
            let namespace = namespace.map(str::to_owned);
            tokio::task::spawn_blocking(move || {
                loader.load_lines(&locale, &group, namespace.as_deref())
            })
            .await
            .map_err(|source| TranslationError::Source(Box::new(source)))?
        })
    }
}

pub(super) fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn read(root: &Path, relative: &Path) -> TranslationResult<Lines> {
    let bytes = match Dir::open_ambient_dir(root, ambient_authority())
        .and_then(|root| root.read(relative))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Lines::new()),
        Err(error) => return Err(TranslationError::Source(Box::new(error))),
    };
    serde_json::from_slice(&bytes).map_err(|source| TranslationError::Source(Box::new(source)))
}

fn merge(lines: &mut Lines, overlay: Lines) {
    for (key, value) in overlay {
        match (lines.get_mut(&key), value) {
            (Some(Line::Group(base)), Line::Group(overlay)) => merge(base, overlay),
            (_, value) => {
                lines.insert(key, value);
            }
        }
    }
}
