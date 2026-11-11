//! Compilation and expiration contracts for template engines.

#![allow(non_snake_case)]

mod blade_compiler;
mod template;

use std::path::{Path, PathBuf};

use super::ViewResult;

pub use blade_compiler::BladeCompiler;

/// Compiles a template and determines when its artifact is stale.
pub trait Compiler: Send + Sync {
    fn getCompiledPath(&self, path: &Path) -> ViewResult<PathBuf>;
    fn isExpired(&self, path: &Path) -> ViewResult<bool>;
    fn compile(&self, path: &Path) -> ViewResult<()>;
}
