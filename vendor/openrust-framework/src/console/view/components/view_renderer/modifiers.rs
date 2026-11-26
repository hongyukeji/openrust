// 第三方许可见 ../LICENSE.renderer。
use crate::console::ConsoleError;
use indexmap::IndexMap;
use serde_json::Value;
use std::sync::Arc;

type TextModifier = dyn Fn(&str, &Value, &Value) -> Result<String, ConsoleError> + Send + Sync;
type StyleModifier = dyn Fn(&str, &Value) -> Result<String, ConsoleError> + Send + Sync;

/// 独立样式的自定义文本和格式修饰器。各组按注册顺序执行。
#[derive(Clone, Default)]
pub struct TerminalStyleModifiers {
    text: IndexMap<String, Arc<TextModifier>>,
    style: IndexMap<String, Arc<StyleModifier>>,
}

impl TerminalStyleModifiers {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在宽度计算前修改文本，可读取当前样式与祖先样式。
    pub fn set_text(
        &mut self,
        name: impl Into<String>,
        modifier: impl Fn(&str, &Value, &Value) -> Result<String, ConsoleError> + Send + Sync + 'static,
    ) -> &mut Self {
        self.text.insert(name.into(), Arc::new(modifier));
        self
    }

    /// 在宽度计算后、输出格式标记封装前修改文本。
    pub fn set_style(
        &mut self,
        name: impl Into<String>,
        modifier: impl Fn(&str, &Value) -> Result<String, ConsoleError> + Send + Sync + 'static,
    ) -> &mut Self {
        self.style.insert(name.into(), Arc::new(modifier));
        self
    }

    pub(super) fn modify_text(
        &self,
        mut text: String,
        style: &Value,
        ancestors: &Value,
    ) -> Result<String, ConsoleError> {
        for modifier in self.text.values() {
            text = modifier(&text, style, ancestors)?;
        }
        Ok(text)
    }

    pub(super) fn modify_style(
        &self,
        mut text: String,
        style: &Value,
    ) -> Result<String, ConsoleError> {
        for modifier in self.style.values() {
            text = modifier(&text, style)?;
        }
        Ok(text)
    }
}
