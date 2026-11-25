use super::{
    ConsoleFormatError,
    formatting::{Style, color, hyperlinks_supported, style_option},
};
use std::sync::{Arc, Mutex, OnceLock};
mod stack;
mod stack_handle;
pub use stack::OutputFormatterStyleStack;
pub use stack_handle::OutputFormatterStyleStackHandle;

/// 样式对象以共享引用保留身份，设置操作仅修改该对象的配置。
pub type FormatterStyle = Arc<dyn OutputFormatterStyleInterface>;
/// 控制台样式接口；具体实现可通过Clone复用于格式器的独立副本。
pub trait OutputFormatterStyleInterface: dyn_clone::DynClone + Send + Sync {
    fn set_foreground(&self, color: Option<&str>) -> Result<(), ConsoleFormatError>;
    fn set_background(&self, color: Option<&str>) -> Result<(), ConsoleFormatError>;
    fn set_option(&self, option: &str) -> Result<(), ConsoleFormatError>;
    fn unset_option(&self, option: &str) -> Result<(), ConsoleFormatError>;
    fn set_options(&self, options: &[&str]) -> Result<(), ConsoleFormatError>;
    fn apply(&self, text: &str) -> Result<String, ConsoleFormatError>;
    /// 默认委托apply；内置样式直接写入同一缓冲，避免中间字符串。
    fn append_to(&self, text: &str, target: &mut String) -> Result<(), ConsoleFormatError> {
        target.push_str(&self.apply(text)?);
        Ok(())
    }
}
#[derive(Clone, Default)]
struct State {
    foreground: String,
    background: String,
    options: Vec<String>,
    color: Style,
    href: Option<String>,
}
impl State {
    fn rebuild(&mut self) -> Result<(), ConsoleFormatError> {
        let mut compiled = Style::default();
        if !self.foreground.is_empty() {
            compiled.fg = color(&self.foreground, false)?;
        }
        if !self.background.is_empty() {
            compiled.bg = color(&self.background, true)?;
        }
        for option in &self.options {
            let option = style_option(option)?;
            if !compiled.options.contains(&option) {
                compiled.options.push(option);
            }
        }
        self.color = compiled;
        Ok(())
    }
}
/// 默认颜色、选项和终端超链接样式，配置共享而显式Clone生成独立副本。
#[derive(Default)]
pub struct OutputFormatterStyle {
    state: Mutex<State>,
    handles_href: OnceLock<bool>,
}
impl Clone for OutputFormatterStyle {
    fn clone(&self) -> Self {
        Self {
            state: Mutex::new(
                self.state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            ),
            handles_href: self.handles_href.clone(),
        }
    }
}
impl OutputFormatterStyle {
    pub fn new(
        foreground: Option<&str>,
        background: Option<&str>,
        options: &[&str],
    ) -> Result<Self, ConsoleFormatError> {
        let mut state = State {
            foreground: truthy_color(foreground),
            background: truthy_color(background),
            options: options.iter().map(|option| (*option).to_owned()).collect(),
            ..State::default()
        };
        state.rebuild()?;
        Ok(Self {
            state: Mutex::new(state),
            handles_href: OnceLock::new(),
        })
    }
    pub fn set_foreground(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.foreground = truthy_color(color);
        state.rebuild()
    }
    pub fn set_background(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.background = truthy_color(color);
        state.rebuild()
    }
    pub fn set_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.options.push(option.to_owned());
        state.rebuild()
    }
    pub fn unset_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = state.options.iter().position(|entry| entry == option) {
            state.options.remove(index);
        }
        state.rebuild()
    }
    pub fn set_options(
        &self,
        options: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<(), ConsoleFormatError> {
        let options = options
            .into_iter()
            .map(|option| option.as_ref().to_owned())
            .collect();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.options = options;
        state.rebuild()
    }
    pub fn set_href(&self, url: impl Into<String>) {
        let url = url.into();
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .href = Some(url);
    }
    pub fn apply(&self, text: &str) -> Result<String, ConsoleFormatError> {
        let mut output = String::new();
        self.append_to(text, &mut output)?;
        Ok(output)
    }
    fn append_to(&self, text: &str, output: &mut String) -> Result<(), ConsoleFormatError> {
        let href_supported = *self.handles_href.get_or_init(hyperlinks_supported);
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 锁内不执行用户回调；颜色编码和目标均属于当前样式对象。
        if href_supported && state.href.is_some() {
            let mut style = state.color.clone();
            style.href = state.href.clone();
            style.append(text, true, output);
        } else {
            state.color.append(text, true, output);
        }
        Ok(())
    }
}
fn truthy_color(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty() && *value != "0")
        .unwrap_or_default()
        .to_owned()
}
impl OutputFormatterStyleInterface for OutputFormatterStyle {
    fn set_foreground(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        Self::set_foreground(self, color)
    }
    fn set_background(&self, color: Option<&str>) -> Result<(), ConsoleFormatError> {
        Self::set_background(self, color)
    }
    fn set_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        Self::set_option(self, option)
    }
    fn unset_option(&self, option: &str) -> Result<(), ConsoleFormatError> {
        Self::unset_option(self, option)
    }
    fn set_options(&self, options: &[&str]) -> Result<(), ConsoleFormatError> {
        Self::set_options(self, options)
    }
    fn apply(&self, text: &str) -> Result<String, ConsoleFormatError> {
        Self::apply(self, text)
    }
    fn append_to(&self, text: &str, output: &mut String) -> Result<(), ConsoleFormatError> {
        Self::append_to(self, text, output)
    }
}
