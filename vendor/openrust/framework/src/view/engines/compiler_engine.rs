#![allow(non_snake_case)]

use std::path::Path;

use crate::view::{ViewData, ViewResult, compilers::BladeCompiler};

/// Renders templates through the Blade compiler and its compiled artifacts.
pub struct CompilerEngine(BladeCompiler);

impl CompilerEngine {
    pub fn new(compiler: BladeCompiler) -> Self {
        Self(compiler)
    }

    pub fn render(&self, path: &Path, source: &str, data: &ViewData) -> ViewResult<String> {
        self.0.render(path, source, data)
    }

    pub fn getCompiler(&self) -> &BladeCompiler {
        &self.0
    }
}
