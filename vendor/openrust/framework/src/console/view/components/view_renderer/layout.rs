// 第三方许可见 ../LICENSE.renderer。
use super::{
    dom::{self, Node},
    lists, style,
    tree::Prepared,
};
use crate::console::{ConsoleError, Terminal};
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
        || !node
            .text
            .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
            .is_empty()
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
) -> Result<Prepared, ConsoleError> {
    if depth > 256 {
        return Err(ConsoleError::new("控制台组件嵌套超过安全深度"));
    }
    let node = &nodes[id];
    if node.name.is_none() {
        return Ok(Prepared {
            style: None,
            children: vec![],
            content: Some(normalized(&node.text, first, last)),
            parent: None,
            spacing: [0; 2],
            first,
            br: false,
        });
    }
    let mut children = vec![];
    let (first_meaningful, last_meaningful) = boundaries(nodes, &node.children);
    for (index, id) in node.children.iter().enumerate() {
        let first = first_meaningful.is_none_or(|first| first >= index);
        let last = last_meaningful.is_none_or(|last| last <= index);
        let child = prepare(nodes, *id, first, last, depth + 1)?;
        if child.style.is_some() || child.content.as_ref().is_some_and(|text| !text.is_empty()) {
            children.push(child);
        }
    }
    // 转换子节点后构造当前元素，与默认布局的构造和错误顺序一致。
    let style = style::classes(node)?;
    lists::prepare_children(node.tag(), &style, &mut children)?;
    Ok(Prepared {
        style: Some(style),
        children,
        content: None,
        parent: None,
        spacing: [0; 2],
        first,
        br: node.tag() == "br",
    })
}
pub(super) fn render(html: &str) -> Result<String, ConsoleError> {
    static TAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[/!a-zA-Z]").expect("合法标签起始表达式"));
    if !TAG.is_match(html) {
        return Ok(html.to_owned());
    }
    let mut nodes = dom::parse(html.trim());
    dom::recover_lists(&mut nodes);
    let body = nodes
        .iter()
        .position(|n| n.tag() == "body")
        .ok_or_else(|| ConsoleError::new("控制台组件缺少HTML主体"))?;
    let mut children = vec![];
    let (first_meaningful, last_meaningful) = boundaries(&nodes, &nodes[body].children);
    for (index, id) in nodes[body].children.iter().enumerate() {
        let first = first_meaningful.is_none_or(|first| first >= index);
        let last = last_meaningful.is_none_or(|last| last <= index);
        let child = prepare(&nodes, *id, first, last, 0)?;
        if child.style.is_some() || child.content.as_ref().is_some_and(|text| !text.is_empty()) {
            children.push(child);
        }
    }
    let first = children
        .first_mut()
        .ok_or_else(|| ConsoleError::new("控制台组件HTML没有可渲染的首元素"))?;
    let viewport = Terminal::new().get_width();
    if !(0..=1_000_000).contains(&viewport) {
        return Err(ConsoleError::new("组件终端宽度超出安全范围"));
    }
    first.render(viewport)
}
