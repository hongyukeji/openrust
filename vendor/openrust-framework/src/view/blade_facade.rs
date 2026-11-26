use super::{
    ViewData, ViewError, ViewResult, ViewValue,
    compilers::{BladeCompiler, CompiledTemplate, Compiler, DirectiveCallback, DirectiveOutput},
};
use crate::{container::ServiceKey, support::facades::Facade};
use std::{
    any::Any,
    path::{Path, PathBuf},
};

/// 当前应用视图工厂实际使用的编译器门面。
pub struct Blade;
impl Facade for Blade {
    type Service = BladeCompiler;
    fn get_facade_accessor() -> ServiceKey {
        "blade.compiler".into()
    }
}
impl Blade {
    pub fn get_path() -> ViewResult<Option<PathBuf>> {
        Ok(Self::get_facade_root()?.get_path())
    }
    pub fn set_path(path: impl Into<PathBuf>) -> ViewResult<()> {
        Self::get_facade_root()?.set_path(path);
        Ok(())
    }
    pub fn compile(path: Option<&Path>) -> ViewResult<()> {
        let compiler = Self::get_facade_root()?;
        let path = path
            .map(Path::to_owned)
            .or_else(|| compiler.get_path())
            .ok_or_else(|| {
                ViewError::CompileFailed(Box::new(std::io::Error::other("尚未设置模板编译路径")))
            })?;
        compiler.compile(&path)
    }
    pub fn compile_string(source: &str) -> ViewResult<CompiledTemplate> {
        Self::get_facade_root()?.compile_string(source)
    }
    pub fn render(source: &str, data: &ViewData) -> ViewResult<String> {
        let compiled = Self::compile_string(source)?;
        let factory = crate::support::facades::View::get_facade_root()?;
        compiled.render_with_factory(data, Some(&factory))
    }
    pub fn get_compiled_path(path: impl AsRef<Path>) -> ViewResult<PathBuf> {
        Self::get_facade_root()?.get_compiled_path(path.as_ref())
    }
    pub fn is_expired(path: impl AsRef<Path>) -> ViewResult<bool> {
        Self::get_facade_root()?.is_expired(path.as_ref())
    }
    pub fn strip_parentheses(expression: impl AsRef<[u8]>) -> Vec<u8> {
        BladeCompiler::strip_parentheses(expression)
    }
    pub fn directive(
        name: &str,
        callback: impl Fn(&[ViewValue]) -> ViewResult<DirectiveOutput> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        Self::get_facade_root()?.directive(name, callback)
    }
    pub fn get_custom_directives()
    -> ViewResult<std::collections::BTreeMap<String, DirectiveCallback>> {
        Ok(Self::get_facade_root()?.get_custom_directives())
    }
    pub fn r#if(
        name: &str,
        callback: impl Fn(&[ViewValue]) -> ViewResult<bool> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        Self::get_facade_root()?.r#if(name, callback)
    }
    pub fn check(name: &str, values: &[ViewValue]) -> ViewResult<bool> {
        Self::get_facade_root()?.check(name, values)
    }
    pub fn with_double_encoding() -> ViewResult<()> {
        Self::get_facade_root()?.with_double_encoding();
        Ok(())
    }
    pub fn without_double_encoding() -> ViewResult<()> {
        Self::get_facade_root()?.without_double_encoding();
        Ok(())
    }
    pub fn stringable<T: 'static>(
        callback: impl Fn(&T) -> ViewResult<String> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        Self::get_facade_root()?.stringable(callback);
        Ok(())
    }
    pub fn stringable_iterable(
        callback: impl Fn(&ViewValue) -> ViewResult<String> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        Self::get_facade_root()?.stringable_iterable(callback);
        Ok(())
    }
    pub fn apply_echo_handler<T: Any>(value: &T) -> ViewResult<String> {
        Self::get_facade_root()?.apply_echo_handler(value)
    }
}
