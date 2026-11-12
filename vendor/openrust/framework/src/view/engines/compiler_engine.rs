#![allow(non_snake_case)]

use std::path::Path;

use super::FileEngine;
use crate::view::{ViewData, ViewResult, compilers::BladeCompiler, rendering::RenderContext};

/// Renders templates through the Blade compiler and its compiled artifacts.
pub struct CompilerEngine(BladeCompiler);

impl CompilerEngine {
    pub fn new(compiler: BladeCompiler) -> Self {
        Self(compiler)
    }

    pub(in crate::view) fn render_with(
        &self,
        path: &Path,
        source: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        self.0.render_with(path, source, data, context)
    }

    pub fn render(&self, path: &Path, source: &str, data: &ViewData) -> ViewResult<String> {
        self.0.render(path, source, data)
    }

    pub fn getCompiler(&self) -> &BladeCompiler {
        &self.0
    }
}

impl crate::contracts::view::Engine for CompilerEngine {
    fn get(&self, root: &Path, path: &Path, data: &ViewData) -> ViewResult<String> {
        let source = FileEngine.get(root, path)?;
        self.render(path, &source, data)
    }
}
