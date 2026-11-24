use regex::Regex;
use std::{fmt, sync::LazyLock};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsoleFormatError {
    InvalidColor(String),
    InvalidOption(String),
    IncorrectlyNestedTag,
}
impl fmt::Display for ConsoleFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidColor(value) => write!(f, "控制台颜色无效：{value}"),
            Self::InvalidOption(value) => write!(f, "控制台样式选项无效：{value}"),
            Self::IncorrectlyNestedTag => f.write_str("控制台样式标签嵌套不匹配"),
        }
    }
}
impl std::error::Error for ConsoleFormatError {}

#[derive(Clone, Default, Eq, PartialEq)]
struct Style {
    fg: String,
    bg: String,
    options: Vec<(u8, u8)>,
    href: Option<String>,
}
impl Style {
    fn append(&self, text: &str, decorated: bool, target: &mut String) {
        if text.is_empty() {
            return;
        }
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

#[derive(Default)]
pub(super) struct Formatter {
    stack: Vec<Style>,
    warning: bool,
}
impl Formatter {
    pub(super) fn register_warning(&mut self) {
        self.warning = true;
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
        let mut output = String::new();
        let mut offset = 0;
        for token in TAGS.captures_iter(text) {
            let span = token.get(0).expect("标签范围存在");
            if span.start() > 0 && text.as_bytes()[span.start() - 1] == b'\\' {
                continue;
            }
            self.append(&text[offset..span.start()], decorated, &mut output);
            offset = span.end();
            let tag = &token[1];
            if tag == "/" {
                self.stack.pop();
                continue;
            }
            let (closing, name) = tag
                .strip_prefix('/')
                .map_or((false, tag), |name| (true, name));
            let Some(style) = self.style(name)? else {
                self.append(span.as_str(), decorated, &mut output);
                continue;
            };
            if !closing {
                self.stack.push(style);
            } else if !self.stack.is_empty() {
                let index = self
                    .stack
                    .iter()
                    .rposition(|entry| entry == &style)
                    .ok_or(ConsoleFormatError::IncorrectlyNestedTag)?;
                self.stack.truncate(index);
            }
        }
        self.append(&text[offset..], decorated, &mut output);
        Ok(output
            .replace('\0', "\\")
            .replace("\\<", "<")
            .replace("\\>", ">"))
    }
    fn append(&self, text: &str, decorated: bool, target: &mut String) {
        if let Some(style) = self.stack.last() {
            style.append(text, decorated, target);
        } else {
            target.push_str(text);
        }
    }
    fn style(&self, name: &str) -> Result<Option<Style>, ConsoleFormatError> {
        let mut style = Style::default();
        match name {
            "info" => style.fg = "32".into(),
            "comment" => style.fg = "33".into(),
            "question" => {
                style.fg = "30".into();
                style.bg = "46".into();
            }
            "error" => {
                style.fg = "37".into();
                style.bg = "41".into();
            }
            "warning" if self.warning => style.fg = "33".into(),
            _ => {
                static ATTRS: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r"([^=]+)=([^;]+)(?:;|$)").expect("合法控制台属性表达式")
                });
                let mut found = false;
                for attribute in ATTRS.captures_iter(name) {
                    found = true;
                    let value = attribute[2].to_lowercase();
                    match attribute[1].to_lowercase().as_str() {
                        "fg" => style.fg = color(&value, false)?,
                        "bg" => style.bg = color(&value, true)?,
                        "options" => {
                            for option in value.split(',').filter(|option| !option.is_empty()) {
                                let codes = match option {
                                    "bold" => (1, 22),
                                    "underscore" => (4, 24),
                                    "blink" => (5, 25),
                                    "reverse" => (7, 27),
                                    "conceal" => (8, 28),
                                    _ => {
                                        return Err(ConsoleFormatError::InvalidOption(
                                            option.into(),
                                        ));
                                    }
                                };
                                if !style.options.contains(&codes) {
                                    style.options.push(codes);
                                }
                            }
                        }
                        "href" => {
                            if hyperlinks_supported() {
                                style.href =
                                    Some(attribute[2].replace("\\<", "<").replace("\\>", ">"));
                            }
                        }
                        _ => return Ok(None),
                    }
                }
                if !found {
                    return Ok(None);
                }
            }
        }
        Ok(Some(style))
    }
}
fn hyperlinks_supported() -> bool {
    std::env::var("TERMINAL_EMULATOR").as_deref() != Ok("JetBrains-JediTerm")
        && std::env::var_os("IDEA_INITIAL_DIRECTORY").is_none()
        && std::env::var("KONSOLE_VERSION")
            .ok()
            .is_none_or(|value| value.is_empty() || value.parse::<u64>().unwrap_or(0) > 201100)
}
fn color(value: &str, background: bool) -> Result<String, ConsoleFormatError> {
    let base = if background { 40 } else { 30 };
    if value.starts_with('#') {
        let mut hex = value.replace('#', "");
        if hex.len() == 3 {
            hex = hex.chars().flat_map(|ch| [ch, ch]).collect();
        }
        if hex.len() != 6 {
            return Err(ConsoleFormatError::InvalidColor(value.into()));
        }
        let rgb = u32::from_str_radix(&hex, 16)
            .map_err(|_| ConsoleFormatError::InvalidColor(value.into()))?;
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
