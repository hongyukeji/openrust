use super::{
    ConsoleFormatError, FormatterStyle, OutputFormatterStyle, OutputFormatterStyleStack,
    OutputFormatterStyleStackHandle,
};
use parking_lot::ReentrantMutex;
use regex::Regex;
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex},
};
mod wrapping;

/// 输出持有同一格式器对象的引用，显式克隆具体类型才生成独立配置。
pub type FormatterHandle = Arc<dyn OutputFormatterInterface>;
pub trait OutputFormatterInterface: dyn_clone::DynClone + Send + Sync + Any {
    fn set_decorated(&self, decorated: bool);
    fn is_decorated(&self) -> bool;
    fn set_style(&self, name: &str, style: FormatterStyle);
    fn has_style(&self, name: &str) -> bool;
    fn get_style(&self, name: &str) -> Result<FormatterStyle, ConsoleFormatError>;
    fn format(&self, message: Option<&str>) -> Result<Option<String>, ConsoleFormatError>;
    /// 返回可换行能力，避免对类型擦除后的对象进行不安全强制转换。
    fn as_wrappable(&self) -> Option<&dyn WrappableOutputFormatterInterface> {
        None
    }
}
pub trait WrappableOutputFormatterInterface: OutputFormatterInterface {
    fn format_and_wrap(
        &self,
        message: Option<&str>,
        width: i64,
    ) -> Result<String, ConsoleFormatError>;
}
struct Config {
    decorated: bool,
    styles: BTreeMap<String, FormatterStyle>,
}
/// 默认标签格式器。排序锁只保护调用顺序，回调不持有配置或栈的数据锁。
pub struct OutputFormatter {
    order: Arc<ReentrantMutex<()>>,
    config: Mutex<Config>,
    stack: OutputFormatterStyleStackHandle,
}
impl Default for OutputFormatter {
    fn default() -> Self {
        Self::new(false)
    }
}
impl Clone for OutputFormatter {
    fn clone(&self) -> Self {
        let _order = self.order.lock();
        let (decorated, styles) = {
            let config = self
                .config
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (config.decorated, config.styles.clone())
        };
        let stack = self.stack.snapshot();
        let styles = styles
            .into_iter()
            .map(|(name, style)| (name, Arc::from(dyn_clone::clone_box(style.as_ref()))))
            .collect();
        let order = Arc::new(ReentrantMutex::new(()));
        Self {
            config: Mutex::new(Config { decorated, styles }),
            stack: OutputFormatterStyleStackHandle::new(stack, order.clone()),
            order,
        }
    }
}
impl OutputFormatter {
    pub fn new(decorated: bool) -> Self {
        let styles = [
            ("error", Some("white"), Some("red")),
            ("info", Some("green"), None),
            ("comment", Some("yellow"), None),
            ("question", Some("black"), Some("cyan")),
        ]
        .into_iter()
        .map(|(name, fg, bg)| {
            let style: FormatterStyle =
                Arc::new(OutputFormatterStyle::new(fg, bg, &[]).expect("内置格式器颜色有效"));
            (name.to_owned(), style)
        })
        .collect();
        let order = Arc::new(ReentrantMutex::new(()));
        Self {
            config: Mutex::new(Config { decorated, styles }),
            stack: OutputFormatterStyleStackHandle::new(
                OutputFormatterStyleStack::default(),
                order.clone(),
            ),
            order,
        }
    }
    pub fn new_with_styles<S: AsRef<str>>(
        decorated: bool,
        styles: impl IntoIterator<Item = (S, FormatterStyle)>,
    ) -> Self {
        let formatter = Self::new(decorated);
        for (name, style) in styles {
            formatter.set_style(name.as_ref(), style);
        }
        formatter
    }
    pub fn set_decorated(&self, decorated: bool) {
        let _order = self.order.lock();
        self.config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .decorated = decorated;
    }
    pub fn is_decorated(&self) -> bool {
        let _order = self.order.lock();
        self.config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .decorated
    }
    pub fn set_style(&self, name: &str, style: FormatterStyle) {
        let name = name.to_ascii_lowercase();
        let _order = self.order.lock();
        let old = self
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .styles
            .insert(name, style);
        drop(old);
    }
    pub fn has_style(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        let _order = self.order.lock();
        self.config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .styles
            .contains_key(&name)
    }
    pub fn get_style(&self, name: &str) -> Result<FormatterStyle, ConsoleFormatError> {
        let _order = self.order.lock();
        self.config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .styles
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| ConsoleFormatError::UndefinedStyle(name.to_owned()))
    }
    pub fn get_style_stack(&self) -> OutputFormatterStyleStackHandle {
        self.stack.clone()
    }
    pub fn format(&self, message: &str) -> Result<String, ConsoleFormatError> {
        self.format_optional(Some(message))
    }
    pub fn format_optional(&self, message: Option<&str>) -> Result<String, ConsoleFormatError> {
        self.format_and_wrap(message, 0)
    }
    pub fn format_and_wrap(
        &self,
        message: Option<&str>,
        width: i64,
    ) -> Result<String, ConsoleFormatError> {
        let Some(text) = message else {
            return Ok(String::new());
        };
        let _order = self.order.lock();
        static TAGS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)<([a-z](?:[^\\<>]|\\.)*|/[a-z][^<>]*|/)>")
                .expect("合法控制台标签表达式")
        });
        let non_ascii = !text.is_ascii();
        let mut unicode = None;
        let mut output = String::new();
        let mut offset = 0;
        let mut line_length = 0;
        for token in TAGS.captures_iter(text) {
            let span = token.get(0).expect("标签范围存在");
            if span.start() > 0 && text.as_bytes()[span.start() - 1] == b'\\' {
                continue;
            }
            if non_ascii && unicode.is_none() {
                unicode = Some(super::formatting::unicode::Slices::new(text)?);
            }
            let position = unicode
                .as_ref()
                .map_or(span.start(), |slices| slices.prefix_length(span.start()));
            let visible = unicode.as_ref().map_or_else(
                || &text[offset..span.start()],
                |slices| slices.slice(offset, Some(position)),
            );
            let part = self.apply_current(visible, &output, width, &mut line_length)?;
            output.push_str(&part);
            offset = if unicode.is_some() {
                position + super::formatting::unicode::length(span.as_str())?
            } else {
                span.end()
            };
            let tag = &token[1];
            if tag == "/" {
                self.stack.pop()?;
                continue;
            }
            let (closing, name) = tag
                .strip_prefix('/')
                .map_or((false, tag), |name| (true, name));
            let Some(style) = self.create_style(name)? else {
                let part = self.apply_current(span.as_str(), &output, width, &mut line_length)?;
                output.push_str(&part);
                continue;
            };
            if closing {
                self.stack.pop_with_style(&style)?;
            } else {
                self.stack.push(style);
            }
        }
        let visible = match unicode.as_ref() {
            Some(slices) => std::borrow::Cow::Borrowed(slices.slice(offset, None)),
            None if non_ascii => icu_normalizer::ComposingNormalizer::new_nfc().normalize(text),
            None => std::borrow::Cow::Borrowed(&text[offset..]),
        };
        let part = self.apply_current(visible.as_ref(), &output, width, &mut line_length)?;
        output.push_str(&part);
        Ok(output
            .replace('\0', "\\")
            .replace("\\<", "<")
            .replace("\\>", ">"))
    }
    fn create_style(&self, name: &str) -> Result<Option<FormatterStyle>, ConsoleFormatError> {
        if let Some(style) = self
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .styles
            .get(name)
            .cloned()
        {
            return Ok(Some(style));
        }
        static ATTRS: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"([^=]+)=([^;]+)(?:;|$)").expect("合法控制台属性表达式"));
        let style = OutputFormatterStyle::default();
        let mut found = false;
        for attribute in ATTRS.captures_iter(name) {
            found = true;
            let value = attribute[2].to_ascii_lowercase();
            match attribute[1].to_ascii_lowercase().as_str() {
                "fg" => style.set_foreground(Some(&value))?,
                "bg" => style.set_background(Some(&value))?,
                "options" => {
                    for option in value.split(',').filter(|option| !option.is_empty()) {
                        style.set_option(option)?;
                    }
                }
                "href" => style.set_href(attribute[2].replace("\\<", "<").replace("\\>", ">")),
                _ => return Ok(None),
            }
        }
        Ok(found.then(|| Arc::new(style) as FormatterStyle))
    }
    pub fn escape(text: &str) -> String {
        static ESCAPE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"([^\\]|^)([<>])").expect("合法控制台转义表达式"));
        let escaped = ESCAPE.replace_all(text, |captures: &regex::Captures<'_>| {
            format!("{}\\{}", &captures[1], &captures[2])
        });
        Self::escape_trailing_backslash(&escaped)
    }
    pub fn escape_trailing_backslash(text: &str) -> String {
        if !text.ends_with('\\') {
            return text.to_owned();
        }
        let mut prefix = text.trim_end_matches('\\').replace('\0', "");
        let count = text.len() - prefix.len();
        prefix.extend(std::iter::repeat_n('\0', count));
        prefix
    }
}
impl OutputFormatterInterface for OutputFormatter {
    fn set_decorated(&self, decorated: bool) {
        Self::set_decorated(self, decorated);
    }
    fn is_decorated(&self) -> bool {
        Self::is_decorated(self)
    }
    fn set_style(&self, name: &str, style: FormatterStyle) {
        Self::set_style(self, name, style);
    }
    fn has_style(&self, name: &str) -> bool {
        Self::has_style(self, name)
    }
    fn get_style(&self, name: &str) -> Result<FormatterStyle, ConsoleFormatError> {
        Self::get_style(self, name)
    }
    fn format(&self, message: Option<&str>) -> Result<Option<String>, ConsoleFormatError> {
        Self::format_optional(self, message).map(Some)
    }
    fn as_wrappable(&self) -> Option<&dyn WrappableOutputFormatterInterface> {
        Some(self)
    }
}
impl WrappableOutputFormatterInterface for OutputFormatter {
    fn format_and_wrap(
        &self,
        message: Option<&str>,
        width: i64,
    ) -> Result<String, ConsoleFormatError> {
        Self::format_and_wrap(self, message, width)
    }
}
