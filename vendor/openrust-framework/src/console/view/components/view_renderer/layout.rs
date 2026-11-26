// 第三方许可见 ../LICENSE.renderer。
use super::{
    StyleRepository, code,
    dom::{self, Node},
    lists, pre, style, table,
    tree::Prepared,
};
use crate::{
    console::{ConsoleError, Terminal},
    support::Str,
};
use regex::Regex;
use std::sync::LazyLock;
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

fn meaningful(node: &Node) -> bool {
    node.name.is_some()
        || (!node.comment
            && !node
                .text
                .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
                .is_empty())
}
fn boundaries(nodes: &[Node], children: &[usize]) -> (Option<usize>, Option<usize>) {
    (
        children.iter().position(|id| meaningful(&nodes[*id])),
        children.iter().rposition(|id| meaningful(&nodes[*id])),
    )
}
fn prepare(
    nodes: &[Node],
    id: usize,
    first: bool,
    last: bool,
    depth: usize,
    styles: Option<&StyleRepository>,
) -> Result<Prepared, ConsoleError> {
    if depth > 256 {
        return Err(ConsoleError::new("控制台组件嵌套超过安全深度"));
    }
    let node = &nodes[id];
    if node.name.is_none() {
        return Ok(Prepared {
            style: None,
            raw_element: false,
            children: vec![],
            content: Some(if node.comment {
                String::new()
            } else {
                normalized(&node.text, first, last)
            }),
            parent: None,
            spacing: [0; 2],
            first,
            br: false,
        });
    }
    if node.tag() == "pre" {
        return pre::prepare(nodes, id, first, depth, styles);
    }
    if node.tag() == "table" {
        return table::prepare(nodes, id, first, depth, styles);
    }
    if node.tag() == "code" {
        return code::prepare(nodes, id, depth, styles);
    }
    let mut children = vec![];
    let (first_meaningful, last_meaningful) = boundaries(nodes, &node.children);
    for (index, id) in node.children.iter().enumerate() {
        let first = first_meaningful.is_none_or(|first| first >= index);
        let last = last_meaningful.is_none_or(|last| last <= index);
        let child = prepare(nodes, *id, first, last, depth + 1, styles)?;
        if child.style.is_some()
            || child.raw_element
            || child.content.as_ref().is_some_and(|text| !text.is_empty())
        {
            children.push(child);
        }
    }
    // 转换子节点后构造当前元素，与默认布局的构造和错误顺序一致。
    let style = style::classes(node, styles)?;
    lists::prepare_children(node.tag(), &style, &mut children, styles)?;
    Ok(Prepared {
        style: Some(style),
        raw_element: false,
        children,
        content: None,
        parent: None,
        spacing: [0; 2],
        first,
        br: node.tag() == "br",
    })
}
pub(super) fn render(html: &str, styles: Option<&StyleRepository>) -> Result<String, ConsoleError> {
    if !html.contains('<') || Str::of(html).strip_tags(&[]).value() == html {
        return Ok(html.to_owned());
    }
    let html = html.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}']);
    let nodes = dom::parse(html);
    let children = &nodes[0].children;
    let (first_meaningful, last_meaningful) = boundaries(&nodes, children);
    let mut first_child = None;
    for (index, id) in children.iter().enumerate() {
        let first = first_meaningful.is_none_or(|first| first >= index);
        let last = last_meaningful.is_none_or(|last| last <= index);
        let child = prepare(&nodes, *id, first, last, 0, styles)?;
        if index == 0
            && (child.style.is_some()
                || child.raw_element
                || child.content.as_ref().is_some_and(|text| !text.is_empty()))
        {
            first_child = Some(child);
        }
    }
    let mut first =
        first_child.ok_or_else(|| ConsoleError::new("控制台组件HTML没有可渲染的首元素"))?;
    let viewport = Terminal::new().get_width();
    if !(0..=1_000_000).contains(&viewport) {
        return Err(ConsoleError::new("组件终端宽度超出安全范围"));
    }
    first.render(viewport)
}
