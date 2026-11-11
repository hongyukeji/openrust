#![allow(non_snake_case)]

use std::path::{Path, PathBuf};

use super::ViewResult;

/// Resolves names and namespaces to files under configured view roots.
pub trait ViewFinder: Send + Sync {
    fn find(&self, view: &str) -> ViewResult<PathBuf>;
    fn addLocation(&mut self, location: &Path) -> ViewResult<()>;
    fn addNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn prependNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn replaceNamespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn addExtension(&mut self, extension: &str) -> ViewResult<()>;
    fn flush(&self);
}
