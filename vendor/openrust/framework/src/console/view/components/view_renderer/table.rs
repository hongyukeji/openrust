// 第三方许可见 ../LICENSE.renderer。
use super::super::display::styling;
use super::{dom::Node, layout, pre, style, tree::Prepared};
use crate::{
    console::{
        ConsoleError, Output, Table, TableCell, TableCellStyle, TableRow, TableSeparator, Terminal,
    },
    support::input_scalars,
};
use regex::Regex;
use serde_json::json;
use std::sync::LazyLock;
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
}
fn format(text: String, style: &style::Style) -> Result<String, ConsoleError> {
    style::format(
        text,
        style,
        &None,
        Terminal::new().get_width(),
        false,
        [0; 2],
    )
}
fn cell_style(style: &style::Style, align: &str) -> Result<TableCellStyle, ConsoleError> {
    let mut tags = vec![];
    for key in &style.color_order {
        let value = if *key == "fg" { &style.fg } else { &style.bg };
        if let Some(value) = value {
            tags.push(format!("{key}={value}"));
        }
    }
    let template = if tags.is_empty() {
        "%s".to_owned()
    } else {
        format!("<{}>%s</>", tags.join(";"))
    };
    TableCellStyle::new([
        (
            "align",
            json!(if align.is_empty() { "left" } else { align }),
        ),
        ("cellFormat", json!(template)),
    ])
}
fn rows(nodes: &[Node], id: usize, depth: usize) -> Result<Vec<TableRow>, ConsoleError> {
    let mut row = vec![];
    for child in &nodes[id].children {
        let node = &nodes[*child];
        if !matches!(node.tag(), "th" | "td") {
            continue;
        }
        let mut class = node.attr("class").to_owned();
        if node.tag() == "th" {
            class.push_str(" strong");
        }
        static BREAK: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"<br\s?/?>").expect("合法单元格换行表达式"));
        let html = pre::inner(nodes, *child, depth + 1)?;
        let html = BREAK.replace_all(&html, "\n");
        let text = layout::render(trim(&html))?;
        if styling().is_match(&text) {
            class.push_str(" font-normal");
        }
        let styles = style::inline(&class)?;
        let content = format(text, &styles)?;
        row.push(
            TableCell::new(content)
                .with_colspan(input_scalars::integer_string(node.attr("colspan")).max(1))
                .with_rowspan(input_scalars::integer_string(node.attr("rowspan")).max(1))
                .with_style(cell_style(&styles, node.attr("align"))?),
        );
    }
    let mut result = vec![];
    if !row.is_empty() {
        result.push(TableRow::Cells(row));
    }
    let border = input_scalars::integer_string(nodes[id].attr("border"));
    if !(0..=1_000_000).contains(&border) {
        return Err(ConsoleError::new("表格分隔线数量超出范围"));
    }
    result.extend((0..border).map(|_| TableRow::Separator(TableSeparator::new())));
    Ok(result)
}
fn title(nodes: &[Node], id: usize) -> Result<String, ConsoleError> {
    format(" %s ".to_owned(), &style::inline(nodes[id].attr("class"))?)
}
pub(super) fn prepare(
    nodes: &[Node],
    id: usize,
    first: bool,
    depth: usize,
) -> Result<Prepared, ConsoleError> {
    let mut output = Output::default();
    output.set_decorated(true);
    {
        let mut table = Table::new(&mut output)?;
        if !nodes[id].attr("style").is_empty() {
            table.set_style_named(nodes[id].attr("style"))?;
        }
        // HTML 标题样式属于本次渲染，不写回长驻应用共享的命名定义。
        let owned_style = table.get_style().clone();
        table.set_style(owned_style);
        for child in &nodes[id].children {
            let node = &nodes[*child];
            match node.tag() {
                "thead" | "tfoot" => {
                    let header = node.tag() == "thead";
                    if !node.attr("title").is_empty() {
                        if header {
                            table
                                .get_style_mut()
                                .set_header_title_format(title(nodes, *child)?);
                            table.set_header_title(Some(node.attr("title")));
                        } else {
                            table
                                .get_style_mut()
                                .set_footer_title_format(title(nodes, *child)?);
                            table.set_footer_title(Some(node.attr("title")));
                        }
                    }
                    for tr in &node.children {
                        if nodes[*tr].tag() != "tr" {
                            continue;
                        }
                        let values = rows(nodes, *tr, depth + 1)?;
                        if header {
                            for row in values {
                                if let TableRow::Cells(cells) = row {
                                    table.set_headers(cells);
                                }
                            }
                        } else if !values.is_empty() {
                            table.add_row(TableSeparator::new())?;
                            table.add_rows(values)?;
                        }
                    }
                }
                "tbody" => {
                    for tr in &node.children {
                        if nodes[*tr].tag() == "tr" {
                            table.add_rows(rows(nodes, *tr, depth + 1)?)?;
                        }
                    }
                }
                _ => {
                    table.add_rows(rows(nodes, *child, depth + 1)?)?;
                }
            }
        }
        table.render()?;
    }
    let content = output
        .stdout
        .strip_suffix('\n')
        .unwrap_or(&output.stdout)
        .to_owned();
    // 表格结果回到普通 div，按外层原始上下文继续布局。
    let mut node = nodes[id].clone();
    node.name.as_mut().expect("表格具有名称").local = "div".into();
    node.attrs.clear();
    let own = style::classes(&node)?;
    Ok(Prepared {
        style: Some(own),
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
        first,
        br: false,
    })
}
