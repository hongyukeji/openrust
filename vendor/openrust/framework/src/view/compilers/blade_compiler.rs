#![allow(non_snake_case)]

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};

use super::{Compiler, template::Template};
use crate::view::{
    ViewData, ViewError, ViewResult, ViewValue,
    rendering::{RenderContext, render_error},
};

const PREFIX: &str = "blade-v1-";
const SUFFIX: &str = ".json";
const COMPILER_VERSION: u32 = 23;

type Directive = Arc<dyn Fn(&[ViewValue]) -> ViewResult<DirectiveOutput> + Send + Sync>;
type Condition = Arc<dyn Fn(&[ViewValue]) -> ViewResult<bool> + Send + Sync>;

#[derive(Clone, Default)]
pub(in crate::view) struct BladeExtensions {
    pub(in crate::view) directives: BTreeMap<String, Directive>,
    pub(in crate::view) conditions: BTreeMap<String, Condition>,
    pub(in crate::view) preserve_html_entities: bool,
}

/// Output produced by a registered Rust Blade directive.
pub enum DirectiveOutput {
    /// A text value escaped by the renderer.
    Text(String),
    /// Trusted HTML supplied by the application's directive.
    Html(String),
}

/// Compiles Blade source into a cacheable tree of template instructions.
pub struct BladeCompiler {
    directory: PathBuf,
    extensions: RwLock<Arc<BladeExtensions>>,
}

#[derive(Serialize, Deserialize)]
struct Compiled {
    #[serde(default)]
    compiler_version: u32,
    source: String,
    directives: Vec<String>,
    conditions: Vec<String>,
    template: Template,
}

impl Compiled {
    fn syntax_matches(&self, extensions: &BladeExtensions) -> bool {
        extensions.directives.keys().eq(self.directives.iter())
            && extensions.conditions.keys().eq(self.conditions.iter())
    }
}

