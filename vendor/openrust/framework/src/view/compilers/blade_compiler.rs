use std::{
    any::{Any, TypeId},
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
const COMPILER_VERSION: u32 = 45;

pub type DirectiveCallback = Arc<dyn Fn(&[ViewValue]) -> ViewResult<DirectiveOutput> + Send + Sync>;
type Directive = DirectiveCallback;
type Condition = Arc<dyn Fn(&[ViewValue]) -> ViewResult<bool> + Send + Sync>;
type Stringable = Arc<dyn Fn(&dyn Any) -> ViewResult<String> + Send + Sync>;
type IterableEcho = Arc<dyn Fn(&ViewValue) -> ViewResult<String> + Send + Sync>;
type AttributeFilter = Arc<dyn Fn(&str, &ViewValue) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub(in crate::view) struct BladeExtensions {
    pub(in crate::view) native_views: &'static [crate::view::NativeTemplate],
    pub(in crate::view) directives: BTreeMap<String, Directive>,
    pub(in crate::view) conditions: BTreeMap<String, Condition>,
    pub(in crate::view) preserve_html_entities: bool,
    stringables: BTreeMap<TypeId, Stringable>,
    pub(in crate::view) iterable_echo: Option<IterableEcho>,
    pub(in crate::view) attribute_filters: BTreeMap<String, AttributeFilter>,
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
    path: RwLock<Option<PathBuf>>,
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

/// 可重复渲染的原生编译结果；不生成或执行 PHP 源码。
#[derive(Clone)]
pub struct CompiledTemplate {
    template: Arc<Template>,
    extensions: Arc<BladeExtensions>,
}
impl CompiledTemplate {
    pub fn render(&self, data: &ViewData) -> ViewResult<String> {
        self.render_with_factory(data, None)
    }
    pub fn render_with_factory(
        &self,
        data: &ViewData,
        factory: Option<&crate::view::Factory>,
    ) -> ViewResult<String> {
        let mut context = RenderContext::new(factory);
        context.extensions = self.extensions.clone();
        self.template
            .render(data, &mut context)
            .map(trim_php_output)
    }
}

impl Compiled {
    fn syntax_matches(&self, extensions: &BladeExtensions) -> bool {
        extensions.directives.keys().eq(self.directives.iter())
            && extensions.conditions.keys().eq(self.conditions.iter())
    }
}

impl BladeCompiler {
    pub fn get_path(&self) -> Option<PathBuf> {
        self.path.read().unwrap().clone()
    }
    pub fn set_path(&self, path: impl Into<PathBuf>) {
        *self.path.write().unwrap() = Some(path.into());
    }
    /// 按源表达式的字节规则去除首尾；保留未闭合输入的可观察结果。
    pub fn strip_parentheses(expression: impl AsRef<[u8]>) -> Vec<u8> {
        let expression = expression.as_ref();
        if expression.first() == Some(&b'(') {
            expression[1..expression.len().saturating_sub(1).max(1)].to_vec()
        } else {
            expression.to_vec()
        }
    }
    pub fn get_custom_directives(&self) -> BTreeMap<String, DirectiveCallback> {
        self.extensions.read().unwrap().directives.clone()
    }
    pub fn compile_string(&self, source: &str) -> ViewResult<CompiledTemplate> {
        let extensions = self.extensions.read().unwrap().clone();
        let name = format!("inline-{}", digest(source.as_bytes()));
        self.validate_native(Path::new(&name), source, &extensions)?;
        Ok(CompiledTemplate {
            template: Arc::new(Template::compile(source, &name, &extensions)?),
            extensions,
        })
    }

    /// 装配构建期生成的不可变函数表；不启动编译器或执行模板业务。
    #[doc(hidden)]
    pub fn native_views(&self, templates: &'static [crate::view::NativeTemplate]) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).native_views = templates;
    }

    fn validate_native(
        &self,
        path: &Path,
        source: &str,
        extensions: &BladeExtensions,
    ) -> ViewResult<()> {
        let blocks = openrust_blade_compiler::blocks(source).map_err(|error| {
            compile_error(io::Error::other(format!("{}: {error}", path.display())))
        })?;
        if blocks.is_empty() {
            return Ok(());
        }
        let sources: Vec<_> = blocks.iter().map(|b| b.body.to_owned()).collect();
        if !extensions
            .native_views
            .iter()
            .any(|template| template.matches(&sources))
        {
            return Err(compile_error(io::Error::other(format!(
                "{}:{} 原生 Rust 代码块未构建或与当前模板不一致，请重新构建应用",
                path.display(),
                blocks[0].line
            ))));
        }
        Ok(())
    }
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            path: RwLock::default(),
            extensions: RwLock::default(),
        }
    }

    /// Preserves existing HTML entities in escaped Blade echoes.
    pub fn without_double_encoding(&self) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).preserve_html_entities = true;
    }

    /// Restores double encoding for escaped Blade echoes.
    pub fn with_double_encoding(&self) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).preserve_html_entities = false;
    }

    /// Registers the application's formatter for one Rust type.
    pub fn stringable<T: 'static>(
        &self,
        format: impl Fn(&T) -> ViewResult<String> + Send + Sync + 'static,
    ) {
        let mut extensions = self.extensions.write().unwrap();
        Arc::make_mut(&mut extensions).stringables.insert(
            TypeId::of::<T>(),
            Arc::new(move |value| {
                let value = value.downcast_ref::<T>().ok_or_else(|| {
                    render_error("The Blade echo value does not match its registered type.")
                })?;
                format(value)
            }),
        );
    }

    /// Registers a formatter for arrays and maps used in Blade echo expressions.
    pub fn stringable_iterable(
        &self,
        format: impl Fn(&ViewValue) -> ViewResult<String> + Send + Sync + 'static,
    ) {
        Arc::make_mut(&mut self.extensions.write().unwrap()).iterable_echo = Some(Arc::new(format));
    }

    /// Formats a Rust value using the callback registered for its type.
    pub fn apply_echo_handler<T: 'static>(&self, value: &T) -> ViewResult<String> {
        let format = self
            .extensions
            .read()
            .unwrap()
            .stringables
            .get(&TypeId::of::<T>())
            .cloned()
            .ok_or_else(|| {
                render_error("A Blade echo formatter was not registered for this type.")
            })?;
        format(value)
    }

    /// Registers a predicate callable by name from a Blade attribute bag.
    pub fn attribute_filter(
        &self,
        name: &str,
        filter: impl Fn(&str, &ViewValue) -> bool + Send + Sync + 'static,
    ) -> ViewResult<()> {
        if !super::template::identifier(name) {
            return Err(invalid_registration("Invalid Blade attribute filter name."));
        }
        Arc::make_mut(&mut self.extensions.write().unwrap())
            .attribute_filters
            .insert(name.to_owned(), Arc::new(filter));
        Ok(())
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
        template.render(data, context).map(trim_php_output)
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
        self.validate_native(Path::new(&identity), source, &extensions)?;
        let template = Template::compile(source, &identity, &extensions)?;
        context.extensions = extensions;
        template.render(data, context).map(trim_php_output)
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
        // 即使命中磁盘缓存也重新核对当前模板与本二进制的完整原生块顺序。
        self.validate_native(path, source, extensions)?;
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
    fn get_compiled_path(&self, path: &Path) -> ViewResult<PathBuf> {
        Ok(self.directory.join(artifact_name(path)))
    }

    fn is_expired(&self, path: &Path) -> ViewResult<bool> {
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
        self.set_path(path);
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

// PhpEngine::evaluatePath uses PHP's byte-based ltrim, not Unicode whitespace.
fn trim_php_output(mut output: String) -> String {
    let prefix = output.len()
        - output
            .trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\0'])
            .len();
    output.drain(..prefix);
    output
}
