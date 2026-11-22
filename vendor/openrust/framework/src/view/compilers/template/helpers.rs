use serde::{Deserialize, Serialize};

use super::{
    Expression, ViewData, ViewResult, ViewValue, expressions, invalid, render_error, separators,
};
use crate::support::Arr;

#[derive(Serialize, Deserialize)]
enum Operation {
    Query,
    Except,
    InArray,
}

/// 仅开放已实现的通用数组函数，不执行任意 PHP／Rust 调用。
#[derive(Serialize, Deserialize)]
pub(super) struct Helper {
    operation: Operation,
    arguments: Vec<Expression>,
}

impl Helper {
    pub(super) fn parse(source: &str, depth: usize) -> ViewResult<Option<Self>> {
        for (name, operation, minimum, maximum) in [
            ("Arr::query", Operation::Query, 1, 1),
            ("Arr::except", Operation::Except, 2, 2),
            ("in_array", Operation::InArray, 2, 3),
        ] {
            let Some(source) = source
                .strip_prefix(name)
                .and_then(|rest| rest.trim_start().strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                continue;
            };
            let mut start = 0;
            let mut arguments = Vec::new();
            for end in separators(source, ",").chain(std::iter::once(source.len())) {
                let argument = source[start..end].trim();
                if !argument.is_empty() || end != source.len() {
                    arguments.push(expressions::parse_expression(argument, depth + 1)?);
                }
                start = end + 1;
            }
            if !(minimum..=maximum).contains(&arguments.len()) {
                return Err(invalid("数组函数的参数数量无效。"));
            }
            return Ok(Some(Self {
                operation,
                arguments,
            }));
        }
        Ok(None)
    }

    pub(super) fn evaluate(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewValue> {
        let arguments = self
            .arguments
            .iter()
            .map(|argument| expressions::value(argument, data, bindings))
            .collect::<ViewResult<Vec<_>>>()?;
        let array = |index: usize| -> ViewResult<serde_json::Value> {
            let value = arguments[index].as_ref();
            if !matches!(value, ViewValue::Array(_) | ViewValue::Object(_)) {
                return Err(render_error("数组函数需要数组参数。"));
            }
            value
                .to_json()
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
        };
        match self.operation {
            Operation::Query => Arr::query(&array(0)?)
                .map(ViewValue::String)
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source))),
            Operation::Except => {
                let keys = match arguments[1].as_ref() {
                    ViewValue::String(key) => vec![key.as_str()],
                    ViewValue::Array(keys) => keys
                        .iter()
                        .map(|key| {
                            key.as_str()
                                .ok_or_else(|| render_error("数组键必须是字符串。"))
                        })
                        .collect::<ViewResult<Vec<_>>>()?,
                    _ => return Err(render_error("排除的数组键必须是字符串或字符串数组。")),
                };
                Arr::except(&array(0)?, &keys)
                    .map(ViewValue::from)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
            }
            Operation::InArray => {
                let strict = match arguments.get(2).map(|value| value.as_ref()) {
                    None => false,
                    Some(ViewValue::Bool(strict)) => *strict,
                    _ => return Err(render_error("in_array 的 strict 参数必须是布尔值。")),
                };
                let contains = |item: &ViewValue| {
                    if strict {
                        expressions::strict_equal(arguments[0].as_ref(), item)
                    } else {
                        expressions::loose_equal(arguments[0].as_ref(), item)
                    }
                };
                let found = match arguments[1].as_ref() {
                    ViewValue::Array(items) => items.iter().any(contains),
                    ViewValue::Object(items) => items.values().any(contains),
                    _ => return Err(render_error("in_array 的候选必须是数组。")),
                };
                Ok(ViewValue::Bool(found))
            }
        }
    }
}
