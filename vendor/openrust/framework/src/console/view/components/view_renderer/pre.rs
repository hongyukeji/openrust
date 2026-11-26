// 第三方许可见 ../LICENSE.renderer。
use super::{dom::Node, style, tree::Prepared};
use crate::{
    console::{ConsoleError, Terminal},
    support::decode_html_entities,
};
/// XML 保存后实体解码的节点内容；文本与属性的转义在这两个步骤中抵消。
fn inner(nodes: &[Node], id: usize, depth: usize) -> Result<String, ConsoleError> {
    if depth > 256 {
        return Err(ConsoleError::new("控制台组件嵌套超过安全深度"));
    }
    let mut html = String::new();
    for child in &nodes[id].children {
        let node = &nodes[*child];
        if node.comment {
            html.push_str(&format!("<!--{}-->", decode_html_entities(&node.text)));
            continue;
        }
        if node.name.is_none() {
            html.push_str(&node.text);
            continue;
        }
        html.push('<');
        html.push_str(node.tag());
        for attr in &node.attrs {
            html.push_str(&format!(
                " {}=\"{}\"",
                attr.name.local,
                if attr.value.is_empty()
                    && !node
                        .explicit_empty
                        .iter()
                        .any(|name| name == attr.name.local.as_ref())
                    && matches!(
                        attr.name.local.as_ref(),
                        "checked"
                            | "disabled"
                            | "selected"
                            | "readonly"
                            | "multiple"
                            | "ismap"
                            | "nohref"
                            | "noshade"
                            | "nowrap"
                            | "compact"
                            | "declare"
                            | "defer"
                            | "noresize"
                    )
                {
                    attr.name.local.as_ref()
                } else {
                    attr.value.as_ref()
                }
            ));
        }
        if node.children.is_empty() {
            html.push_str("/>");
        } else {
            html.push('>');
            html.push_str(&inner(nodes, *child, depth + 1)?);
            html.push_str("</");
            html.push_str(node.tag());
            html.push('>');
        }
    }
    Ok(html)
}
pub(super) fn prepare(
    nodes: &[Node],
    id: usize,
    first: bool,
    depth: usize,
) -> Result<Prepared, ConsoleError> {
    let html = inner(nodes, id, depth)?;
    let mut lines: Vec<_> = html.split('\n').collect();
    if lines.first() == Some(&"") {
        lines.remove(0);
    }
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let longest = lines.iter().map(|s| s.len()).max().unwrap_or(0);
    let width = longest
        .checked_add(3)
        .filter(|n| *n <= 1_000_000)
        .ok_or_else(|| ConsoleError::new("预格式填充宽度超出范围"))?;
    let mut node = nodes[id].clone();
    node.name.as_mut().expect("预格式元素具有名称").local = "div".into();
    let mut content = String::new();
    for line in lines {
        let style = style::classes(&node)?;
        let line = format!("{line}{}", " ".repeat(width - line.len()));
        let raw = Prepared {
            style: None,
            raw_element: false,
            children: vec![],
            content: Some(line),
            parent: None,
            spacing: [0; 2],
            first: true,
            br: false,
        };
        let mut element = Prepared {
            style: Some(style),
            raw_element: false,
            children: vec![raw],
            content: None,
            parent: None,
            spacing: [0; 2],
            first: false,
            br: false,
        };
        content.push_str(&element.render(Terminal::new().get_width())?);
    }
    Ok(Prepared {
        style: None,
        raw_element: true,
        children: vec![],
        content: Some(content),
        parent: None,
        spacing: [0; 2],
        first,
        br: false,
    })
}
