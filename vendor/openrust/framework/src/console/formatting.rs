use regex::Regex;
mod template;
pub(super) mod unicode;
use std::{fmt, sync::LazyLock};
pub use template::ConsoleTemplateError;

#[derive(Clone, Debug)]
pub enum ConsoleFormatError {
    InvalidColor(String),
    InvalidOption(String),
    UndefinedStyle(String),
    IncorrectlyNestedTag,
    UnicodeSegmentation(pcre2::Error),
    CustomSource(std::sync::Arc<dyn std::error::Error + Send + Sync>),
}
impl fmt::Display for ConsoleFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidColor(value) => write!(f, "控制台颜色无效：{value}"),
            Self::InvalidOption(value) => write!(f, "控制台样式选项无效：{value}"),
            Self::UndefinedStyle(value) => write!(f, "控制台样式未定义：{value}"),
            Self::IncorrectlyNestedTag => f.write_str("控制台样式标签嵌套不匹配"),
            Self::UnicodeSegmentation(error) => write!(f, "控制台文本字素解析失败：{error}"),
            Self::CustomSource(error) => write!(f, "控制台样式处理失败：{error}"),
        }
    }
}
impl ConsoleFormatError {
    pub fn from_source(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::CustomSource(std::sync::Arc::new(error))
    }
}
impl From<ConsoleFormatError> for super::ConsoleError {
    fn from(error: ConsoleFormatError) -> Self {
        Self::from_source(error)
    }
}
impl PartialEq for ConsoleFormatError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidColor(left), Self::InvalidColor(right))
            | (Self::InvalidOption(left), Self::InvalidOption(right))
            | (Self::UndefinedStyle(left), Self::UndefinedStyle(right)) => left == right,
            (Self::IncorrectlyNestedTag, Self::IncorrectlyNestedTag) => true,
            (Self::UnicodeSegmentation(left), Self::UnicodeSegmentation(right)) => {
                left.code() == right.code()
                    && left.offset() == right.offset()
                    && std::mem::discriminant(left.kind()) == std::mem::discriminant(right.kind())
            }
            (Self::CustomSource(left), Self::CustomSource(right)) => {
                std::sync::Arc::ptr_eq(left, right)
            }
            _ => false,
        }
    }
}
impl Eq for ConsoleFormatError {}
impl std::error::Error for ConsoleFormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CustomSource(error) => Some(error.as_ref()),
            Self::UnicodeSegmentation(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Default, Eq, PartialEq)]
pub(super) struct Style {
    pub(super) fg: String,
    pub(super) bg: String,
    pub(super) options: Vec<(u8, u8)>,
    pub(super) href: Option<String>,
}
impl Style {
    pub(super) fn append(&self, text: &str, decorated: bool, target: &mut String) {
        if !decorated {
            target.push_str(text);
            return;
        }
        let mut open = Vec::new();
        let mut close = Vec::new();
        if !self.fg.is_empty() {
            open.push(self.fg.clone());
            close.push("39".into());
        }
        if !self.bg.is_empty() {
            open.push(self.bg.clone());
            close.push("49".into());
        }
        for (start, end) in &self.options {
            open.push(start.to_string());
            close.push(end.to_string());
        }
        if !open.is_empty() {
            target.push_str(&format!("\x1b[{}m", open.join(";")));
        }
        if let Some(href) = &self.href {
            target.push_str(&format!("\x1b]8;;{href}\x1b\\"));
        }
        target.push_str(text);
        if self.href.is_some() {
            target.push_str("\x1b]8;;\x1b\\");
        }
        if !close.is_empty() {
            target.push_str(&format!("\x1b[{}m", close.join(";")));
        }
    }
}

