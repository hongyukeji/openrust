// 第三方许可见 ../LICENSE.renderer。只在已实现的样式范围内渲染，未知样式明确报错。
use super::super::display::{trim_text, visible_width};
use super::dom::{self, Node};
use crate::{
    console::{ConsoleError, Terminal},
    support::Str,
};
use regex::Regex;
use std::sync::LazyLock;
#[derive(Clone, Default)]
struct Style {
    display: &'static str,
    margin: [usize; 4],
    padding: [usize; 4],
    fg: Option<String>,
    bg: Option<String>,
    options: Vec<&'static str>,
    modifiers: Vec<u8>,
    color_order: Vec<&'static str>,
    width: Option<usize>,
    full: bool,
    max: Option<usize>,
    min: Option<usize>,
    align: &'static str,
    transform: Vec<&'static str>,
    repeat: Option<String>,
    flex: bool,
    space: [usize; 2],
    href: Option<String>,
}
fn error(value: &str) -> ConsoleError {
    ConsoleError::new(format!("控制台组件暂不支持此终端样式：{value}"))
}
fn classes(node: &Node) -> Result<Style, ConsoleError> {
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
    for class in node.attr("class").split_whitespace() {
        match class {
            "block" => s.display = "block",
            "flex" => s.display = "flex",
            "hidden" => s.display = "hidden",
            "flex-1" => s.flex = true,
            "w-full" => s.full = true,
            "w-auto" => {
                s.width = None;
                s.full = false;
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
                    if s.fg.is_none() {
                        s.color_order.push("fg");
                    }
                    s.fg = Some(color(value)?);
                    continue;
                }
                if let Some(value) = class.strip_prefix("bg-") {
                    if s.bg.is_none() {
                        s.color_order.push("bg");
                    }
                    s.bg = Some(color(value)?);
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
                    "w" => {
                        s.width = Some(n);
                        s.full = false;
                    }
                    "max-w" => s.max = Some(n),
                    "min-w" => s.min = Some(n),
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
fn color(value: &str) -> Result<String, ConsoleError> {
    match value {
        "black" | "white" | "gray" | "red" | "green" | "blue" | "yellow" | "cyan" | "magenta" => {
            Ok(value.to_owned())
        }
        _ => Err(error(value)),
    }
}
fn normalized(text: &str, first: bool, last: bool) -> String {
    if text == " " {
        return text.to_owned();
    }
    static SPACE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[ \t\n\r\x0b\x0c]+").expect("合法空白表达式"));
    let text = SPACE.replace_all(text, " ");
    if text.trim().is_empty() {
        return String::new();
    }
    let text = if first { text.trim_start() } else { &text };
    let text = if last { text.trim_end() } else { text };
    text.to_owned()
}
// 完整宽度原值是分数字符串；最小宽度1仍取完整宽度，2以上按既定比较取数值。
fn selected_width(s: &Style, width: usize, force: Option<usize>) -> Option<usize> {
    if let Some(fixed) = force.or(s.width) {
        return Some(fixed.max(s.min.unwrap_or(0)));
    }
    if s.full {
        Some(
            s.min
                .filter(|n| *n > 1)
                .unwrap_or_else(|| width.saturating_sub(s.margin[1] + s.margin[3])),
        )
    } else {
        s.min
    }
}
fn style(
    content: String,
    s: &Style,
    first: bool,
    width: usize,
    force: Option<usize>,
) -> Result<String, ConsoleError> {
    if s.display == "hidden" {
        return Ok(String::new());
    }
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
    if let Some(target) = selected_width(s, width, force) {
        if target == 0 {
            text.clear();
        } else {
            let target = s.max.filter(|n| *n > 0).map_or(target, |m| m.min(target));
            let target = i64::try_from(target).map_err(ConsoleError::from_source)?
                - i64::try_from(s.padding[1] + s.padding[3]).map_err(ConsoleError::from_source)?;
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
    let [mt, mr, mb, ml] = s.margin;
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
struct Flow<'a> {
    first: bool,
    last: bool,
    width: usize,
    parent: Option<&'a Style>,
    force: Option<usize>,
    depth: usize,
}
fn render_node(nodes: &[Node], id: usize, flow: Flow<'_>) -> Result<String, ConsoleError> {
    let Flow {
        first,
        last,
        width,
        parent,
        force,
        depth,
    } = flow;
    if depth > 256 {
        return Err(ConsoleError::new("控制台组件嵌套超过安全深度"));
    }
    let n = &nodes[id];
    if n.name.is_none() {
        return Ok(normalized(&n.text, first, last));
    }
    let mut s = classes(n)?;
    if let Some(parent) = parent {
        if s.bg.is_none() && parent.bg.is_some() {
            s.bg = parent.bg.clone();
            s.color_order.push("bg");
        }
        if s.fg.is_none() && parent.fg.is_some() {
            s.fg = parent.fg.clone();
            s.color_order.push("fg");
        }
        if !s.options.contains(&"bold") && parent.options.contains(&"bold") {
            s.options.push("bold");
        }
        if !first {
            if parent.space[0] > 0 {
                s.margin[3] = parent.space[0];
            }
            if parent.space[1] > 0 {
                s.margin[0] = parent.space[1];
            }
        }
    }
    let fixed = force
        .or(s.width)
        .map(|n| n.max(s.min.unwrap_or(0)))
        .or_else(|| s.min.filter(|n| !s.full || *n > 1));
    let inner = fixed
        .filter(|n| *n > 0)
        .map_or(width, |n| n + s.margin[1] + s.margin[3]);
    let inner = s
        .max
        .filter(|n| *n > 0)
        .map_or(inner, |m| inner.min(m))
        .saturating_sub(s.margin[1] + s.margin[3] + s.padding[1] + s.padding[3]);
    let children: Vec<_> = n
        .children
        .iter()
        .copied()
        .filter(|id| {
            nodes[*id].name.is_some() || !normalized(&nodes[*id].text, true, true).is_empty()
        })
        .collect();
    let mut rendered = vec![];
    let mut flexes = vec![];
    for (at, child) in children.iter().enumerate() {
        let value = render_node(
            nodes,
            *child,
            Flow {
                first: at == 0,
                last: at + 1 == children.len(),
                width: inner,
                parent: Some(&s),
                force: None,
                depth: depth + 1,
            },
        )?;
        let child_style = if nodes[*child].name.is_some() {
            Some(classes(&nodes[*child])?)
        } else {
            None
        };
        if s.display == "flex" && child_style.as_ref().is_some_and(|c| c.flex) {
            flexes.push((at, child_style.expect("已确认元素样式")));
        }
        rendered.push(value);
    }
    if !flexes.is_empty() {
        let fixed: usize = rendered
            .iter()
            .enumerate()
            .filter(|(at, _)| !flexes.iter().any(|(f, _)| f == at))
            .map(|(_, v)| visible_width(v))
            .sum();
        let margins: usize = flexes.iter().map(|(_, c)| c.margin[1] + c.margin[3]).sum();
        let available = inner.saturating_sub(fixed + margins);
        for (index, (at, _)) in flexes.iter().enumerate() {
            let allocated = available / flexes.len()
                + if index + 1 == flexes.len() {
                    available % flexes.len()
                } else {
                    0
                };
            rendered[*at] = render_node(
                nodes,
                children[*at],
                Flow {
                    first: *at == 0,
                    last: *at + 1 == children.len(),
                    width: inner,
                    parent: Some(&s),
                    force: Some(allocated),
                    depth: depth + 1,
                },
            )?;
        }
    }
    let mut result = style(rendered.join(""), &s, first, width, force)?;
    if n.tag() == "br" && s.display != "hidden" && s.display != "block" {
        result.push('\r');
    }
    Ok(result)
}
pub(super) fn render(html: &str) -> Result<String, ConsoleError> {
    static TAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[/!a-zA-Z]").expect("合法标签起始表达式"));
    if !TAG.is_match(html) {
        return Ok(html.to_owned());
    }
    let nodes = dom::parse(html.trim());
    let body = nodes
        .iter()
        .position(|n| n.tag() == "body")
        .ok_or_else(|| ConsoleError::new("控制台组件缺少HTML主体"))?;
    let children: Vec<_> = nodes[body]
        .children
        .iter()
        .copied()
        .filter(|id| {
            nodes[*id].name.is_some() || !normalized(&nodes[*id].text, true, true).is_empty()
        })
        .collect();
    // 默认实现先构造全部兄弟元素，最后只写第一个；后面的无效样式也必须报错。
    let mut pending: Vec<_> = children.iter().map(|id| (*id, 0usize)).collect();
    while let Some((id, depth)) = pending.pop() {
        if depth > 256 {
            return Err(ConsoleError::new("控制台组件嵌套超过安全深度"));
        }
        if nodes[id].name.is_some() {
            classes(&nodes[id])?;
        }
        pending.extend(nodes[id].children.iter().map(|id| (*id, depth + 1)));
    }
    let first = *children
        .first()
        .ok_or_else(|| ConsoleError::new("控制台组件HTML没有可渲染的首元素"))?;
    let width = usize::try_from(Terminal::new().get_width()).map_err(ConsoleError::from_source)?;
    if width > 1_000_000 {
        return Err(ConsoleError::new("组件终端宽度超出安全范围"));
    }
    render_node(
        &nodes,
        first,
        Flow {
            first: true,
            last: children.len() == 1,
            width,
            parent: None,
            force: None,
            depth: 0,
        },
    )
}
