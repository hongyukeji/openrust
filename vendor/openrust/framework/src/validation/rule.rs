use std::sync::Arc;

use crate::contracts::validation::ValidationRule;

use super::{
    ValidationError, ValidationResult, Value,
    rules::{Dimensions, EncodingRule, Exists, File, Password, Unique},
};

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
    StrictInteger,
    StrictBoolean,
    Numeric,
    Date,
    Accepted,
    Url,
    Uuid,
    In(Vec<String>),
    Regex(Arc<pcre2::bytes::Regex>),
    RequiredIf(String, Vec<String>),
    ExcludeIf(String, Vec<String>),
    ArrayKeys(Vec<String>),
    Boolean,
    Base64,
    Email,
    Confirmed(Option<String>),
    CurrentPassword(Option<String>),
    Password(Password),
    Array,
    Min(bigdecimal::BigDecimal),
    Max(bigdecimal::BigDecimal),
    Size(bigdecimal::BigDecimal),
    Between(bigdecimal::BigDecimal, bigdecimal::BigDecimal),
    File(File),
    Dimensions(Dimensions),
    Encoding(EncodingRule),
    Exists(Exists),
    Unique(Unique),
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
    pub fn base64() -> Self {
        Self(Kind::Base64)
    }
    pub fn email() -> Self {
        Self(Kind::Email)
    }
    #[allow(non_snake_case, reason = "Matches Laravel rule naming")]
    pub fn currentPassword(guard: Option<&str>) -> Self {
        Self(Kind::CurrentPassword(guard.map(str::to_owned)))
    }
    pub fn confirmed() -> Self {
        Self(Kind::Confirmed(None))
    }
    pub fn array() -> Self {
        Self(Kind::Array)
    }
    pub fn min(bound: impl Into<bigdecimal::BigDecimal>) -> Self {
        Self(Kind::Min(bound.into()))
    }
    pub fn max(bound: impl Into<bigdecimal::BigDecimal>) -> Self {
        Self(Kind::Max(bound.into()))
    }
    pub fn dimensions() -> Dimensions {
        Dimensions::default()
    }
    pub fn exists(table: &str, column: Option<&str>) -> Exists {
        Exists::new(table, column)
    }
    pub fn unique(table: &str, column: Option<&str>) -> Unique {
        Unique::new(table, column)
    }
    pub fn custom(rule: impl ValidationRule + 'static) -> Self {
        Self(Kind::Custom(Arc::new(rule)))
    }

    pub fn parse(rule: &str) -> ValidationResult<Self> {
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
            "integer:strict" => Self(Kind::StrictInteger),
            "boolean:strict" => Self(Kind::StrictBoolean),
            "numeric" => Self(Kind::Numeric),
            "date" => Self(Kind::Date),
            "accepted" => Self(Kind::Accepted),
            "url" => Self(Kind::Url),
            "uuid" => Self(Kind::Uuid),
            "base64" => Self::base64(),
            "email" | "email:rfc" => Self::email(),
            "confirmed" => Self::confirmed(),
            "current_password" => Self::currentPassword(None),
            "array" => Self::array(),
            "file" => File::default().into(),
            "image" => File::image(false).into(),
            _ => match rule.split_once(':') {
                Some(("in", arguments)) => Self(Kind::In(
                    super::rule_parameters::csv(arguments)
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                Some(("array", arguments)) => Self(Kind::ArrayKeys(
                    super::rule_parameters::csv(arguments)
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                Some((kind @ ("required_if" | "exclude_if"), arguments)) => {
                    let args = super::rule_parameters::csv(arguments);
                    if args.len() < 2 {
                        return Err(invalid());
                    }
                    let field = args[0].to_string();
                    let values = args[1..].iter().map(ToString::to_string).collect();
                    Self(if kind == "required_if" {
                        Kind::RequiredIf(field, values)
                    } else {
                        Kind::ExcludeIf(field, values)
                    })
                }
                Some(("regex", pattern)) => Self(Kind::Regex(Arc::new(
                    super::standard_rules::regex(pattern)?,
                ))),
                Some(("current_password", arguments)) => {
                    Self::currentPassword(Some(super::rule_parameters::csv(arguments)[0].as_ref()))
                }
                Some(("confirmed", arguments)) => Self(Kind::Confirmed(Some(
                    super::rule_parameters::csv(arguments)[0].to_string(),
                ))),
                Some(("min", bound)) => Self::min(
                    bound
                        .parse::<bigdecimal::BigDecimal>()
                        .map_err(|_| invalid())?,
                ),
                Some(("max", bound)) => Self::max(
                    bound
                        .parse::<bigdecimal::BigDecimal>()
                        .map_err(|_| invalid())?,
                ),
                Some(("dimensions", arguments)) => Dimensions::parse(arguments)?.into(),
                Some(("encoding", argument)) => {
                    Self(Kind::Encoding(EncodingRule::parse(argument)?))
                }
                Some(("size", bound)) => Self(Kind::Size(bound.parse().map_err(|_| invalid())?)),
                Some(("between", bounds)) => {
                    let (min, max) = bounds.split_once(',').ok_or_else(invalid)?;
                    Self(Kind::Between(
                        min.parse().map_err(|_| invalid())?,
                        max.parse().map_err(|_| invalid())?,
                    ))
                }
                Some((name @ ("image" | "mimes" | "mimetypes" | "extensions"), arguments)) => {
                    File::parse(name, arguments)?.into()
                }
                Some((kind @ ("exists" | "unique"), arguments)) => {
                    let parsed = super::rule_parameters::csv(arguments);
                    let arguments: Vec<_> =
                        parsed.iter().map(std::convert::AsRef::as_ref).collect();
                    let (table, rest) = arguments
                        .split_first()
                        .filter(|(table, _)| !table.is_empty())
                        .ok_or_else(invalid)?;
                    if kind == "exists" {
                        Exists::parse(table, rest)?.into()
                    } else {
                        Unique::parse(table, rest)?.into()
                    }
                }
                _ => return Err(invalid()),
            },
        })
    }
}

pub(super) fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value
            .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
            .is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

pub(super) fn integer(value: &Value) -> Option<i128> {
    match value {
        Value::Bool(true) => Some(1),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                return Some(i128::from(value));
            }
            if number.is_u64() {
                return None;
            }
            let number = number.as_f64()?;
            (number.fract() == 0.0 && number >= i64::MIN as f64 && number < -(i64::MIN as f64))
                .then_some(number as i128)
        }
        Value::String(value) => {
            let raw = value.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
            let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
            if digits.is_empty()
                || !digits.bytes().all(|byte| byte.is_ascii_digit())
                || (digits.len() > 1 && digits.starts_with('0'))
            {
                return None;
            }
            raw.parse::<i64>().ok().map(i128::from)
        }
        _ => None,
    }
}

pub(super) fn size(value: &Value, numeric: bool) -> Option<(bigdecimal::BigDecimal, &'static str)> {
    if numeric && let Some(number) = super::standard_rules::number(value) {
        return Some((number, "numeric"));
    }
    match value {
        Value::Array(values) => Some((bigdecimal::BigDecimal::from(values.len() as u64), "array")),
        Value::Object(values) => Some((bigdecimal::BigDecimal::from(values.len() as u64), "array")),
        Value::String(value) => Some((
            bigdecimal::BigDecimal::from(value.chars().count() as u64),
            "string",
        )),
        Value::Number(value) => Some((
            bigdecimal::BigDecimal::from(value.to_string().len() as u64),
            "string",
        )),
        _ => None,
    }
}
