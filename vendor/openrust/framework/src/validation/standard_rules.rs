use super::{ValidationError, ValidationResult, Value};
use bigdecimal::BigDecimal;

fn numeric_text(value: &Value) -> Option<String> {
    let raw = match value {
        Value::Number(value) if value.is_f64() => {
            crate::support::php_float_string(value.as_f64().unwrap())
        }
        Value::Number(value) => value.to_string(),
        Value::String(value) => value
            .trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
            .to_owned(),
        _ => return None,
    };
    static SYNTAX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let syntax = SYNTAX.get_or_init(|| {
        regex::Regex::new(r"\A[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\z")
            .expect("固定数值语法有效")
    });
    syntax.is_match(&raw).then_some(raw)
}

pub(super) fn numeric(value: &Value) -> bool {
    numeric_text(value).is_some()
}

pub(super) fn number(value: &Value) -> Option<BigDecimal> {
    // 复用十进制定点数，不能把超出 f64 精度的阈值舍入后比较。
    numeric_text(value)?.parse().ok()
}

pub(super) fn check_exponent(value: &Value) -> ValidationResult<()> {
    if let Some(raw) = numeric_text(value)
        && let Some((_, exponent)) = raw.split_once(['e', 'E'])
        && !exponent
            .parse::<i32>()
            .is_ok_and(|exponent| (-1000..=1000).contains(&exponent))
    {
        return Err(ValidationError::Execution {
            source: std::io::Error::other("科学计数法的指数超出允许范围 -1000 至 1000").into(),
        });
    }
    Ok(())
}

pub(super) fn regex(pattern: &str) -> ValidationResult<pcre2::bytes::Regex> {
    let delimiter = pattern.chars().next().ok_or(ValidationError::InvalidRule)?;
    if delimiter.is_ascii_alphanumeric() || delimiter == '\\' || delimiter.is_whitespace() {
        return Err(ValidationError::InvalidRule);
    }
    let closing = match delimiter {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        other => other,
    };
    let end = pattern
        .rfind(closing)
        .filter(|end| *end > 0)
        .ok_or(ValidationError::InvalidRule)?;
    let modifiers = &pattern[end + delimiter.len_utf8()..];
    if modifiers.chars().any(|ch| !"imsxuAUJnD".contains(ch)) {
        return Err(ValidationError::InvalidRule);
    }
    let body = &pattern[delimiter.len_utf8()..end];
    let prefix: String = modifiers
        .chars()
        .filter(|ch| "imsxUJn".contains(*ch))
        .collect();
    let body = if modifiers.contains('A') {
        format!("\\A(?:{body})")
    } else {
        body.to_owned()
    };
    pcre2::bytes::RegexBuilder::new()
        .dollar_endonly(modifiers.contains('D'))
        .utf(modifiers.contains('u'))
        .ucp(modifiers.contains('u'))
        .build(&if prefix.is_empty() {
            body
        } else {
            format!("(?{prefix}){body}")
        })
        .map_err(|_| ValidationError::InvalidRule)
}

pub(super) fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) if value.is_f64() => Some(crate::support::php_float_string(
            value.as_f64().expect("浮点数值"),
        )),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(if *value { "1" } else { "" }.into()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

pub(super) fn date(value: &str) -> bool {
    crate::support::calendar::valid_date(value)
}
