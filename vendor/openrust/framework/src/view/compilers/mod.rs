//! Compilation and expiration contracts for template engines.

mod blade_compiler;
mod template;

use std::path::{Path, PathBuf};

use super::ViewResult;

pub(in crate::view) use blade_compiler::BladeExtensions;
pub use blade_compiler::{BladeCompiler, DirectiveOutput};

/// Compiles a template and determines when its artifact is stale.
pub trait Compiler: Send + Sync {
    fn get_compiled_path(&self, path: &Path) -> ViewResult<PathBuf>;
    fn is_expired(&self, path: &Path) -> ViewResult<bool>;
    fn compile(&self, path: &Path) -> ViewResult<()>;
}
