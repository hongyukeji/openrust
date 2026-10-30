//! 普通与 selective 验证共用唯一规则执行器。
use super::{Field, InputContext, Kind, Rule, RuleError, Rules, path};
use crate::core::{Error, Result, ValidationErrors};
use serde_json::Value;
use validator::ValidateEmail;

pub(super) fn run(
    rules: &Rules,
    input: &Value,
    patterns: Option<&[&str]>,
    context: InputContext,
) -> Result<()> {
    if rules.invalid.is_some() {
        return Err(path::invalid());
    }
    path::inspect(input)?;
    if let Some(patterns) = patterns {
        path::check_patterns(patterns)?;
    }
    // 即使没有选中字段，非法 schema 也不能被过滤掩盖。
    for (attribute, field_rules) in &rules.fields {
        let _ = path::parse(attribute)?;
        for rule in field_rules {
            check_rule(rule)?;
        }
    }
    let mut errors = ValidationErrors::new();
    let mut budget = path::Budget::default();
    let (mut error_count, mut message_bytes) = (0_usize, 0_usize);
    for (attribute, field_rules) in &rules.fields {
        let segments = path::parse(attribute)?;
        for (attribute, value) in path::expand(input, &segments, &mut budget)? {
            if !path::selected(patterns, &attribute, &mut budget)? {
                continue;
            }
            let field = Field {
                attribute: &attribute,
                value,
                input,
                context,
            };
            for rule in field_rules {
                if let Err(error) = evaluate(rule, field_rules, field) {
                    error_count += 1;
                    let message = message(&error);
                    message_bytes = message_bytes.saturating_add(message.len());
                    if error_count > 1024
                        || message.len() > 16 * 1024
                        || message_bytes > 1024 * 1024
                    {
                        return Err(path::limit());
                    }
                    errors.add(path::error_attribute(&attribute), message);
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Validation(errors))
    }
}

fn check_rule(rule: &Rule) -> Result<()> {
    let valid = match &rule.0 {
        Kind::Length(min, max) => !matches!((min, max), (Some(min), Some(max)) if min > max),
        Kind::Range(min, max) => !matches!((min, max), (Some(min), Some(max)) if min > max),
        _ => true,
    };
    if valid { Ok(()) } else { Err(path::invalid()) }
}

fn evaluate(rule: &Rule, rules: &[Rule], field: Field<'_>) -> std::result::Result<(), RuleError> {
    if matches!(rule.0, Kind::Required) {
        return if field.value.is_some_and(required) {
            Ok(())
        } else {
            Err(RuleError::new("required"))
        };
    }
    if matches!(rule.0, Kind::Present) {
        return if field.value.is_some() {
            Ok(())
        } else {
            Err(RuleError::new("present"))
        };
    }
    if matches!(rule.0, Kind::Nullable) {
        return Ok(());
    }
    let Some(value) = field.value else {
        return Ok(());
    };
    // Length/Range 保留既有 validator 对显式字符串的检查；其余普通规则沿 Laravel
    // 空串短路。Typed 与原始 Json/Query 共用规则，不在此偷偷 trim 或补 required。
    if !matches!(rule.0, Kind::Length(..) | Kind::Range(..))
        && value.as_str().is_some_and(empty_string)
    {
        return Ok(());
    }
    if value.is_null() && rules.iter().any(|rule| matches!(rule.0, Kind::Nullable)) {
        return Ok(());
    }
    evaluate_present(rule, rules, field, value)
}

fn evaluate_present(
    rule: &Rule,
    rules: &[Rule],
    field: Field<'_>,
    value: &Value,
) -> std::result::Result<(), RuleError> {
    let failure = match &rule.0 {
        Kind::Required | Kind::Present | Kind::Nullable => None,
        Kind::String => (!value.is_string()).then(|| RuleError::new("string")),
        Kind::Integer => (!laravel_integer(value)).then(|| RuleError::new("integer")),
        Kind::Boolean => (!boolean(value)).then(|| RuleError::new("boolean")),
        Kind::Array => (!value.is_array() && !value.is_object()).then(|| RuleError::new("array")),
        Kind::Email => (!value.as_str().is_some_and(|value| value.validate_email()))
            .then(|| RuleError::new("email")),
        Kind::Min(min) => boundary(rules, value, *min, true),
        Kind::Max(max) => boundary(rules, value, *max, false),
        Kind::Length(min, max) => length(value, *min, *max),
        Kind::Range(min, max) => range(value, field.context, *min, *max),
        Kind::Custom(callback) => return callback(field),
    };
    failure.map_or(Ok(()), Err)
}
fn required(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !empty_string(value),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    }
}
fn integer(value: &Value, context: InputContext) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
        .or_else(|| {
            if context == InputContext::Query {
                value.as_str()?.parse().ok()
            } else {
                None
            }
        })
}
fn laravel_integer(value: &Value) -> bool {
    let text = match value {
        Value::String(value) => value.trim_ascii().to_owned(),
        Value::Number(value) => {
            let text = value.to_string();
            text.strip_suffix(".0").unwrap_or(&text).to_owned()
        }
        Value::Bool(true) => "1".to_owned(),
        _ => return false,
    };
    let digits = text.strip_prefix(['+', '-']).unwrap_or(&text);
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits.len() == 1 || !digits.starts_with('0'))
        && text.parse::<i64>().is_ok()
}
fn boolean(value: &Value) -> bool {
    value.is_boolean()
        || matches!(value.as_i64(), Some(0 | 1))
        || value
            .as_str()
            .is_some_and(|value| matches!(value, "0" | "1"))
}
fn boundary(rules: &[Rule], value: &Value, bound: i64, minimum: bool) -> Option<RuleError> {
    let mut error = RuleError::new(if minimum { "min" } else { "max" });
    error.add_param(if minimum { "min" } else { "max" }.into(), &bound);
    let numeric = rules.iter().any(|rule| matches!(rule.0, Kind::Integer));
    let compared = if numeric {
        match super::numeric::compare(value, bound) {
            Ok(compared) => compared,
            Err(()) => return Some(error),
        }
    } else {
        None
    };
    let compared = compared.or_else(|| {
        let size = match value {
            Value::String(value) => value.chars().count(),
            Value::Array(value) => value.len(),
            Value::Object(value) => value.len(),
            Value::Null | Value::Bool(false) => 0,
            Value::Bool(true) => 1,
            Value::Number(value) => {
                let text = value.to_string();
                text.strip_suffix(".0").unwrap_or(&text).chars().count()
            }
        };
        Some(i128::try_from(size).ok()?.cmp(&i128::from(bound)))
    });
    if compared.is_some_and(|compared| {
        if minimum {
            !compared.is_lt()
        } else {
            !compared.is_gt()
        }
    }) {
        None
    } else {
        Some(error)
    }
}
fn length(value: &Value, min: Option<u64>, max: Option<u64>) -> Option<RuleError> {
    let length = match value {
        Value::String(value) => Some(value.chars().count()),
        Value::Array(value) => Some(value.len()),
        Value::Object(value) => Some(value.len()),
        _ => None,
    }
    .and_then(|length| u64::try_from(length).ok());
    if length.is_some_and(|length| {
        min.is_none_or(|min| length >= min) && max.is_none_or(|max| length <= max)
    }) {
        return None;
    }
    let mut error = RuleError::new("length");
    if let Some(min) = min {
        error.add_param("min".into(), &min);
    }
    if let Some(max) = max {
        error.add_param("max".into(), &max);
    }
    Some(error)
}
fn range(
    value: &Value,
    context: InputContext,
    min: Option<i64>,
    max: Option<i64>,
) -> Option<RuleError> {
    if integer(value, context).is_some_and(|value| {
        min.is_none_or(|min| value >= i128::from(min))
            && max.is_none_or(|max| value <= i128::from(max))
    }) {
        return None;
    }
    let mut error = RuleError::new("range");
    if let Some(min) = min {
        error.add_param("min".into(), &min);
    }
    if let Some(max) = max {
        error.add_param("max".into(), &max);
    }
    Some(error)
}
fn message(failure: &RuleError) -> String {
    failure.message.as_ref().map_or_else(
        || {
            let key = format!("openrust.validation.{}", failure.code);
            let args = failure
                .params
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value
                            .as_str()
                            .map_or_else(|| value.to_string(), str::to_owned),
                    )
                })
                .collect();
            crate::i18n::translate_current(&key, &args)
                .filter(|message| message != &key)
                .unwrap_or_else(|| failure.code.to_string())
        },
        ToString::to_string,
    )
}

fn empty_string(value: &str) -> bool {
    value
        .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
        .is_empty()
}