impl BladeCompiler {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            extensions: RwLock::default(),
        }
    }

    /// Preserves existing HTML entities in escaped Blade echoes.
    pub fn withoutDoubleEncoding(&self) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).preserve_html_entities = true;
    }

    /// Restores double encoding for escaped Blade echoes.
    pub fn withDoubleEncoding(&self) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).preserve_html_entities = false;
    }

    /// Registers a Rust callback for a custom Blade directive name.
    pub fn directive(
        &self,
        name: &str,
        callback: impl Fn(&[ViewValue]) -> ViewResult<DirectiveOutput> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        if !super::template::identifier(name) {
            return Err(invalid_registration("Invalid Blade directive name."));
        }
        let mut extensions = self.extensions.write().unwrap();
        if extensions.conditions.contains_key(name) {
            return Err(invalid_registration(
                "A Blade condition already has this name.",
            ));
        }
        Arc::make_mut(&mut extensions)
            .directives
            .insert(name.to_owned(), Arc::new(callback));
        Ok(())
    }

    /// Registers an application-owned Rust predicate for Blade conditional blocks.
    pub fn r#if(
        &self,
        name: &str,
        callback: impl Fn(&[ViewValue]) -> ViewResult<bool> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        if !super::template::identifier(name) {
            return Err(invalid_registration("Invalid Blade condition name."));
        }
        let mut extensions = self.extensions.write().unwrap();
        if extensions.directives.contains_key(name) {
            return Err(invalid_registration(
                "A Blade directive already has this name.",
            ));
        }
        Arc::make_mut(&mut extensions)
            .conditions
            .insert(name.to_owned(), Arc::new(callback));
        Ok(())
    }

    /// Evaluates an application-owned Blade predicate.
    pub fn check(&self, name: &str, values: &[ViewValue]) -> ViewResult<bool> {
        let callback = self
            .extensions
            .read()
            .unwrap()
            .conditions
            .get(name)
            .cloned()
            .ok_or_else(|| render_error("Blade condition registration was not found."))?;
        callback(values)
    }

    pub(in crate::view) fn render_with(
        &self,
        path: &Path,
        source: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let extensions = self.extensions.read().unwrap().clone();
        let template = self.load(path, source, &extensions)?;
        context.extensions = extensions;
        template.render(data, context)
    }

    pub fn render(&self, path: &Path, source: &str, data: &ViewData) -> ViewResult<String> {
        self.render_with(path, source, data, &mut RenderContext::new(None))
    }

    pub(in crate::view) fn render_inline(
        &self,
        component: &str,
        source: &str,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let identity = format!("inline-{component}-{}", digest(source.as_bytes()));
        let extensions = self.extensions.read().unwrap().clone();
        let template = Template::compile(source, &identity, &extensions)?;
        context.extensions = extensions;
        template.render(data, context)
    }

    pub fn clear(&self) -> ViewResult<()> {
        let directory = match Dir::open_ambient_dir(&self.directory, ambient_authority()) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(compile_error(error)),
        };
        for entry in directory.entries().map_err(compile_error)? {
            let entry = entry.map_err(compile_error)?;
            let file = entry.file_name();
            let artifact = file.to_str().and_then(|name| {
                let digest = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
                (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    .then_some(name)
            });
            if let Some(name) = artifact
                && entry.file_type().map_err(compile_error)?.is_file()
            {
                match directory.remove_file(name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(compile_error(error)),
                }
            }
        }
        Ok(())
    }

    fn load(
        &self,
        path: &Path,
        source: &str,
        extensions: &BladeExtensions,
    ) -> ViewResult<Template> {
        let name = artifact_name(path);
        let fingerprint = digest(source.as_bytes());
        if let Some(compiled) = self.read_artifact(&name)?
            && compiled.compiler_version == COMPILER_VERSION
            && compiled.source == fingerprint
            && compiled.syntax_matches(extensions)
        {
            return Ok(compiled.template);
        }
        let compiled = Compiled {
            compiler_version: COMPILER_VERSION,
            source: fingerprint,
            template: Template::compile(source, &name, extensions)?,
            directives: extensions.directives.keys().cloned().collect(),
            conditions: extensions.conditions.keys().cloned().collect(),
        };
        self.publish(&name, &compiled)?;
        Ok(compiled.template)
    }

    fn read_artifact(&self, name: &str) -> ViewResult<Option<Compiled>> {
        let bytes = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .and_then(|directory| directory.read(name));
        match bytes {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(compile_error(error)),
        }
    }

    fn publish(&self, name: &str, compiled: &Compiled) -> ViewResult<()> {
        fs::create_dir_all(&self.directory).map_err(compile_error)?;
        let directory =
            Dir::open_ambient_dir(&self.directory, ambient_authority()).map_err(compile_error)?;
        let temporary = format!(".blade-{:032x}.tmp", fastrand::u128(..));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = directory
            .open_with(&temporary, &options)
            .map_err(compile_error)?;
        let result = (|| {
            serde_json::to_writer(&mut file, compiled).map_err(compile_error)?;
            file.flush().map_err(compile_error)?;
            drop(file);
            directory
                .rename(&temporary, &directory, name)
                .map_err(compile_error)
        })();
        if result.is_err() {
            let _ = directory.remove_file(&temporary);
        }
        result
    }
}

impl Compiler for BladeCompiler {
    fn getCompiledPath(&self, path: &Path) -> ViewResult<PathBuf> {
        Ok(self.directory.join(artifact_name(path)))
    }

    fn isExpired(&self, path: &Path) -> ViewResult<bool> {
        let source = fs::read(path).map_err(ViewError::ReadFailed)?;
        Ok(self
            .read_artifact(&artifact_name(path))?
            .is_none_or(|compiled| {
                compiled.compiler_version != COMPILER_VERSION
                    || compiled.source != digest(&source)
                    || !compiled.syntax_matches(&self.extensions.read().unwrap())
            }))
    }

    fn compile(&self, path: &Path) -> ViewResult<()> {
        let source = fs::read_to_string(path).map_err(ViewError::ReadFailed)?;
        let extensions = self.extensions.read().unwrap().clone();
        self.load(path, &source, &extensions)?;
        Ok(())
    }
}

fn artifact_name(path: &Path) -> String {
    format!(
        "{PREFIX}{}{SUFFIX}",
        digest(path.as_os_str().as_encoded_bytes())
    )
}

fn digest(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(64);
    for byte in aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes).as_ref() {
        write!(result, "{byte:02x}").unwrap();
    }
    result
}

fn compile_error(error: impl std::error::Error + Send + Sync + 'static) -> ViewError {
    ViewError::CompileFailed(Box::new(error))
}

fn invalid_registration(message: &'static str) -> ViewError {
    compile_error(io::Error::new(io::ErrorKind::InvalidInput, message))
}
