#![allow(non_snake_case)]

use std::path::{Path, PathBuf};

use super::ViewResult;

/// Resolves names and namespaces to files under configured view roots.
pub trait ViewFinder: Send + Sync {
    fn find(&self, view: &str) -> ViewResult<PathBuf>;
    fn add_location(&mut self, location: &Path) -> ViewResult<()>;
    fn add_namespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn prepend_namespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn replace_namespace(&mut self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn add_extension(&mut self, extension: &str) -> ViewResult<()>;
    fn flush(&self);
}
