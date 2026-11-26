// 第三方许可见 ../LICENSE.renderer。
use crate::{console::ConsoleError, support::input_scalars};
use regex::Regex;
use std::{
    cmp::Ordering,
    sync::{Arc, LazyLock},
};
/// 数字类参数在入口转整数；文本保留非数值宽度表达式及其比较顺序。
#[derive(Clone, Debug)]
pub(super) enum Dimension {
    Integer(i64),
    Text(String),
}
impl Dimension {
    pub fn parse(value: &str) -> Result<Self, ConsoleError> {
        let value = value
            .trim_start_matches('-')
            .split('-')
            .next()
            .unwrap_or("");
        if value.is_empty() {
            return Err(ConsoleError::new("组件尺寸缺少参数"));
        }
        if input_scalars::numeric_prefix(value) == Some(value) {
            let n = input_scalars::integer_string(value);
            if n.unsigned_abs() > 1_000_000 {
                return Err(ConsoleError::new("组件尺寸超出安全范围"));
            }
            Ok(Self::Integer(n))
        } else {
            Ok(Self::Text(value.to_owned()))
        }
    }
    pub fn integer(&self) -> i64 {
        match self {
            Self::Integer(n) => *n,
            Self::Text(value) => input_scalars::integer_string(value),
        }
    }
    pub fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Integer(a), Self::Text(b)) => a.to_string().cmp(b),
            (Self::Text(a), Self::Integer(b)) => a.cmp(&b.to_string()),
        }
    }
    fn maximum(self, other: Self) -> Self {
        if self.compare(&other).is_lt() {
            other
        } else {
            self
        }
    }
    fn minimum(self, other: Self) -> Self {
        if self.compare(&other).is_gt() {
            other
        } else {
            self
        }
    }
}
#[derive(Clone)]
pub(super) struct Dimensions {
    pub width: Option<Dimension>,
    pub min: Option<Dimension>,
    pub max: Option<Dimension>,
    pub margin: i64,
    pub padding: i64,
}
pub(super) struct Constraint {
    pub parent: Option<Arc<Self>>,
    pub dimensions: Dimensions,
}
fn overflow() -> ConsoleError {
    ConsoleError::new("组件尺寸运算超出范围")
}
fn fraction(value: &str, width: i64) -> Result<Option<i64>, ConsoleError> {
    static FRACTION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"([0-9]+)/([0-9]+)").expect("合法分数宽度表达式"));
    let Some(parts) = FRACTION.captures(value) else {
        return Ok(None);
    };
    let numerator = &parts[1];
    let denominator = &parts[2];
    let divisor = input_scalars::decimal_float(denominator).ok_or_else(overflow)?;
    if divisor == 0.0 {
        return Err(ConsoleError::new("组件分数宽度的分母不能为零"));
    }
    let product = numerator
        .parse::<i64>()
        .ok()
        .and_then(|n| width.checked_mul(n));
    let product = product
        .map_or_else(
            || input_scalars::decimal_float(numerator).map(|n| width as f64 * n),
            |n| Some(n as f64),
        )
        .ok_or_else(overflow)?;
    Ok(Some(input_scalars::double_integer(
        (product / divisor).floor(),
    )))
}
pub(super) fn parent_width(
    parent: &Option<Arc<Constraint>>,
    viewport: i64,
) -> Result<i64, ConsoleError> {
    let mut chain = vec![];
    let mut current = parent.as_deref();
    while let Some(node) = current {
        chain.push(&node.dimensions);
        current = node.parent.as_deref();
    }
    let mut width = viewport;
    for d in chain.into_iter().rev() {
        let mut value =
            d.width
                .clone()
                .unwrap_or(Dimension::Integer(0))
                .maximum(Dimension::Integer(
                    d.min.as_ref().map_or(0, Dimension::integer),
                ));
        if value.compare(&Dimension::Integer(1)).is_lt() {
            value = Dimension::Integer(width);
        } else if let Dimension::Integer(n) = value {
            value = Dimension::Integer(n.checked_add(d.margin).ok_or_else(overflow)?);
        }
        width = match &value {
            Dimension::Text(text) => fraction(text, width)?.unwrap_or_else(|| value.integer()),
            Dimension::Integer(n) => *n,
        };
        let max = d.max.as_ref().map_or(0, Dimension::integer);
        if max > 0 {
            width = width.min(max);
        }
        width = width
            .checked_sub(d.margin)
            .and_then(|n| n.checked_sub(d.padding))
            .ok_or_else(overflow)?;
    }
    Ok(width)
}
pub(super) enum Target {
    Unchanged,
    Empty,
    Columns(i64),
}
pub(super) fn target(
    d: &Dimensions,
    parent: &Option<Arc<Constraint>>,
    viewport: i64,
) -> Result<Target, ConsoleError> {
    let value = d
        .width
        .clone()
        .unwrap_or(Dimension::Integer(-1))
        .maximum(d.min.clone().unwrap_or(Dimension::Integer(-1)));
    if value.compare(&Dimension::Integer(0)).is_lt() {
        return Ok(Target::Unchanged);
    }
    if matches!(value, Dimension::Integer(0)) {
        return Ok(Target::Empty);
    }
    let width = match value {
        Dimension::Integer(n) => n,
        Dimension::Text(text) => fraction(&text, parent_width(parent, viewport)?)?
            .ok_or_else(|| ConsoleError::new(format!("组件分数宽度无效：{text}")))?
            .checked_sub(d.margin)
            .ok_or_else(overflow)?,
    };
    let mut value = Dimension::Integer(width);
    if let Some(max) = &d.max
        && max.compare(&Dimension::Integer(0)).is_gt()
    {
        value = max.clone().minimum(value);
    }
    let width = match value {
        Dimension::Integer(n) => n,
        Dimension::Text(_) => return Err(ConsoleError::new("组件最大宽度不能参与数值运算")),
    };
    Ok(Target::Columns(
        width.checked_sub(d.padding).ok_or_else(overflow)?,
    ))
}
