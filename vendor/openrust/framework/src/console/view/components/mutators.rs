// 第三方许可见 LICENSE。
use crate::{
    console::ConsoleError,
    contracts::{container::Container as _, foundation::Application as _},
    support::facades::{App, Facade as _, current_container},
};
use regex::Regex;
use std::sync::LazyLock;

/// 组件文本变换；显式方法对应可调用对象，不要求不稳定的 Fn 实现。
pub trait Mutator: Send + Sync {
    fn apply(&self, value: &str) -> Result<String, ConsoleError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnsureDynamicContentIsHighlighted;
impl Mutator for EnsureDynamicContentIsHighlighted {
    fn apply(&self, value: &str) -> Result<String, ConsoleError> {
        static CONTENT: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\[([^\]]+)\]").expect("固定的动态内容表达式"));
        Ok(CONTENT
            .replace_all(value, "<options=bold>[$1]</>")
            .into_owned())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EnsurePunctuation;
impl Mutator for EnsurePunctuation {
    fn apply(&self, value: &str) -> Result<String, ConsoleError> {
        if punctuated(value) {
            Ok(value.to_owned())
        } else {
            Ok(format!("{value}."))
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EnsureNoPunctuation;
impl Mutator for EnsureNoPunctuation {
    fn apply(&self, value: &str) -> Result<String, ConsoleError> {
        Ok(if punctuated(value) {
            &value[..value.len() - 1]
        } else {
            value
        }
        .to_owned())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EnsureRelativePaths;
impl Mutator for EnsureRelativePaths {
    fn apply(&self, value: &str) -> Result<String, ConsoleError> {
        let Some(container) = current_container() else {
            return Ok(value.to_owned());
        };
        if !container.has("path.base") {
            return Ok(value.to_owned());
        }
        let app = App::get_facade_root().map_err(ConsoleError::from_source)?;
        let prefix = format!("{}/", app.base_path().display());
        Ok(value.replace(&prefix, ""))
    }
}
fn punctuated(value: &str) -> bool {
    matches!(value.as_bytes().last(), Some(b'.' | b'?' | b'!' | b':'))
}
