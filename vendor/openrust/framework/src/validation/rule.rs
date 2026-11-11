use std::sync::Arc;

use crate::contracts::validation::ValidationRule;

use super::{ValidationError, ValidationResult, Value};

/// A fluent validation rule or application-defined rule.
#[derive(Clone)]
pub struct Rule(pub(super) Kind);

#[derive(Clone)]
pub(super) enum Kind {
    Required,
    Present,
    Sometimes,
    Nullable,
    Bail,
    String,
    Integer,
    Boolean,
    Array,
    Min(usize),
    Max(usize),
    Custom(Arc<dyn ValidationRule>),
}

impl Rule {
    pub fn required() -> Self {
        Self(Kind::Required)
    }
    pub fn present() -> Self {
        Self(Kind::Present)
    }
    pub fn sometimes() -> Self {
        Self(Kind::Sometimes)
    }
    pub fn nullable() -> Self {
        Self(Kind::Nullable)
    }
    pub fn bail() -> Self {
        Self(Kind::Bail)
    }
    pub fn string() -> Self {
        Self(Kind::String)
    }
    pub fn integer() -> Self {
        Self(Kind::Integer)
    }
    pub fn boolean() -> Self {
        Self(Kind::Boolean)
    }
    pub fn array() -> Self {
        Self(Kind::Array)
    }
    pub fn min(bound: usize) -> Self {
        Self(Kind::Min(bound))
    }
    pub fn max(bound: usize) -> Self {
        Self(Kind::Max(bound))
    }
    pub fn custom(rule: impl ValidationRule + 'static) -> Self {
        Self(Kind::Custom(Arc::new(rule)))
    }

    pub(super) fn parse(rule: &str) -> ValidationResult<Self> {
        let invalid = || ValidationError::InvalidRule;
        Ok(match rule {
            "required" => Self::required(),
            "present" => Self::present(),
            "sometimes" => Self::sometimes(),
            "nullable" => Self::nullable(),
            "bail" => Self::bail(),
            "string" => Self::string(),
            "integer" => Self::integer(),
            "boolean" => Self::boolean(),
            "array" => Self::array(),
            _ => match rule.split_once(':') {
                Some(("min", bound)) => Self::min(bound.parse().map_err(|_| invalid())?),
                Some(("max", bound)) => Self::max(bound.parse().map_err(|_| invalid())?),
                _ => return Err(invalid()),
            },
        })
    }
}

pub(super) fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

pub(super) fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
        .or_else(|| {
            let raw = value.as_str()?.trim();
            let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
            if digits.len() > 1 && digits.starts_with('0') {
                return None;
            }
            raw.parse::<i64>().ok().map(i128::from)
        })
}

pub(super) fn size(value: &Value, numeric: bool) -> Option<(i128, &'static str)> {
    if numeric {
        return integer(value).map(|number| (number, "numeric"));
    }
    match value {
        Value::Array(values) => Some((values.len() as i128, "array")),
        Value::Object(values) => Some((values.len() as i128, "array")),
        Value::String(value) => Some((value.chars().count() as i128, "string")),
        Value::Number(value) => Some((value.to_string().len() as i128, "string")),
        _ => None,
    }
}
