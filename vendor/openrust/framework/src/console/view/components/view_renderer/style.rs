// 第三方许可见 ../LICENSE.renderer。只在已实现的样式范围内渲染，未知样式明确报错。
use super::super::display::{trim_text, visible_width};
use super::dimensions::{self, Constraint, Dimension, Dimensions, Target};
use super::dom::Node;
use super::{palette, style_tokens};
use crate::{
    console::{ConsoleError, Terminal},
    support::Str,
};
use regex::Regex;
use std::sync::Arc;
use std::sync::LazyLock;
#[derive(Clone, Default)]
pub(super) struct Style {
    pub(super) display: &'static str,
    pub(super) margin: [usize; 4],
    pub(super) padding: [usize; 4],
    pub(super) fg: Option<String>,
    pub(super) bg: Option<String>,
    pub(super) options: Vec<&'static str>,
    pub(super) modifiers: Vec<u8>,
    pub(super) color_order: Vec<&'static str>,
    pub(super) width: Option<Dimension>,
    pub(super) max: Option<Dimension>,
    pub(super) min: Option<Dimension>,
    pub(super) justify: Option<&'static str>,
    pub(super) align: &'static str,
    pub(super) transform: Vec<&'static str>,
    pub(super) repeat: Option<String>,
    pub(super) flex: bool,
    pub(super) space: [usize; 2],
    pub(super) href: Option<String>,
}
fn error(value: &str) -> ConsoleError {
    ConsoleError::new(format!("控制台组件暂不支持此终端样式：{value}"))
}
pub(super) fn classes(node: &Node) -> Result<Style, ConsoleError> {
    let mut s = Style {
        display: match node.tag() {
            "span" | "strong" | "b" | "i" | "em" | "u" | "s" | "a" | "br" => "inline",
            _ => "block",
        },
        ..Default::default()
    };
    match node.tag() {
        "table" | "code" | "pre" | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "hr" => {
            return Err(error(node.tag()));
        }
        "p" => {
            s.margin[0] = 1;
            s.margin[2] = 1;
        }
        "b" => s.options.push("bold"),

        "a" if !node.attr("href").is_empty() => s.href = Some(node.attr("href").to_owned()),
        _ => {}
    }
    for token in style_tokens::tokens(node.attr("class"), Terminal::new().get_width()) {
        let class = token.resolved;
        match class {
            "block" => s.display = "block",
            "flex" => s.display = "flex",
            "hidden" => s.display = "hidden",
            "flex-1" => s.flex |= token.original == "flex-1",
            "w-full" => s.width = Some(Dimension::Text("1/1".into())),
            "w-auto" => {
                s.width = None;
            }
            "uppercase" => {
                if !s.transform.contains(&"upper") {
                    s.transform.push("upper");
                }
            }
            "lowercase" => {
                if !s.transform.contains(&"lower") {
                    s.transform.push("lower");
                }
            }
            "capitalize" => {
                if !s.transform.contains(&"capitalize") {
                    s.transform.push("capitalize");
                }
            }
            "font-bold" => s.options.push("bold"),
            "strong" => {
                if !s.modifiers.contains(&1) {
                    s.modifiers.push(1);
                }
            }
            "italic" => {
                if !s.modifiers.contains(&3) {
                    s.modifiers.push(3);
                }
            }
            "underline" => {
                if !s.modifiers.contains(&4) {
                    s.modifiers.push(4);
                }
            }
            "line-through" => {
                if !s.modifiers.contains(&9) {
                    s.modifiers.push(9);
                }
            }
            "invisible" => {
                if !s.modifiers.contains(&8) {
                    s.modifiers.push(8);
                }
            }
            "justify-between" => s.justify = Some("between"),
            "justify-around" => s.justify = Some("around"),
            "justify-evenly" => s.justify = Some("evenly"),
            "justify-center" => s.justify = Some("center"),
            "text-center" => s.align = "center",
            "text-left" => s.align = "left",
            "text-right" => s.align = "right",
            _ => {
                if let Some(value) = class
                    .strip_prefix("content-repeat-[")
                    .and_then(|v| v.strip_suffix(']'))
                {
                    let value = value.trim_matches('\'');
                    if visible_width(value) == 0 {
                        return Err(error(class));
                    }
                    s.repeat = Some(value.to_owned());
                    if !s.transform.contains(&"repeat") {
                        s.transform.push("repeat");
                    }
                    continue;
                }
                if let Some(value) = class.strip_prefix("text-") {
                    let (base, _) = palette::arguments(value)?;
                    if matches!(base, "left" | "right" | "center") {
                        s.align = match base {
                            "left" => "left",
                            "right" => "right",
                            _ => "center",
                        };
                        continue;
                    }
                    if s.fg.is_none() {
                        s.color_order.push("fg");
                    }
                    s.fg = Some(palette::color(value)?);
                    continue;
                }
                if let Some(value) = class.strip_prefix("bg-") {
                    if s.bg.is_none() {
                        s.color_order.push("bg");
                    }
                    s.bg = Some(palette::color(value)?);
                    continue;
                }
                if class.starts_with("w-full-") {
                    s.width = Some(Dimension::Text("1/1".into()));
                    continue;
                }
                if class.starts_with("w-auto-") {
                    s.width = None;
                    continue;
                }
                let size = class
                    .strip_prefix("min-w-")
                    .map(|value| (1, value))
                    .or_else(|| class.strip_prefix("max-w-").map(|value| (2, value)))
                    .or_else(|| class.strip_prefix("w-").map(|value| (0, value)));
                if let Some((kind, value)) = size {
                    let value = Some(Dimension::parse(value)?);
                    match kind {
                        1 => s.min = value,
                        2 => s.max = value,
                        _ => s.width = value,
                    };
                    continue;
                }
                let Some((kind, value)) = class.rsplit_once('-') else {
                    return Err(error(class));
                };
                let n: usize = value.parse().map_err(|_| error(class))?;
                // 防止不可信自定义模板申请无界填充；终端布局不是文件大小接口。
                if n > 1_000_000 {
                    return Err(error(class));
                }
                match kind {
                    "space-x" => s.space[0] = n,
                    "space-y" => s.space[1] = n,
                    _ => {
                        let (slots, key) = if let Some(k) = kind.strip_prefix('m') {
                            (&mut s.margin, k)
                        } else if let Some(k) = kind.strip_prefix('p') {
                            (&mut s.padding, k)
                        } else {
                            return Err(error(class));
                        };
                        match key {
                            "" => *slots = [n; 4],
                            "x" => {
                                slots[1] = n;
                                slots[3] = n;
                            }
                            "y" => {
                                slots[0] = n;
                                slots[2] = n;
                            }
                            "t" => slots[0] = n,
                            "r" => slots[1] = n,
                            "b" => slots[2] = n,
                            "l" => slots[3] = n,
                            _ => return Err(error(class)),
                        }
                    }
                }
            }
        }
    }
    let code = match node.tag() {
        "strong" => Some(1),
        "em" | "i" => Some(3),
        "u" => Some(4),
        "s" => Some(9),
        _ => None,
    };
    if let Some(code) = code
        && !s.modifiers.contains(&code)
    {
        s.modifiers.push(code);
    }
    Ok(s)
}
impl Style {
    pub(super) fn dimensions(&self) -> Dimensions {
        Dimensions {
            width: self.width.clone(),
            min: self.min.clone(),
            max: self.max.clone(),
            margin: (self.margin[1] + self.margin[3]) as i64,
            padding: (self.padding[1] + self.padding[3]) as i64,
        }
    }
    pub(super) fn margins(&self, first: bool, spacing: [usize; 2]) -> [usize; 4] {
        let mut result = self.margin;
        if !first {
            if spacing[0] > 0 {
                result[3] = spacing[0];
            }
            if spacing[1] > 0 {
                result[0] = spacing[1];
            }
        }
        result
    }
}
pub(super) fn format(
    content: String,
    s: &Style,
    parent: &Option<Arc<Constraint>>,
    viewport: i64,
    first: bool,
    inherited_space: [usize; 2],
) -> Result<String, ConsoleError> {
    let mut text = content;
    for operation in &s.transform {
        text = match *operation {
            "upper" => Str::upper(&text),
            "lower" => Str::lower(&text),
            "capitalize" => Str::title(&text),
            "repeat" => {
                let repeat = s.repeat.as_ref().expect("重复变换具有内容");
                let terminal = usize::try_from(Terminal::new().get_width())
                    .map_err(ConsoleError::from_source)?;
                repeat.repeat(terminal / visible_width(repeat))
            }
            _ => text,
        };
    }
    match dimensions::target(&s.dimensions(), parent, viewport)? {
        Target::Unchanged => {}
        Target::Empty => text.clear(),
        Target::Columns(target) => {
            static NEWLINES: LazyLock<Regex> =
                LazyLock::new(|| Regex::new(r"\n+").expect("合法连续换行表达式"));
            let mut lines = NEWLINES.find_iter(&text);
            let first = lines.next();
            let groups = usize::from(first.is_some()) + lines.count();
            let target = target
                .checked_mul(i64::try_from(groups + 1).map_err(ConsoleError::from_source)?)
                .and_then(|n| n.checked_add(first.map_or(0, |m| m.len() as i64)))
                .filter(|n| n.unsigned_abs() <= 1_000_000)
                .ok_or_else(|| ConsoleError::new("组件填充宽度超出安全范围"))?;
            let length = visible_width(&text);
            if target < 0 || length > target as usize {
                text = trim_text(&text, target)?;
            } else {
                let remaining = target as usize - length;
                text = match s.align {
                    "right" => format!("{}{text}", " ".repeat(remaining)),
                    "center" => format!(
                        "{}{text}{}",
                        " ".repeat(remaining / 2),
                        " ".repeat(remaining - remaining / 2)
                    ),
                    _ => format!("{text}{}", " ".repeat(remaining)),
                };
            }
        }
    }
    for code in &s.modifiers {
        text = format!("\x1b[{code}m{text}\x1b[0m");
    }
    if s.display == "hidden" {
        return Ok(String::new());
    }
    let [mt, mr, mb, ml] = s.margins(first, inherited_space);
    let [pt, pr, pb, pl] = s.padding;
    let text = text.replace(
        '\n',
        &format!("{}\n{}", " ".repeat(mr + pr), " ".repeat(ml + pl)),
    );
    static BREAK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\r[ \t]?").expect("合法换行表达式"));
    let text = BREAK.replace_all(&text, "\n").into_owned();
    let mut tags = vec![];
    if let Some(href) = &s.href {
        tags.push(format!("href={href}"));
    }
    for key in &s.color_order {
        let value = if *key == "bg" { &s.bg } else { &s.fg };
        if let Some(value) = value {
            tags.push(format!("{key}={value}"));
        }
    }
    if !s.options.is_empty() {
        let mut options = s.options.clone();
        options.dedup();
        tags.push(format!("options={}", options.join(",")));
    }
    let wrap = |v: &str| {
        let body = format!("{}{v}{}", " ".repeat(pl), " ".repeat(pr));
        let body = if tags.is_empty() {
            body
        } else {
            format!("<{}>{body}</>", tags.join(";"))
        };
        format!("{}{body}{}", " ".repeat(ml), " ".repeat(mr))
    };
    let formatted = wrap(&text);
    let empty = if text.is_empty() {
        formatted.clone()
    } else {
        formatted.replace(&text, &" ".repeat(visible_width(&text)))
    };
    Ok(format!(
        "{}{}{}{}{}{}",
        if !first && matches!(s.display, "block" | "flex") {
            "\n"
        } else {
            ""
        },
        "\n".repeat(mt),
        if pt > 0 {
            format!("{empty}\n")
        } else {
            String::new()
        },
        formatted,
        if pb > 0 {
            format!("\n{empty}")
        } else {
            String::new()
        },
        "\n".repeat(mb)
    ))
}
