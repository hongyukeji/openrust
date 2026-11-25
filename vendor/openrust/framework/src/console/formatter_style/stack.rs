use super::{ConsoleFormatError, FormatterStyle, OutputFormatterStyle};
use std::sync::Arc;
/// 样式栈保留对象引用；克隆栈不复制其中的样式对象。
#[derive(Clone)]
pub struct OutputFormatterStyleStack {
    pub(super) styles: Vec<FormatterStyle>,
    pub(super) empty: FormatterStyle,
}
impl Default for OutputFormatterStyleStack {
    fn default() -> Self {
        Self::new(None)
    }
}
impl OutputFormatterStyleStack {
    pub fn new(empty: Option<FormatterStyle>) -> Self {
        Self {
            styles: Vec::new(),
            empty: empty.unwrap_or_else(|| Arc::new(OutputFormatterStyle::default())),
        }
    }
    pub fn reset(&mut self) {
        self.styles.clear();
    }
    pub fn push(&mut self, style: FormatterStyle) {
        self.styles.push(style);
    }
    pub fn pop(&mut self) -> Result<FormatterStyle, ConsoleFormatError> {
        Ok(self.styles.pop().unwrap_or_else(|| self.empty.clone()))
    }
    pub fn pop_with_style(
        &mut self,
        style: &FormatterStyle,
    ) -> Result<FormatterStyle, ConsoleFormatError> {
        if self.styles.is_empty() {
            return Ok(self.empty.clone());
        }
        for index in (0..self.styles.len()).rev() {
            if style.apply("")? == self.styles[index].apply("")? {
                let result = self.styles[index].clone();
                self.styles.truncate(index);
                return Ok(result);
            }
        }
        Err(ConsoleFormatError::IncorrectlyNestedTag)
    }
    pub fn get_current(&self) -> FormatterStyle {
        self.current_ref().clone()
    }
    pub fn set_empty_style(&mut self, style: FormatterStyle) -> &mut Self {
        self.empty = style;
        self
    }
    pub fn get_empty_style(&self) -> FormatterStyle {
        self.empty.clone()
    }
    pub(crate) fn current_ref(&self) -> &FormatterStyle {
        self.styles.last().unwrap_or(&self.empty)
    }
}

impl crate::contracts::service::ResetInterface for OutputFormatterStyleStack {
    fn reset(&mut self) {
        Self::reset(self);
    }
}
