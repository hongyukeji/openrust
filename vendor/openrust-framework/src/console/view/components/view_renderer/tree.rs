// 第三方许可见 ../LICENSE.renderer。
use super::super::display::visible_width;
use super::{
    dimensions::{self, Constraint, Dimension},
    style::{self, Style},
};
use crate::{console::ConsoleError, support::input_scalars};
use std::sync::Arc;
/// 缓存子节点拼接结果；外层宽度变化只重新格式化该内容，不重算后代。
pub(super) struct Prepared {
    pub style: Option<Style>,
    pub raw_element: bool,
    pub children: Vec<Self>,
    pub content: Option<String>,
    pub parent: Option<Arc<Constraint>>,
    pub spacing: [usize; 2],
    pub first: bool,
    pub br: bool,
}
fn length(value: &str) -> i64 {
    i64::try_from(visible_width(value)).unwrap_or(i64::MAX)
}
fn spaces(n: f64) -> Result<String, ConsoleError> {
    if !n.is_finite() || !(0.0..=1_000_000.0).contains(&n) {
        return Err(ConsoleError::new("组件填充宽度超出安全范围"));
    }
    Ok(" ".repeat(n as usize))
}
impl Prepared {
    pub fn inherit(&mut self, parent: &Style, constraint: Arc<Constraint>) {
        self.parent = Some(constraint);
        self.spacing = parent.space;
        if let Some(style) = &mut self.style {
            if style.bg.is_none() && parent.bg.is_some() {
                style.bg = parent.bg.clone();
                style.color_order.push("bg");
            }
            if style.fg.is_none() && parent.fg.is_some() {
                style.fg = parent.fg.clone();
                style.color_order.push("fg");
            }
            if style.bold.is_none()
                && let Some(bold) = parent.bold
            {
                style::font_bold(style, bold);
            }
        }
    }
    fn render_children(&mut self, viewport: i64) -> Result<Vec<String>, ConsoleError> {
        self.children
            .iter_mut()
            .map(|child| child.render(viewport))
            .collect()
    }
    fn join_children(&mut self, viewport: i64) -> Result<String, ConsoleError> {
        let own = self.style.as_ref().expect("元素具有样式");
        let constraint = Arc::new(Constraint {
            parent: self.parent.clone(),
            dimensions: own.dimensions(),
        });
        for child in &mut self.children {
            child.inherit(own, constraint.clone());
        }
        let mut values = self.render_children(viewport)?;
        if self.style.as_ref().expect("元素具有样式").display == "flex" {
            if values.is_empty() {
                return Err(ConsoleError::new("弹性布局没有可计算宽度的子元素"));
            }
            let parent_width = dimensions::parent_width(&Some(constraint.clone()), viewport)?;
            let total: i128 = values.iter().map(|value| i128::from(length(value))).sum();
            let flexes: Vec<_> = self
                .children
                .iter()
                .enumerate()
                .filter_map(|(index, child)| {
                    child
                        .style
                        .as_ref()
                        .filter(|style| style.flex)
                        .map(|_| index)
                })
                .collect();
            let mut inner = 0i128;
            for index in &flexes {
                let child = &self.children[*index];
                let margins = child
                    .style
                    .as_ref()
                    .expect("弹性节点具有样式")
                    .margins(child.first, child.spacing);
                inner +=
                    i128::from(length(&values[*index])) - margins[1] as i128 - margins[3] as i128;
            }
            let available = (i128::from(parent_width) - total + inner).max(0);
            for (index, child_index) in flexes.iter().enumerate() {
                let child = self.children[*child_index]
                    .style
                    .as_mut()
                    .expect("弹性节点具有样式");
                if available == 0 && child.repeat.is_none() {
                    continue;
                }
                let unit = available as f64 / flexes.len() as f64;
                let allocated = unit.floor()
                    + if index + 1 == flexes.len() {
                        (unit - unit.floor()) * flexes.len() as f64
                    } else {
                        0.0
                    };
                // 生成宽度类的浮点插值经过既定文本转换，再由样式参数转回整数。
                let n = input_scalars::integer_string(&input_scalars::double_string(allocated));
                child.width = Some(Dimension::Integer(n));
            }
            values = self.render_children(viewport)?;
        }
        let Some(justify) = self.style.as_ref().expect("元素具有样式").justify else {
            return Ok(values.join(""));
        };
        if (justify == "between" && values.len() <= 1) || (justify == "around" && values.is_empty())
        {
            return Ok(values.join(""));
        }
        if values.is_empty() {
            return Err(ConsoleError::new("对齐布局没有可计算宽度的子元素"));
        }
        let parent_width = dimensions::parent_width(&Some(constraint), viewport)? as f64;
        let total = values.iter().map(|value| length(value) as f64).sum::<f64>();
        let count = values.len() as f64;
        let available = parent_width - total;
        let space = available
            / match justify {
                "between" => count - 1.0,
                "evenly" => count + 1.0,
                "around" => count,
                _ => 1.0,
            };
        if space < 1.0 {
            return Ok(values.join(""));
        }
        match justify {
            "between" => {
                let mut out = String::new();
                let last = values.len() - 1;
                for (index, value) in values.into_iter().enumerate() {
                    if index > 0 {
                        out.push_str(&spaces(if index == last {
                            space.ceil()
                        } else {
                            space.floor()
                        })?);
                    }
                    out.push_str(&value);
                }
                Ok(out)
            }
            "evenly" => {
                let mut out = String::new();
                for value in values {
                    out.push_str(&spaces(space.floor())?);
                    out.push_str(&value);
                }
                out.push_str(&spaces(
                    space.floor() + ((space - space.floor()) * (count + 1.0)).ceil(),
                )?);
                Ok(out)
            }
            "around" => {
                let mut out = String::new();
                let mut used = total;
                for (index, value) in values.into_iter().enumerate() {
                    if index > 0 {
                        out.push_str(&spaces(space.ceil())?);
                        used += space.ceil();
                    }
                    out.push_str(&value);
                }
                let outside = (parent_width - used) / 2.0;
                Ok(format!(
                    "{}{out}{}",
                    spaces(outside.floor())?,
                    spaces(outside.ceil())?
                ))
            }
            _ => Ok(format!(
                "{}{}{}",
                spaces((space / 2.0).floor())?,
                values.join(""),
                spaces((space / 2.0).ceil())?
            )),
        }
    }
    pub fn render(&mut self, viewport: i64) -> Result<String, ConsoleError> {
        if self.style.is_none() {
            return Ok(self.content.clone().expect("文本节点已经规范化"));
        }
        if self.content.is_none() {
            self.content = Some(self.join_children(viewport)?);
        }
        let own = self.style.as_ref().expect("元素具有样式");
        let mut result = style::format(
            self.content.clone().expect("已拼接元素内容"),
            own,
            &self.parent,
            viewport,
            self.first,
            self.spacing,
        )?;
        if self.br && own.display != "hidden" && own.display != "block" {
            result.push('\r');
        }
        Ok(result)
    }
}
