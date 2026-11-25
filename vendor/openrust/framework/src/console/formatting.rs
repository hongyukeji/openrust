use regex::Regex;
mod template;
mod unicode;
use std::{fmt, sync::LazyLock};
pub use template::ConsoleTemplateError;

#[derive(Clone, Debug)]
pub enum ConsoleFormatError {
    InvalidColor(String),
    InvalidOption(String),
    IncorrectlyNestedTag,
    UnicodeSegmentation(pcre2::Error),
    CustomSource(std::sync::Arc<dyn std::error::Error + Send + Sync>),
}
impl fmt::Display for ConsoleFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidColor(value) => write!(f, "控制台颜色无效：{value}"),
            Self::InvalidOption(value) => write!(f, "控制台样式选项无效：{value}"),
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
            | (Self::InvalidOption(left), Self::InvalidOption(right)) => left == right,
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
    stack: super::OutputFormatterStyleStack,
    styles: std::collections::BTreeMap<String, super::FormatterStyle>,
}
impl Default for Formatter {
    fn default() -> Self {
        let styles = [
            ("error", Some("white"), Some("red")),
            ("info", Some("green"), None),
            ("comment", Some("yellow"), None),
            ("question", Some("black"), Some("cyan")),
        ]
        .into_iter()
        .map(|(name, fg, bg)| {
            let style: super::FormatterStyle = std::sync::Arc::new(
                super::OutputFormatterStyle::new(fg, bg, &[]).expect("内置颜色配置有效"),
            );
            (name.to_owned(), style)
        })
        .collect();
        Self {
            stack: super::OutputFormatterStyleStack::default(),
            styles,
        }
    }
}
impl Clone for Formatter {
    fn clone(&self) -> Self {
        Self {
            stack: self.stack.clone(),
            styles: self
                .styles
                .iter()
                .map(|(name, style)| {
                    (
                        name.clone(),
                        std::sync::Arc::from(dyn_clone::clone_box(style.as_ref())),
                    )
                })
                .collect(),
        }
    }
}
impl Formatter {
    pub(super) fn escape(text: &str) -> String {
        let mut escaped = String::new();
        let mut previous = None;
        for character in text.chars() {
            if matches!(character, '<' | '>') && previous != Some('\\') {
                escaped.push('\\');
            }
            escaped.push(character);
            previous = Some(character);
        }
        Self::escape_trailing_backslash(&escaped)
    }
    pub(super) fn escape_trailing_backslash(text: &str) -> String {
        if !text.ends_with('\\') {
            return text.to_owned();
        }
        let mut prefix = text.trim_end_matches('\\').replace('\0', "");
        let count = text.len() - prefix.len();
        prefix.extend(std::iter::repeat_n('\0', count));
        prefix
    }
    pub(super) fn register_warning(&mut self) {
        self.styles.entry("warning".into()).or_insert_with(|| {
            std::sync::Arc::new(
                super::OutputFormatterStyle::new(Some("yellow"), None, &[])
                    .expect("内置警告颜色有效"),
            )
        });
    }
    pub(super) fn format(
        &mut self,
        text: &str,
        decorated: bool,
    ) -> Result<String, ConsoleFormatError> {
        static TAGS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)<([a-z](?:[^\\<>]|\\.)*|/[a-z][^<>]*|/)>")
                .expect("合法控制台标签表达式")
        });
        let non_ascii = !text.is_ascii();
        let mut unicode = None;
        let mut output = String::new();
        let mut offset = 0;
        for token in TAGS.captures_iter(text) {
            let span = token.get(0).expect("标签范围存在");
            if span.start() > 0 && text.as_bytes()[span.start() - 1] == b'\\' {
                continue;
            }
            if non_ascii && unicode.is_none() {
                unicode = Some(unicode::Slices::new(text)?);
            }
            let position = unicode
                .as_ref()
                .map_or(span.start(), |slices| slices.prefix_length(span.start()));
            let visible = unicode.as_ref().map_or_else(
                || &text[offset..span.start()],
                |slices| slices.slice(offset, Some(position)),
            );
            self.append(visible, decorated, &mut output)?;
            offset = if unicode.is_some() {
                position + unicode::length(span.as_str())?
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
            let Some(style) = self.style(name)? else {
                self.append(span.as_str(), decorated, &mut output)?;
                continue;
            };
            if !closing {
                self.stack.push(style);
            } else {
                self.stack.pop_with_style(&style)?;
            }
        }
        let visible = match unicode.as_ref() {
            Some(slices) => std::borrow::Cow::Borrowed(slices.slice(offset, None)),
            None if non_ascii => icu_normalizer::ComposingNormalizer::new_nfc().normalize(text),
            None => std::borrow::Cow::Borrowed(&text[offset..]),
        };
        self.append(visible.as_ref(), decorated, &mut output)?;
        Ok(output
            .replace('\0', "\\")
            .replace("\\<", "<")
            .replace("\\>", ">"))
    }
    fn append(
        &self,
        text: &str,
        decorated: bool,
        target: &mut String,
    ) -> Result<(), ConsoleFormatError> {
        if text.is_empty() {
            return Ok(());
        }
        if decorated {
            self.stack.current_ref().append_to(text, target)?;
        } else {
            target.push_str(text);
        }
        Ok(())
    }
    fn style(&self, name: &str) -> Result<Option<super::FormatterStyle>, ConsoleFormatError> {
        if let Some(style) = self.styles.get(name) {
            return Ok(Some(style.clone()));
        }
        static ATTRS: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"([^=]+)=([^;]+)(?:;|$)").expect("合法控制台属性表达式"));
        let style = super::OutputFormatterStyle::default();
        let mut found = false;
        for attribute in ATTRS.captures_iter(name) {
            found = true;
            let value = attribute[2].to_lowercase();
            match attribute[1].to_lowercase().as_str() {
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
        if !found {
            return Ok(None);
        }
        Ok(Some(std::sync::Arc::new(style)))
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
    let mut plain = formatter.format(text, false)?;
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