pub(super) struct Formatter {
    inner: super::FormatterHandle,
}
impl Default for Formatter {
    fn default() -> Self {
        Self {
            inner: std::sync::Arc::new(super::OutputFormatter::default()),
        }
    }
}
impl Clone for Formatter {
    fn clone(&self) -> Self {
        Self {
            inner: std::sync::Arc::from(dyn_clone::clone_box(self.inner.as_ref())),
        }
    }
}
impl Formatter {
    pub(super) fn from_handle(inner: super::FormatterHandle) -> Self {
        Self { inner }
    }
    pub(super) fn handle(&self) -> super::FormatterHandle {
        self.inner.clone()
    }
    pub(super) fn set_decorated(&self, decorated: bool) {
        self.inner.set_decorated(decorated);
    }
    pub(super) fn is_decorated(&self) -> bool {
        self.inner.is_decorated()
    }
    pub(super) fn format_current(&self, text: &str) -> Result<String, ConsoleFormatError> {
        Ok(self.inner.format(Some(text))?.unwrap_or_default())
    }
    pub(super) fn format(
        &mut self,
        text: &str,
        decorated: bool,
    ) -> Result<String, ConsoleFormatError> {
        if self.is_decorated() == decorated {
            return self.format_current(text);
        }
        let previous = self.is_decorated();
        self.set_decorated(decorated);
        let rendered = self.format_current(text)?;
        self.set_decorated(previous);
        Ok(rendered)
    }
    pub(super) fn register_warning(&mut self) {
        if !self.inner.has_style("warning") {
            self.inner.set_style(
                "warning",
                std::sync::Arc::new(
                    super::OutputFormatterStyle::new(Some("yellow"), None, &[])
                        .expect("内置警告颜色有效"),
                ),
            );
        }
    }
    pub(super) fn escape(text: &str) -> String {
        super::OutputFormatter::escape(text)
    }
    pub(super) fn escape_trailing_backslash(text: &str) -> String {
        super::OutputFormatter::escape_trailing_backslash(text)
    }
}
pub(super) fn hyperlinks_supported() -> bool {
    std::env::var("TERMINAL_EMULATOR").as_deref() != Ok("JetBrains-JediTerm")
        && std::env::var_os("IDEA_INITIAL_DIRECTORY").is_none()
        && std::env::var("KONSOLE_VERSION")
            .ok()
            .is_none_or(|value| value.is_empty() || value.parse::<u64>().unwrap_or(0) > 201100)
}
pub(super) fn color(value: &str, background: bool) -> Result<String, ConsoleFormatError> {
    let base = if background { 40 } else { 30 };
    if value.starts_with('#') {
        let mut hex = value.replace('#', "");
        if hex.len() == 3 {
            hex = hex.chars().flat_map(|ch| [ch, ch]).collect();
        }
        if hex.len() != 6 {
            return Err(ConsoleFormatError::InvalidColor(value.into()));
        }
        // 固定数值转换会忽略非十六进制字符；VM弃用告警不属于原生输出协议。
        let digits: String = hex.chars().filter(char::is_ascii_hexdigit).collect();
        let rgb = if digits.is_empty() {
            0
        } else {
            u32::from_str_radix(&digits, 16).expect("已过滤的六位十六进制位于整数范围内")
        };
        let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
        let mode = ["COLORTERM", "TERM"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .map(|value| value.to_lowercase())
            .find_map(|value| {
                if value.contains("truecolor") {
                    Some(24)
                } else if value.contains("256color") {
                    Some(8)
                } else {
                    None
                }
            })
            .unwrap_or(4);
        return Ok(match mode {
            24 => format!("{};2;{r};{g};{b}", if background { 48 } else { 38 }),
            8 => {
                let index = if r == g && g == b {
                    if r < 8 {
                        16
                    } else if r > 248 {
                        231
                    } else {
                        232 + ((r - 8) * 24 + 123) / 247
                    }
                } else {
                    16 + 36 * ((r * 5 + 127) / 255)
                        + 6 * ((g * 5 + 127) / 255)
                        + (b * 5 + 127) / 255
                };
                format!("{};5;{index}", if background { 48 } else { 38 })
            }
            _ => (base
                + ((u32::from(b >= 128) << 2) | (u32::from(g >= 128) << 1) | u32::from(r >= 128)))
            .to_string(),
        });
    }
    if let Some(index) = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ]
    .iter()
    .position(|name| *name == value)
    {
        return Ok((base + u32::try_from(index).expect("八种基础颜色")).to_string());
    }
    if value == "default" {
        return Ok((base + 9).to_string());
    }
    if let Some(index) = [
        "gray",
        "bright-red",
        "bright-green",
        "bright-yellow",
        "bright-blue",
        "bright-magenta",
        "bright-cyan",
        "bright-white",
    ]
    .iter()
    .position(|name| *name == value)
    {
        return Ok(((if background { 100 } else { 90 }) + index).to_string());
    }
    Err(ConsoleFormatError::InvalidColor(value.into()))
}

/// 可见字符的字素计数复用已有Unicode解析。
pub(super) fn grapheme_length(text: &str) -> Result<usize, ConsoleFormatError> {
    unicode::length(text)
}

pub(super) fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() {
                1
            } else {
                let mut bytes = [0; 4];
                ::console::measure_text_width(ch.encode_utf8(&mut bytes))
            }
        })
        .sum()
}

pub(super) fn format_template(
    template: &str,
    value: &serde_json::Value,
) -> Result<String, ConsoleTemplateError> {
    self::template::format(template, value)
}

/// 移除格式标签、SGR及终端超链接，保留其他控制序列的宽度语义。
pub(super) fn unformatted_text(
    formatter: &mut Formatter,
    text: &str,
) -> Result<String, ConsoleFormatError> {
    let previous = formatter.is_decorated();
    formatter.set_decorated(false);
    let mut plain = formatter.format_current(text)?;
    formatter.set_decorated(previous);
    if !plain.contains('\x1b') {
        return Ok(plain);
    }
    static SGR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\x1b\[[^m]*m").expect("合法SGR表达式"));
    static LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\x1b]8;[^;]*;[^\x1b]*\x1b\\").expect("合法终端超链接表达式"));
    if SGR.is_match(&plain) {
        plain = SGR.replace_all(&plain, "").into_owned();
    }
    if LINK.is_match(&plain) {
        plain = LINK.replace_all(&plain, "").into_owned();
    }
    Ok(plain)
}

pub(super) fn style_option(value: &str) -> Result<(u8, u8), ConsoleFormatError> {
    match value {
        "bold" => Ok((1, 22)),
        "underscore" => Ok((4, 24)),
        "blink" => Ok((5, 25)),
        "reverse" => Ok((7, 27)),
        "conceal" => Ok((8, 28)),
        _ => Err(ConsoleFormatError::InvalidOption(value.into())),
    }
}
