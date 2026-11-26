// 第三方许可见 ../LICENSE.renderer。只在已实现的样式范围内渲染，未知样式明确报错。
use super::super::display::{trim_text, visible_width};
use super::dimensions::{self, Constraint, Dimension, Dimensions, Target};
use super::dom::Node;
use super::{
    StyleRepository, TerminalStyles, palette,
    style_methods::{self, StyleArgument as Argument},
    style_tokens,
};
use crate::{
    console::{ConsoleError, Terminal},
    support::Str,
};
use regex::Regex;
use std::sync::Arc;
use std::sync::LazyLock;
#[derive(Clone, Default)]
pub(super) struct Style {
    pub(super) element: &'static str,
    pub(super) display: &'static str,
    pub(super) margin: [usize; 4],
    pub(super) padding: [usize; 4],
    pub(super) fg: Option<String>,
    pub(super) bg: Option<String>,
    pub(super) bold: Option<bool>,
    pub(super) modifiers: Vec<u8>,
    pub(super) color_order: Vec<&'static str>,
    pub(super) width: Option<Dimension>,
    pub(super) max: Option<Dimension>,
    pub(super) min: Option<Dimension>,
    pub(super) justify: Option<&'static str>,
    pub(super) align: &'static str,
    pub(super) transform: Vec<&'static str>,
    pub(super) repeat: Option<String>,
    pub(super) prepend: String,
    pub(super) append: String,
    pub(super) truncate: (i64, String),
    pub(super) markers: Vec<String>,
    pub(super) flex: bool,
    pub(super) space: [usize; 2],
    pub(super) href: Option<String>,
}
fn error(value: &str) -> ConsoleError {
    ConsoleError::new(format!("控制台组件暂不支持此终端样式：{value}"))
}
pub(super) fn classes(
    node: &Node,
    styles: Option<&StyleRepository>,
) -> Result<Style, ConsoleError> {
    let mut s = Style {
        element: match node.tag() {
            "ul" => "ul",
            "ol" => "ol",
            "li" => "li",
            "dl" => "dl",
            "dt" => "dt",
            "dd" => "dd",
            "hr" => "hr",
            _ => "generic",
        },
        display: match node.tag() {
            "span" | "strong" | "b" | "i" | "em" | "u" | "s" | "a" | "br" => "inline",
            _ => "block",
        },
        ..Default::default()
    };
    match node.tag() {
        "table" | "code" | "pre" => {
            return Err(error(node.tag()));
        }
        "p" => {
            s.margin[0] = 1;
            s.margin[2] = 1;
        }

        _ => {}
    }
    apply(&mut s, node.attr("class"), node.tag(), 0, styles)?;
    if node.tag() == "b" {
        s.bold = Some(true);
    }
    if node.tag() == "a" && !node.attr("href").is_empty() {
        s.href = Some(node.attr("href").to_owned());
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
pub(super) fn inline(class: &str, styles: Option<&StyleRepository>) -> Result<Style, ConsoleError> {
    let mut style = Style {
        element: "generic",
        display: "inline",
        ..Default::default()
    };
    apply(&mut style, class, "span", 0, styles)?;
    Ok(style)
}
pub(super) fn transform(s: &mut Style, key: &'static str) {
    if !s.transform.contains(&key) {
        s.transform.push(key);
    }
}
fn mark(s: &mut Style, value: &str) {
    if !s.markers.iter().any(|m| m == value) {
        s.markers.push(value.to_owned());
    }
    s.flex = s.markers.iter().any(|m| m == "flex-1");
}
pub(super) fn apply(
    s: &mut Style,
    input: &str,
    tag: &str,
    depth: usize,
    styles: Option<&StyleRepository>,
) -> Result<(), ConsoleError> {
    if depth > 256 {
        return Err(ConsoleError::new("组件样式递归超出范围"));
    }
    let defaults = match tag {
        "p" => "block my-1",
        "ul" => "block list-disc",
        "ol" => "block list-decimal",
        "dt" => "block font-bold",
        "dd" => "block ml-4",
        "hr" => "block border-t",
        "span" | "strong" | "b" | "i" | "em" | "u" | "s" | "a" | "br" => "",
        _ => "block",
    };
    for token in style_tokens::tokens(
        &format!("{defaults} {input}"),
        Terminal::new().get_width(),
        styles.is_some(),
    ) {
        if let Some(registered) = styles.and_then(|repo| repo.lookup(token.original)) {
            let mut formatter = TerminalStyles::new(s, styles, tag, depth);
            registered.invoke(&mut formatter, &[])?;
            continue;
        }
        let Some(resolved) = token.resolved else {
            continue;
        };
        let m = style_methods::resolve(resolved)?;
        mark(s, token.original);
        match m.name.as_str() {
            "block" => s.display = "block",
            "flex" => s.display = "flex",
            "hidden" => s.display = "hidden",
            "flex1" => {}
            "wfull" => s.width = Some(Dimension::Text("1/1".into())),
            "wauto" => s.width = None,
            "w" | "minw" | "maxw" => {
                let value = match m.argument(0)? {
                    Argument::Integer(n) if n.unsigned_abs() <= 1_000_000 => Dimension::Integer(*n),
                    Argument::Text(v) => Dimension::Text((*v).to_owned()),
                    _ => return Err(error(token.original)),
                };
                match m.name.as_str() {
                    "w" => s.width = Some(value),
                    "minw" => s.min = Some(value),
                    _ => s.max = Some(value),
                }
            }
            "uppercase" => transform(s, "upper"),
            "lowercase" => transform(s, "lower"),
            "capitalize" => transform(s, "capitalize"),
            "snakecase" => transform(s, "snake"),
            "prepend" => {
                s.prepend = m.text(0)?.to_owned();
                transform(s, "prepend");
            }
            "append" => {
                s.append = m.text(0)?.to_owned();
                transform(s, "append");
            }
            "truncate" => {
                s.truncate = (
                    m.args.first().map_or(Ok(0), Argument::integer)?,
                    m.args.get(1).map_or(Ok("…"), Argument::text)?.to_owned(),
                );
                transform(s, "truncate");
            }
            "contentrepeat" => {
                static UNQUOTE: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r"\[?'?([^'|\]]+)'?\]?").expect("合法重复内容表达式")
                });
                let value = UNQUOTE.replace_all(m.text(0)?, "$1").into_owned();
                if visible_width(&value) == 0 {
                    return Err(error(token.original));
                }
                s.repeat = Some(value);
                transform(s, "repeat");
            }
            "fontbold" => s.bold = Some(true),
            "fontnormal" => s.bold = Some(false),
            "strong" | "italic" | "underline" | "linethrough" | "invisible" => {
                let code = match m.name.as_str() {
                    "strong" => 1,
                    "italic" => 3,
                    "underline" => 4,
                    "linethrough" => 9,
                    _ => 8,
                };
                if !s.modifiers.contains(&code) {
                    s.modifiers.push(code);
                }
            }
            "justifybetween" => s.justify = Some("between"),
            "justifyaround" => s.justify = Some("around"),
            "justifyevenly" => s.justify = Some("evenly"),
            "justifycenter" => s.justify = Some("center"),
            "text" | "bg" => {
                let value = m.text(0)?;
                let variant = m.args.get(1).map_or(Ok(0), Argument::integer)?;
                if m.name == "text" && matches!(value, "left" | "right" | "center") {
                    s.align = match value {
                        "left" => "left",
                        "right" => "right",
                        _ => "center",
                    };
                } else {
                    let color = palette::color_value(value, variant, styles)?;
                    let (slot, key) = if m.name == "bg" {
                        (&mut s.bg, "bg")
                    } else {
                        (&mut s.fg, "fg")
                    };
                    if slot.is_none() {
                        s.color_order.push(key);
                    }
                    *slot = Some(color);
                }
            }
            "href" => {
                let value = m.text(0)?;
                if !value.is_empty() && value != "0" {
                    s.href = Some(value.to_owned());
                }
            }
            "bordert" => {
                if s.element != "hr" {
                    return Err(ConsoleError::new("分隔线样式只能用于hr元素"));
                }
                if let Some(arg) = m.args.first() {
                    arg.integer()?;
                }
                if !s.modifiers.contains(&0) {
                    s.modifiers.push(0);
                }
            }
            "list" => {
                let kind = m.text(0)?;
                let index = m.args.get(1).map_or(Ok(0), Argument::integer)?;
                if !matches!(s.element, "ul" | "ol" | "li") {
                    return Err(ConsoleError::new("列表样式只能用于ul、ol或li元素"));
                }
                if s.element == "li" {
                    let prefix = match kind {
                        "square" => Some("▪ ".to_owned()),
                        "disc" => Some("• ".to_owned()),
                        "decimal" => Some(format!("{index}. ")),
                        _ => None,
                    };
                    if let Some(prefix) = prefix {
                        s.prepend = prefix;
                        transform(s, "prepend");
                    }
                }
            }
            "setstyle" => mark(s, m.text(0)?),
            "addstyle" => apply(s, m.text(0)?, tag, depth + 1, styles)?,
            "m" | "mt" | "mr" | "mb" | "ml" | "mx" | "my" | "p" | "pt" | "pr" | "pb" | "pl"
            | "px" | "py" | "spacex" | "spacey" => {
                let n = m.integer(0)?;
                let n = usize::try_from(n)
                    .ok()
                    .filter(|n| *n <= 1_000_000)
                    .ok_or_else(|| error(token.original))?;
                if m.name == "spacex" {
                    s.space[0] = n;
                } else if m.name == "spacey" {
                    s.space[1] = n;
                } else {
                    let slot = if m.name.starts_with('m') {
                        &mut s.margin
                    } else {
                        &mut s.padding
                    };
                    match &m.name[1..] {
                        "" => *slot = [n; 4],
                        "x" => {
                            slot[1] = n;
                            slot[3] = n;
                        }
                        "y" => {
                            slot[0] = n;
                            slot[2] = n;
                        }
                        "t" => slot[0] = n,
                        "r" => slot[1] = n,
                        "b" => slot[2] = n,
                        _ => slot[3] = n,
                    }
                }
            }
            _ => return Err(error(token.original)),
        }
    }
    Ok(())
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
            "snake" => {
                static FIRST: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"([a-z0-9])([A-Z])").expect("合法文本边界表达式"));
                static SECOND: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r"([^_])([A-Z][a-z])").expect("合法词首边界表达式")
                });
                Str::lower(&SECOND.replace_all(&FIRST.replace_all(&text, "${1}_${2}"), "${1}_${2}"))
            }
            "prepend" => format!("{}{text}", s.prepend),
            "append" => format!("{text}{}", s.append),
            "truncate" => {
                let width = match &s.width {
                    None => 0,
                    Some(Dimension::Integer(n)) => *n,
                    Some(Dimension::Text(v)) => {
                        dimensions::fraction(v, dimensions::parent_width(parent, viewport)?)?
                            .ok_or_else(|| ConsoleError::new("组件截断宽度无效"))?
                            - s.margin[1] as i64
                            - s.margin[3] as i64
                    }
                };
                let width = width - s.padding[1] as i64 - s.padding[3] as i64;
                let limit = if s.truncate.0 > 0 {
                    s.truncate.0
                } else {
                    width
                };
                if limit == 0 {
                    text
                } else {
                    let limit = limit - visible_width(&s.truncate.1) as i64;
                    if visible_width(&text) as i64 <= limit {
                        text
                    } else {
                        format!("{}{}", trim_text(&text, limit)?, s.truncate.1)
                            .trim_end()
                            .to_owned()
                    }
                }
            }
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
        if *code == 0 {
            let width = visible_width(&text);
            let width = if width == 0 {
                usize::try_from(
                    dimensions::parent_width(parent, viewport)?
                        - s.margin[1] as i64
                        - s.margin[3] as i64,
                )
                .ok()
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(|| ConsoleError::new("分隔线宽度超出范围"))?
            } else {
                width
            };
            text = "─".repeat(width);
        } else {
            text = format!("\x1b[{code}m{text}\x1b[0m");
        }
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
    if let Some(bold) = s.bold {
        tags.push(if bold { "options=bold" } else { "options=," }.to_owned());
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
