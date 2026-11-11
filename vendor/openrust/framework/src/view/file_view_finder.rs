#![allow(non_snake_case)]

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::RwLock,
};

use super::{ViewError, ViewFinder, ViewResult};

/// Resolves file names through ordered application roots and namespace hints.
pub struct FileViewFinder {
    paths: Vec<PathBuf>,
    hints: HashMap<String, Vec<PathBuf>>,
    extensions: Vec<String>,
    resolved: RwLock<HashMap<String, ViewLocation>>,
}

#[derive(Clone)]
struct ViewLocation {
    root: PathBuf,
    candidate: PathBuf,
}

pub(super) struct ViewSource {
    pub(super) path: PathBuf,
    pub(super) root: PathBuf,
    pub(super) resolved: PathBuf,
}

impl FileViewFinder {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: paths.into_iter().collect(),
            hints: HashMap::new(),
            extensions: vec![String::from("html")],
            resolved: RwLock::default(),
        }
    }

    fn findInPaths(&self, name: &Path, paths: &[PathBuf]) -> ViewResult<(ViewLocation, PathBuf)> {
        for directory in paths {
            let Some(root) = existing_path(directory)? else {
                continue;
            };
            for extension in &self.extensions {
                let candidate = root.join(name.with_extension(extension));
                if let Some(path) = confined_file(&root, &candidate)? {
                    return Ok((ViewLocation { root, candidate }, path));
                }
            }
        }
        Err(ViewError::NotFound)
    }

    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(super) fn source(&self, view: &str) -> ViewResult<ViewSource> {
        let (location, resolved) = self.resolve(view)?;
        Ok(ViewSource {
            path: location.candidate,
            root: location.root,
            resolved,
        })
    }

    fn resolve(&self, view: &str) -> ViewResult<(ViewLocation, PathBuf)> {
        let view = view.trim();
        let (name, paths) = match view.split_once("::") {
            Some((namespace, name)) if valid_segment(namespace) => (
                name,
                self.hints
                    .get(namespace)
                    .ok_or(ViewError::NotFound)?
                    .as_slice(),
            ),
            Some(_) => return Err(ViewError::InvalidName),
            None => (view, self.paths.as_slice()),
        };
        let name = relative_name(name)?;

        let cached = self.resolved.read().unwrap().get(view).cloned();
        if let Some(location) = cached
            && let Some(path) = confined_file(&location.root, &location.candidate)?
        {
            return Ok((location, path));
        }

        let (location, path) = self.findInPaths(&name, paths)?;
        self.resolved
            .write()
            .unwrap()
            .insert(view.to_owned(), location.clone());
        Ok((location, path))
    }
}

impl ViewFinder for FileViewFinder {
    fn find(&self, view: &str) -> ViewResult<PathBuf> {
        self.resolve(view).map(|(_, path)| path)
    }

    fn addLocation(&mut self, location: &Path) -> ViewResult<()> {
        self.paths.push(location.to_owned());
        self.flush();
        Ok(())
    }

    fn addNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()> {
        let hints = owned_hints(namespace, hints)?;
        self.hints
            .entry(namespace.to_owned())
            .or_default()
            .extend(hints);
        self.flush();
        Ok(())
    }

    fn prependNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()> {
        let mut hints = owned_hints(namespace, hints)?;
        let current = self.hints.entry(namespace.to_owned()).or_default();
        hints.append(current);
        *current = hints;
        self.flush();
        Ok(())
    }

    fn replaceNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()> {
        self.hints
            .insert(namespace.to_owned(), owned_hints(namespace, hints)?);
        self.flush();
        Ok(())
    }

    fn addExtension(&mut self, extension: &str) -> ViewResult<()> {
        if !extension.split('.').all(valid_segment) {
            return Err(ViewError::InvalidName);
        }
        self.extensions.retain(|existing| existing != extension);
        self.extensions.insert(0, extension.to_owned());
        self.flush();
        Ok(())
    }

    fn flush(&self) {
        self.resolved.write().unwrap().clear();
    }
}

fn owned_hints(namespace: &str, hints: &[&Path]) -> ViewResult<Vec<PathBuf>> {
    if !valid_segment(namespace) {
        return Err(ViewError::InvalidName);
    }
    Ok(hints.iter().map(|path| path.to_path_buf()).collect())
}

fn relative_name(name: &str) -> ViewResult<PathBuf> {
    let mut path = PathBuf::new();
    for segment in name.split(['.', '/']) {
        if !valid_segment(segment) {
            return Err(ViewError::InvalidName);
        }
        path.push(segment);
    }
    Ok(path)
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
}

fn confined_file(root: &Path, candidate: &Path) -> ViewResult<Option<PathBuf>> {
    let Some(path) = existing_path(candidate)? else {
        return Ok(None);
    };
    if !path.starts_with(root) {
        return Err(ViewError::OutsideRoot);
    }
    match path.metadata() {
        Ok(metadata) if metadata.is_file() => Ok(Some(path)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ViewError::ReadFailed(error)),
    }
}

fn existing_path(path: &Path) -> ViewResult<Option<PathBuf>> {
    match path.canonicalize() {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ViewError::ReadFailed(error)),
    }
}
