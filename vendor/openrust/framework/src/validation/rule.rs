use std::sync::Arc;

use crate::contracts::validation::ValidationRule;

use super::{
    ValidationError, ValidationResult, Value,
    rules::{AnyOf, AnyOfRules, Dimensions, EncodingRule, Exists, File, Password, Unique},
};

/// A fluent validation rule or application-defined rule.
#[derive(Clone)]
pub struct Rule(pub(super) Kind, pub(super) Option<(String, Vec<String>)>);

#[derive(Clone)]
pub(super) enum Kind {
    Extension(String, Vec<String>),
    Required,
    Conditional {
        name: &'static str,
        parameters: Vec<String>,
    },
    Present,
    Missing,
    Filled,
    Declined,
    Prohibited,
    Exclude,
    Relation {
        same: bool,
        fields: Vec<String>,
    },
    Sometimes,
    Nullable,
    Bail,
    String,
    Scalar(super::scalar_rules::ScalarRule),
    Integer,
    StrictInteger,
    StrictBoolean,
    Numeric,
    Date,
    DateComparison {
        name: &'static str,
        parameter: String,
    },
    SizeComparison {
        name: &'static str,
        parameter: String,
    },
    Accepted,
    Url,
    ActiveUrl,
    Uuid,
    In(Vec<String>),
    NotIn(Vec<String>),
    InArray(String),
    Distinct {
        strict: bool,
        ignore_case: bool,
    },
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
    pub(super) fn new(kind: Kind) -> Self {
        let declaration = match &kind {
            Kind::Required => Some(("required", Vec::new())),
            Kind::Present => Some(("present", Vec::new())),
            Kind::Sometimes => Some(("sometimes", Vec::new())),
            Kind::Nullable => Some(("nullable", Vec::new())),
            Kind::Bail => Some(("bail", Vec::new())),
            Kind::String => Some(("string", Vec::new())),
            Kind::Integer => Some(("integer", Vec::new())),
            Kind::Boolean => Some(("boolean", Vec::new())),
            Kind::Base64 => Some(("base64", Vec::new())),
            Kind::Email => Some(("email", Vec::new())),
            Kind::Array => Some(("array", Vec::new())),
            Kind::CurrentPassword(guard) => {
                Some(("current_password", guard.iter().cloned().collect()))
            }
            Kind::Confirmed(field) => Some(("confirmed", field.iter().cloned().collect())),
            Kind::Min(bound) => Some(("min", vec![bound.to_string()])),
            Kind::Max(bound) => Some(("max", vec![bound.to_string()])),
            _ => None,
        }
        .map(|(name, parameters)| (name.to_owned(), parameters));
        Self(kind, declaration)
    }

    pub fn any_of<T: Into<AnyOfRules>>(rules: impl IntoIterator<Item = T>) -> AnyOf {
        AnyOf::new(rules)
    }

    pub fn required() -> Self {
        Self::new(Kind::Required)
    }
    pub fn present() -> Self {
        Self::new(Kind::Present)
    }
    pub fn sometimes() -> Self {
        Self::new(Kind::Sometimes)
    }
    pub fn nullable() -> Self {
        Self::new(Kind::Nullable)
    }
    pub fn bail() -> Self {
        Self::new(Kind::Bail)
    }
    pub fn string() -> Self {
        Self::new(Kind::String)
    }
    pub fn integer() -> Self {
        Self::new(Kind::Integer)
    }
    pub fn boolean() -> Self {
        Self::new(Kind::Boolean)
    }
    pub fn base64() -> Self {
        Self::new(Kind::Base64)
    }
    pub fn email() -> Self {
        Self::new(Kind::Email)
    }
    pub fn current_password(guard: Option<&str>) -> Self {
        Self::new(Kind::CurrentPassword(guard.map(str::to_owned)))
    }
    pub fn confirmed() -> Self {
        Self::new(Kind::Confirmed(None))
    }
    pub fn array() -> Self {
        Self::new(Kind::Array)
    }
    pub fn min(bound: impl Into<bigdecimal::BigDecimal>) -> Self {
        Self::new(Kind::Min(bound.into()))
    }
    pub fn max(bound: impl Into<bigdecimal::BigDecimal>) -> Self {
        Self::new(Kind::Max(bound.into()))
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
        Self::new(Kind::Custom(Arc::new(rule)))
    }

