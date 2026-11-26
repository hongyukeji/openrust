use std::path::{Path, PathBuf};

use super::{CompilerEngine, FileEngine};
use crate::view::{
    ViewData, ViewError, ViewResult, compilers::BladeCompiler, file_view_finder::ViewSource,
    rendering::RenderContext,
};

/// Resolves the file and Blade engines used by the view factory.
pub struct EngineResolver {
    file: FileEngine,
    blade: CompilerEngine,
}

impl EngineResolver {
    pub fn new(compiled: PathBuf) -> Self {
        Self {
            file: FileEngine,
            blade: CompilerEngine::new(BladeCompiler::new(compiled)),
        }
    }

    pub fn resolve(&self, name: &str) -> ViewResult<Engine<'_>> {
        match name {
            "file" => Ok(Engine::File(&self.file)),
            "blade" => Ok(Engine::Blade(&self.blade)),
            _ => Err(ViewError::UnknownEngine),
        }
    }

    pub(in crate::view) fn blade(&self) -> &CompilerEngine {
        &self.blade
    }
}

pub enum Engine<'a> {
    File(&'a FileEngine),
    Blade(&'a CompilerEngine),
}

impl Engine<'_> {
    pub(in crate::view) fn render(
        &self,
        source: ViewSource,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        match self {
            Self::File(engine) => engine.get(&source.root, &source.resolved),
            Self::Blade(engine) => {
                let content = FileEngine.get(&source.root, &source.resolved)?;
                engine.render_with(&source.path, &content, data, context)
            }
        }
    }
}

impl crate::contracts::view::Engine for Engine<'_> {
    fn get(&self, root: &Path, path: &Path, data: &ViewData) -> ViewResult<String> {
        let engine: &dyn crate::contracts::view::Engine = match self {
            Self::File(engine) => *engine,
            Self::Blade(engine) => *engine,
        };
        engine.get(root, path, data)
    }
}
