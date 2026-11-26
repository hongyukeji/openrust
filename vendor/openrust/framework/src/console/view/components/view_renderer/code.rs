// 第三方许可见 ../LICENSE.renderer。
mod tokens;
use super::{StyleRepository, dom::Node, pre, style, tree::Prepared};
use crate::{
    console::{ConsoleError, Terminal},
    support::input_scalars,
};
fn paint(
    text: &str,
    class: &str,
    styles: Option<&StyleRepository>,
) -> Result<String, ConsoleError> {
    style::format(
        text.to_owned(),
        &style::inline(class, styles)?,
        &None,
        Terminal::new().get_width(),
        false,
        [0; 2],
    )
}
pub(super) fn prepare(
    nodes: &[Node],
    id: usize,
    depth: usize,
    styles: Option<&StyleRepository>,
) -> Result<Prepared, ConsoleError> {
    let node = &nodes[id];
    let mut source = pre::inner(nodes, id, depth)?;
    let extra = source
        .split('\n')
        .find(|line| {
            !line.is_empty()
                && line
                    .bytes()
                    .all(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c'))
        })
        .map(str::to_owned);
    if let Some(extra) = extra {
        source = source
            .split('\n')
            .map(|line| line.strip_prefix(&extra).unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
    }
    source = source
        .trim_matches('\n')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let start = input_scalars::integer_string(node.attr("start-line")).max(1);
    let mark = input_scalars::integer_string(node.attr("line")).max(0);
    let mut lines: Vec<Vec<(tokens::Category, String)>> = vec![vec![]];
    for (kind, text) in tokens::groups(&source)? {
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                lines.push(vec![]);
            }
            if !part.is_empty() {
                lines
                    .last_mut()
                    .expect("代码至少一行")
                    .push((kind, part.to_owned()));
            }
        }
    }
    use crate::support::NumberValue;
    let plus_one = |n| match n {
        NumberValue::Integer(i) => i
            .checked_add(1)
            .map(NumberValue::Integer)
            .unwrap_or(NumberValue::Float(i as f64 + 1.0)),
        NumberValue::Float(f) => NumberValue::Float(f + 1.0),
    };
    let text = |n| match n {
        NumberValue::Integer(i) => i.to_string(),
        NumberValue::Float(f) => input_scalars::double_string(f),
    };
    let mut number = NumberValue::Integer(start - 1);
    let mut indexed = indexmap::IndexMap::new();
    for line in lines {
        let key = match number {
            NumberValue::Integer(i) => i,
            NumberValue::Float(_) => return Err(ConsoleError::new("代码行键超出整数范围")),
        };
        indexed.insert(key, line);
        number = plus_one(number);
    }
    let last = *indexed.last().expect("代码至少一行").0;
    let width = text(plus_one(NumberValue::Integer(last))).len().max(3);
    let mut content = String::new();
    for (key, line) in indexed {
        let number = plus_one(NumberValue::Integer(key));
        let marked = matches!(number,NumberValue::Integer(n)if n==mark);
        if mark != 0 {
            if marked {
                content.push_str(&paint("  ➜ ", "text-red strong", styles)?);
            } else {
                content.push_str("    ");
            }
        }
        content.push_str(&paint(
            &format!("{:>width$}", text(number)),
            if marked { "italic strong" } else { "text-gray" },
            styles,
        )?);
        content.push_str(&paint("▕ ", "text-gray", styles)?);
        for (kind, text) in line {
            content.push_str(&paint(&text, kind.class(), styles)?);
        }
        content.push('\n');
    }
    let content = content.trim_matches('\n').to_owned();
    let mut node = node.clone();
    node.name.as_mut().expect("代码元素具有名称").local = "div".into();
    node.attrs.clear();
    Ok(Prepared {
        style: Some(style::classes(&node, styles)?),
        raw_element: false,
        children: vec![Prepared {
            style: None,
            raw_element: false,
            children: vec![],
            content: Some(content),
            parent: None,
            spacing: [0; 2],
            first: true,
            br: false,
        }],
        content: None,
        parent: None,
        spacing: [0; 2],
        first: false,
        br: false,
    })
}