    pub fn parse(rule: &str) -> ValidationResult<Self> {
        let (name, parameter) = rule
            .split_once(':')
            .map_or((rule, None), |(name, value)| (name, Some(value)));
        let name = crate::support::Str::snake(name.trim(), "_");
        let name = match name.as_str() {
            "int" => "integer".to_owned(),
            "bool" => "boolean".to_owned(),
            _ => name,
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(ValidationError::InvalidRule);
        }
        let parameters = parameter.map_or_else(Vec::new, |value| {
            if name == "regex" {
                vec![value.to_owned()]
            } else {
                super::rule_parameters::csv(value)
                    .into_iter()
                    .map(|value| value.into_owned())
                    .collect()
            }
        });
        let declaration = parameter.map_or_else(|| name.clone(), |value| format!("{name}:{value}"));
        let mut parsed = Self::parse_inner(&declaration)?;
        parsed.1 = Some((name, parameters));
        Ok(parsed)
    }

    fn parse_inner(rule: &str) -> ValidationResult<Self> {
        let (name, parameter) = rule
            .split_once(':')
            .map_or((rule, None), |(name, parameter)| (name, Some(parameter)));
        if let Some(rule) = super::scalar_rules::ScalarRule::parse(name, parameter)? {
            return Ok(Self::new(Kind::Scalar(rule)));
        }
        if let Some(name) = [
            "before",
            "before_or_equal",
            "after",
            "after_or_equal",
            "date_equals",
        ]
        .into_iter()
        .find(|candidate| *candidate == name)
        {
            return Ok(Self::new(Kind::DateComparison {
                name,
                parameter: parameter
                    .ok_or(ValidationError::InvalidRule)
                    .map(super::rule_parameters::csv)?[0]
                    .to_string(),
            }));
        }
        if let Some(name) = ["gt", "lt", "gte", "lte"]
            .into_iter()
            .find(|candidate| *candidate == name)
        {
            return Ok(Self::new(Kind::SizeComparison {
                name,
                parameter: super::rule_parameters::csv(
                    parameter.ok_or(ValidationError::InvalidRule)?,
                )[0]
                .to_string(),
            }));
        }
        let conditional = [
            "accepted_if",
            "declined_if",
            "missing_if",
            "missing_unless",
            "missing_with",
            "missing_with_all",
            "present_if",
            "present_unless",
            "present_with",
            "present_with_all",
            "required_if_accepted",
            "required_if_declined",
            "required_unless",
            "required_with",
            "required_with_all",
            "required_without",
            "required_without_all",
            "prohibited_if",
            "prohibited_if_accepted",
            "prohibited_if_declined",
            "prohibited_unless",
            "prohibits",
            "exclude_unless",
            "exclude_with",
            "exclude_without",
        ]
        .into_iter()
        .find(|candidate| *candidate == name);
        if let Some(name) = conditional {
            let parameters: Vec<_> = parameter
                .map(super::rule_parameters::csv)
                .unwrap_or_default()
                .iter()
                .map(ToString::to_string)
                .collect();
            let minimum = if matches!(
                name,
                "accepted_if"
                    | "declined_if"
                    | "missing_if"
                    | "missing_unless"
                    | "present_if"
                    | "present_unless"
                    | "required_unless"
                    | "prohibited_if"
                    | "prohibited_unless"
                    | "exclude_unless"
            ) {
                2
            } else if name.starts_with("required_w") || name == "prohibits" {
                0
            } else {
                1
            };
            if parameters.len() < minimum {
                return Err(ValidationError::InvalidRule);
            }
            return Ok(Self::new(Kind::Conditional { name, parameters }));
        }
        let invalid = || ValidationError::InvalidRule;
        Ok(match rule {
            "required" => Self::required(),
            "missing" => Self::new(Kind::Missing),
            "filled" => Self::new(Kind::Filled),
            "declined" => Self::new(Kind::Declined),
            "prohibited" => Self::new(Kind::Prohibited),
            "exclude" => Self::new(Kind::Exclude),
            "present" => Self::present(),
            "sometimes" => Self::sometimes(),
            "nullable" => Self::nullable(),
            "bail" => Self::bail(),
            "string" => Self::string(),
            "integer" => Self::integer(),
            "boolean" => Self::boolean(),
            "integer:strict" => Self::new(Kind::StrictInteger),
            "boolean:strict" => Self::new(Kind::StrictBoolean),
            "numeric" => Self::new(Kind::Numeric),
            "in" => Self::new(Kind::In(Vec::new())),
            "not_in" => Self::new(Kind::NotIn(Vec::new())),
            "distinct" => Self::new(Kind::Distinct {
                strict: false,
                ignore_case: false,
            }),
            "date" => Self::new(Kind::Date),
            "accepted" => Self::new(Kind::Accepted),
            "active_url" => Self::new(Kind::ActiveUrl),
            "url" => Self::new(Kind::Url),
            "uuid" => Self::new(Kind::Uuid),
            "base64" => Self::base64(),
            "email" | "email:rfc" => Self::email(),
            "confirmed" => Self::confirmed(),
            "current_password" => Self::current_password(None),
            "array" => Self::array(),
            "file" => File::default().into(),
            "image" => File::image(false).into(),
            _ => match rule.split_once(':') {
                Some((name @ ("same" | "different"), arguments)) => Self::new(Kind::Relation {
                    same: name == "same",
                    fields: super::rule_parameters::csv(arguments)
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                }),
                Some(("in", arguments)) => Self::new(Kind::In(
                    super::rule_parameters::csv(arguments)
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                Some(("distinct", arguments)) => {
                    let parameters = super::rule_parameters::csv(arguments);
                    Self::new(Kind::Distinct {
                        strict: parameters.iter().any(|value| value == "strict"),
                        ignore_case: parameters.iter().any(|value| value == "ignore_case"),
                    })
                }
                Some(("in_array", arguments)) => Self::new(Kind::InArray(
                    super::rule_parameters::csv(arguments)[0].to_string(),
                )),
                Some(("not_in", arguments)) => Self::new(Kind::NotIn(
                    super::rule_parameters::csv(arguments)
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                Some(("array", arguments)) => Self::new(Kind::ArrayKeys(
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
                    Self::new(if kind == "required_if" {
                        Kind::RequiredIf(field, values)
                    } else {
                        Kind::ExcludeIf(field, values)
                    })
                }
                Some(("regex", pattern)) => Self::new(Kind::Regex(Arc::new(
                    super::standard_rules::regex(pattern)?,
                ))),
                Some(("current_password", arguments)) => {
                    Self::current_password(Some(super::rule_parameters::csv(arguments)[0].as_ref()))
                }
                Some(("confirmed", arguments)) => Self::new(Kind::Confirmed(Some(
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
                    Self::new(Kind::Encoding(EncodingRule::parse(argument)?))
                }
                Some(("size", bound)) => {
                    Self::new(Kind::Size(bound.parse().map_err(|_| invalid())?))
                }
                Some(("between", bounds)) => {
                    let (min, max) = bounds.split_once(',').ok_or_else(invalid)?;
                    Self::new(Kind::Between(
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
                _ if matches!(
                    name,
                    "same"
                        | "different"
                        | "in_array"
                        | "required_if"
                        | "exclude_if"
                        | "regex"
                        | "min"
                        | "max"
                        | "size"
                        | "between"
                        | "dimensions"
                        | "encoding"
                        | "exists"
                        | "unique"
                        | "mimes"
                        | "mimetypes"
                        | "extensions"
                        | "email"
                ) =>
                {
                    return Err(invalid());
                }
                _ => Self::new(Kind::Extension(
                    name.to_owned(),
                    parameter.map_or_else(Vec::new, |value| {
                        super::rule_parameters::csv(value)
                            .into_iter()
                            .map(|value| value.into_owned())
                            .collect()
                    }),
                )),
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
            integer(&Value::String(crate::support::php_float_string(
                number.as_f64()?,
            )))
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
