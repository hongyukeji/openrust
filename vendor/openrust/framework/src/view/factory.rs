#![allow(non_snake_case)]

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::{
    FileViewFinder, View, ViewData, ViewError, ViewFinder, ViewResult, compilers::BladeCompiler,
    engines::EngineResolver,
};

/// Creates named views from the application's finder and engines.
pub struct Factory {
    finder: FileViewFinder,
    engines: EngineResolver,
}

impl Factory {
    pub fn new(mut finder: FileViewFinder, compiled: PathBuf) -> Self {
        finder
            .addExtension("blade.html")
            .expect("valid Blade extension");
        Self {
            finder,
            engines: EngineResolver::new(compiled),
        }
    }

    pub fn make(&self, name: impl Into<Cow<'static, str>>, data: ViewData) -> ViewResult<View> {
        let name = name.into();
        self.finder.find(&name)?;
        Ok(View { name, data })
    }

    pub fn first(&self, names: &[&str], data: ViewData) -> ViewResult<View> {
        for name in names {
            match self.finder.find(name) {
                Ok(_) => {
                    return Ok(View {
                        name: (*name).to_owned().into(),
                        data,
                    });
                }
                Err(ViewError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Err(ViewError::NotFound)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.finder.find(name).is_ok()
    }

    pub fn render(&self, name: &str, data: &ViewData) -> ViewResult<String> {
        let source = self.finder.source(name)?;
        let engine = if is_blade_path(&source.path) {
            "blade"
        } else {
            "file"
        };
        self.engines.resolve(engine)?.render(source, data)
    }

    pub fn getFinder(&self) -> &FileViewFinder {
        &self.finder
    }

    pub fn getEngineResolver(&self) -> &EngineResolver {
        &self.engines
    }

    pub(crate) fn compiler(&self) -> &BladeCompiler {
        self.engines.blade().getCompiler()
    }

    pub(crate) fn paths(&self) -> &[PathBuf] {
        self.finder.paths()
    }

    pub(crate) fn render_error(&self, name: &str, status: u16, message: &str) -> Option<String> {
        let data = ViewData::from_iter([
            ("status".to_owned(), status.into()),
            ("message".to_owned(), message.into()),
        ]);
        self.render(name, &data).ok()
    }
}

pub(super) fn is_blade_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".blade.html"))
}
