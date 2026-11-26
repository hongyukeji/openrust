// 第三方许可见 ../LICENSE.renderer。
use super::{
    style::{self, Style},
    tree::Prepared,
};
use crate::console::ConsoleError;
fn has(style: &Style, name: &str) -> bool {
    style.markers.iter().any(|m| m == name)
}
/// 列表装配时检查直接子元素并追加列表样式；原始文本节点不参与编号。
pub(super) fn prepare_children(
    tag: &str,
    parent: &Style,
    children: &mut [Prepared],
    styles: Option<&super::StyleRepository>,
) -> Result<(), ConsoleError> {
    if !matches!(tag, "ul" | "ol" | "dl") {
        return Ok(());
    }
    let mut index = 0;
    for child in children {
        let Some(own) = &mut child.style else {
            if child.raw_element {
                return Err(ConsoleError::new("列表不接受预格式子元素"));
            }
            continue;
        };
        if tag == "dl" {
            if !matches!(own.element, "dt" | "dd") {
                return Err(ConsoleError::new("定义列表只接受dt和dd子元素"));
            }
            continue;
        }
        if own.element != "li" {
            return Err(ConsoleError::new("列表只接受li子元素"));
        }
        let class = if tag == "ul" {
            if has(own, "list-none") {
                continue;
            }
            if has(parent, "list-none") {
                "list-none"
            } else if has(parent, "list-square") {
                "list-square"
            } else if has(parent, "list-disc") {
                "list-disc"
            } else {
                "list-none"
            }
        } else if has(own, "list-none") || has(parent, "list-none") {
            "list-none"
        } else if has(parent, "list-decimal") {
            index += 1;
            style::apply(own, &format!("list-decimal-{index}"), "li", 0, styles)?;
            continue;
        } else {
            "list-none"
        };
        style::apply(own, class, "li", 0, styles)?;
    }
    Ok(())
}
