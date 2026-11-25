// 第三方许可见 LICENSE。
use crate::{
    console::ConsoleError,
    contracts::{container::Container as _, foundation::Application as _},
    support::facades::{App, Facade as _, current_container},
};
use regex::Regex;
use serde_json::Value;
use std::{borrow::Cow, sync::LazyLock};

/// 组件文本变换；显式方法对应可调用对象，不要求不稳定的 Fn 实现。
pub trait Mutator: Send + Sync {
    fn apply(&self, value: &str) -> Result<String, ConsoleError>;
    fn apply_value(&self, value: Value) -> Result<Value, ConsoleError> {
        self.apply(&value_text(&value)).map(Value::String)
    }
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
    fn apply_value(&self, value: Value) -> Result<Value, ConsoleError> {
        if punctuated(&value_text(&value)) {
            self.apply(&value_text(&value)).map(Value::String)
        } else {
            Ok(value)
        }
    }

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
    fn apply_value(&self, value: Value) -> Result<Value, ConsoleError> {
        if current_container().is_none_or(|container| !container.has("path.base")) {
            return Ok(value);
        }
        let replace = |value: Value| self.apply(&value_text(&value)).map(Value::String);
        match value {
            Value::Array(values) => values
                .into_iter()
                .map(replace)
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(values) => values
                .into_iter()
                .map(|(key, value)| replace(value).map(|value| (key, value)))
                .collect::<Result<_, _>>()
                .map(Value::Object),
            value => replace(value),
        }
    }
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

fn value_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(text) => Cow::Borrowed(text),
        Value::Array(_) | Value::Object(_) => Cow::Borrowed("Array"),
        Value::Null | Value::Bool(false) => Cow::Borrowed(""),
        Value::Bool(true) => Cow::Borrowed("1"),
        Value::Number(_) => Cow::Owned(crate::support::input_scalars::string(value.clone())),
    }
}
