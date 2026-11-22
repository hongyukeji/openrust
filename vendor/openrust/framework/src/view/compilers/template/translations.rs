use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Expression, ViewData, ViewResult, ViewValue, invalid, separators};
use crate::{
    support::facades::Lang,
    translation::PluralCount,
    view::{ViewError, rendering::render_error},
};

#[derive(Serialize, Deserialize)]
pub(super) struct Translation {
    choice: bool,
    arguments: Vec<Expression>,
}

impl Translation {
    pub(super) fn parse(source: &str, depth: usize) -> ViewResult<Option<Self>> {
        for name in ["__", "trans", "trans_choice"] {
            let Some(arguments) = source
                .strip_prefix(name)
                .and_then(|rest| rest.trim_start().strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                continue;
            };
            return Self::arguments(arguments, name == "trans_choice", depth).map(Some);
        }
        Ok(None)
    }

    pub(super) fn arguments(source: &str, choice: bool, depth: usize) -> ViewResult<Self> {
        let mut start = 0;
        let mut arguments = Vec::new();
        for end in separators(source, ",").chain(std::iter::once(source.len())) {
            let argument = source[start..end].trim();
            // PHP 参数列表允许末尾逗号，空的中间参数仍然报错。
            if !argument.is_empty() || end != source.len() {
                arguments.push(super::expressions::parse_expression(argument, depth + 1)?);
            }
            start = end + 1;
        }
        let minimum = if choice { 2 } else { 1 };
        if !(minimum..=minimum + 2).contains(&arguments.len()) {
            return Err(invalid("翻译函数的参数数量无效。"));
        }
        Ok(Self { choice, arguments })
    }

    pub(super) fn evaluate(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewValue> {
        let arguments = self
            .arguments
            .iter()
            .map(|argument| {
                super::expressions::value(argument, data, bindings).map(|value| value.into_owned())
            })
            .collect::<ViewResult<Vec<_>>>()?;
        let key = arguments[0]
            .as_str()
            .ok_or_else(|| render_error("翻译键必须是字符串。"))?;
        let replace_index = if self.choice { 2 } else { 1 };
        let replacements = arguments
            .get(replace_index)
            .map(replacements)
            .transpose()?
            .unwrap_or_default();
        let locale = match arguments.get(replace_index + 1) {
            None | Some(ViewValue::Null) => None,
            Some(ViewValue::String(locale)) => Some(locale.as_str()),
            _ => return Err(render_error("翻译语言必须是字符串或 null。")),
        };
        let result = if self.choice {
            let number = &arguments[1];
            let count = if let Some(integer) = number.as_i64() {
                PluralCount::Integer(integer)
            } else if let Some(number) = number.as_f64() {
                PluralCount::Float(number)
            } else if let Some(length) = match number {
                ViewValue::Array(items) => Some(items.len()),
                ViewValue::Object(items) => Some(items.len()),
                _ => None,
            } {
                PluralCount::Integer(
                    i64::try_from(length).map_err(|_| render_error("复数计数超出范围。"))?,
                )
            } else {
                return Err(render_error("复数计数必须是数字或数组。"));
            };
            Lang::choice(key, count, &replacements, locale)
        } else {
            Lang::get(key, &replacements, locale)
        };
        result
            .map(ViewValue::String)
            .map_err(|source| ViewError::RenderFailed(Box::new(source)))
    }
}

fn scalar(value: &ViewValue) -> ViewResult<String> {
    match value {
        ViewValue::String(value) => Ok(value.clone()),
        ViewValue::Number(value) => Ok(value.to_string()),
        ViewValue::Bool(value) => Ok(if *value { "1" } else { "" }.into()),
        ViewValue::Null => Ok(String::new()),
        _ => Err(render_error(
            "翻译替换值必须是字符串、数字、布尔值或 null。",
        )),
    }
}

// 复用同一替换规则处理函数和 @lang 翻译区块。
pub(super) fn replacements(replace: &ViewValue) -> ViewResult<BTreeMap<String, String>> {
    match replace {
        ViewValue::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, value)| Ok((index.to_string(), scalar(value)?)))
            .collect(),
        ViewValue::Object(items) => items
            .iter()
            .map(|(key, value)| Ok((key.clone(), scalar(value)?)))
            .collect(),
        _ => Err(render_error("翻译替换参数必须是数组。")),
    }
}

pub(super) fn block(key: &str, replace: &ViewValue) -> ViewResult<String> {
    Lang::get(
        key.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']),
        &replacements(replace)?,
        None,
    )
    .map_err(|source| ViewError::RenderFailed(Box::new(source)))
}
