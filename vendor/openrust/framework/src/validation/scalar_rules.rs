use super::{ValidationResult, Value, standard_rules};
use std::{net::IpAddr, sync::Arc};

/// 无字段依赖的规则复用现有校验时机、空值处理与消息格式化。
#[derive(Clone)]
pub(super) enum ScalarRule {
    Ascii,
    Case(&'static str),
    DateFormat(Vec<String>),
    Ulid,
    Timezone {
        group: String,
        country: Option<String>,
    },
    MultipleOf(String),
    Array {
        name: &'static str,
        parameters: Vec<String>,
    },
    NotRegex(Arc<pcre2::bytes::Regex>),
    Affix {
        name: &'static str,
        parameters: Vec<String>,
    },
    Pattern {
        name: &'static str,
        expression: Arc<pcre2::bytes::Regex>,
        numeric: bool,
    },
    List,
    Ip(&'static str),
    Json,
    Digits {
        name: &'static str,
        minimum: Option<bigdecimal::BigDecimal>,
        maximum: Option<bigdecimal::BigDecimal>,
        parameters: Vec<String>,
        decimal: Option<Arc<pcre2::bytes::Regex>>,
    },
}

impl ScalarRule {
    pub(super) fn parse(name: &str, parameter: Option<&str>) -> ValidationResult<Option<Self>> {
        if name == "timezone" {
            let parameters = parameter
                .map(super::rule_parameters::csv)
                .unwrap_or_default();
            let group = parameters
                .first()
                .map_or("ALL".into(), |group| group.to_uppercase());
            let country = parameters.get(1).map(|country| country.to_uppercase());
            if !matches!(
                group.as_str(),
                "ALL"
                    | "ALL_WITH_BC"
                    | "PER_COUNTRY"
                    | "AFRICA"
                    | "AMERICA"
                    | "ANTARCTICA"
                    | "ARCTIC"
                    | "ASIA"
                    | "ATLANTIC"
                    | "AUSTRALIA"
                    | "EUROPE"
                    | "INDIAN"
                    | "PACIFIC"
                    | "UTC"
            ) || group == "PER_COUNTRY"
                && country.as_ref().is_none_or(|country| country.len() != 2)
            {
                return Err(super::ValidationError::InvalidRule);
            }
            return Ok(Some(Self::Timezone { group, country }));
        }
        if name == "multiple_of" {
            return Ok(Some(Self::MultipleOf(
                parameter
                    .ok_or(super::ValidationError::InvalidRule)
                    .map(super::rule_parameters::csv)?[0]
                    .to_string(),
            )));
        }
        if name == "date_format" {
            let parameters: Vec<_> = parameter
                .ok_or(super::ValidationError::InvalidRule)
                .map(super::rule_parameters::csv)?
                .iter()
                .map(ToString::to_string)
                .collect();
            return Ok(Some(Self::DateFormat(parameters)));
        }
        let array = [
            "array_keys",
            "required_array_keys",
            "in_array_keys",
            "contains",
            "doesnt_contain",
        ]
        .into_iter()
        .find(|candidate| *candidate == name);
        if let Some(name) = array {
            let parameters: Vec<_> = parameter
                .map(super::rule_parameters::csv)
                .unwrap_or_default()
                .iter()
                .map(ToString::to_string)
                .collect();
            if name == "array_keys" && parameters.is_empty() {
                return Err(super::ValidationError::InvalidRule);
            }
            return Ok(Some(Self::Array { name, parameters }));
        }
        if name == "not_regex" {
            return Ok(Some(Self::NotRegex(Arc::new(standard_rules::regex(
                parameter.ok_or(super::ValidationError::InvalidRule)?,
            )?))));
        }
        let affix = match name {
            "starts_with" => Some("starts_with"),
            "doesnt_start_with" => Some("doesnt_start_with"),
            "ends_with" => Some("ends_with"),
            "doesnt_end_with" => Some("doesnt_end_with"),
            _ => None,
        };
        if let Some(name) = affix {
            return Ok(Some(Self::Affix {
                name,
                parameters: parameter
                    .map(super::rule_parameters::csv)
                    .unwrap_or_default()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            }));
        }
        if matches!(
            name,
            "digits" | "digits_between" | "max_digits" | "min_digits" | "decimal"
        ) {
            let parameters: Vec<String> = parameter
                .map(super::rule_parameters::csv)
                .unwrap_or_default()
                .iter()
                .map(ToString::to_string)
                .collect();
            if parameters.len() < if name == "digits_between" { 2 } else { 1 } {
                return Err(super::ValidationError::InvalidRule);
            }
            let first = parameters[0]
                .parse::<bigdecimal::BigDecimal>()
                .map_err(|_| super::ValidationError::InvalidRule)?;
            let second = parameters
                .get(1)
                .filter(|_| matches!(name, "digits_between" | "decimal"))
                .map(|value| value.parse::<bigdecimal::BigDecimal>())
                .transpose()
                .map_err(|_| super::ValidationError::InvalidRule)?;
            let (name, minimum, maximum) = match name {
                "digits" => ("digits", Some(first.clone()), Some(first)),
                "digits_between" => ("digits_between", Some(first), second),
                "max_digits" => ("max_digits", None, Some(first)),
                "min_digits" => ("min_digits", Some(first), None),
                _ => (
                    "decimal",
                    Some(first.clone()),
                    Some(second.unwrap_or(first)),
                ),
            };
            return Ok(Some(Self::Digits {
                name,
                minimum,
                maximum,
                parameters,
                decimal: (name == "decimal")
                    .then(|| standard_rules::regex(r"/^[+-]?\d*\.?(\d*)$/").map(Arc::new))
                    .transpose()?,
            }));
        }
        let pattern = match (
            name,
            parameter.is_some_and(|value| value.split(',').next() == Some("ascii")),
        ) {
            ("ascii", _) => return Ok(Some(Self::Ascii)),
            ("ulid", _) => return Ok(Some(Self::Ulid)),
            ("lowercase", _) => return Ok(Some(Self::Case("lowercase"))),
            ("uppercase", _) => return Ok(Some(Self::Case("uppercase"))),
            ("list", _) => return Ok(Some(Self::List)),
            ("ip", _) => return Ok(Some(Self::Ip("ip"))),
            ("ipv4", _) => return Ok(Some(Self::Ip("ipv4"))),
            ("ipv6", _) => return Ok(Some(Self::Ip("ipv6"))),
            ("json", _) => return Ok(Some(Self::Json)),
            ("alpha", true) => ("alpha", r"/\A[a-zA-Z]+\z/u", false),
            ("alpha", false) => ("alpha", r"/\A[\pL\pM]+\z/u", false),
            ("alpha_dash", true) => ("alpha_dash", r"/\A[a-zA-Z0-9_-]+\z/u", true),
            ("alpha_dash", false) => ("alpha_dash", r"/\A[\pL\pM\pN_-]+\z/u", true),
            ("alpha_num", true) => ("alpha_num", r"/\A[a-zA-Z0-9]+\z/u", true),
            ("alpha_num", false) => ("alpha_num", r"/\A[\pL\pM\pN]+\z/u", true),
            ("hex_color", _) => (
                "hex_color",
                r"/^#(?:(?:[0-9a-f]{3}){1,2}|(?:[0-9a-f]{4}){1,2})$/i",
                false,
            ),
            ("mac_address", _) => (
                "mac_address",
                r"/\A(?:[0-9a-f]{2}(:|-)(?:[0-9a-f]{2}\1){4}[0-9a-f]{2}|[0-9a-f]{4}(\.)[0-9a-f]{4}\2[0-9a-f]{4})\z/i",
                false,
            ),
            _ => return Ok(None),
        };
        Ok(Some(Self::Pattern {
            name: pattern.0,
            expression: Arc::new(standard_rules::regex(pattern.1)?),
            numeric: pattern.2,
        }))
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Ulid => "ulid",
            Self::Timezone { .. } => "timezone",
            Self::MultipleOf(_) => "multiple_of",
            Self::Case(name) => name,
            Self::DateFormat(_) => "date_format",
            Self::Array { name, .. } => name,
            Self::NotRegex(_) => "not_regex",
            Self::Affix { name, .. } => name,
            Self::Pattern { name, .. } => name,
            Self::List => "list",
            Self::Ip(name) => name,
            Self::Json => "json",
            Self::Digits { name, .. } => name,
        }
    }

    pub(super) fn replacements(&self) -> Vec<(String, String)> {
        if let Self::MultipleOf(parameter) = self {
            return vec![("value".into(), parameter.clone())];
        }
        if let Self::DateFormat(parameters) = self {
            return vec![("format".into(), parameters[0].clone())];
        }
        if let Self::Affix { parameters, .. } | Self::Array { parameters, .. } = self {
            return vec![("values".into(), parameters.join(", "))];
        }
        let Self::Digits {
            name, parameters, ..
        } = self
        else {
            return Vec::new();
        };
        match *name {
            "digits" => vec![("digits".into(), parameters[0].clone())],
            "max_digits" => vec![("max".into(), parameters[0].clone())],
            "min_digits" => vec![("min".into(), parameters[0].clone())],
            "digits_between" => vec![
                ("min".into(), parameters[0].clone()),
                ("max".into(), parameters[1].clone()),
            ],
            _ => vec![(
                "decimal".into(),
                parameters
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("-"),
            )],
        }
    }

    pub(super) fn passes(&self, value: &Value) -> ValidationResult<bool> {
        Ok(match self {
            Self::Digits {
                minimum,
                maximum,
                decimal,
                ..
            } => {
                let text = standard_rules::text(value);
                let length = if let Some(expression) = decimal {
                    if !standard_rules::numeric(value) {
                        None
                    } else if let Some(text) = text {
                        expression
                            .captures(text.as_bytes())
                            .map_err(|source| super::ValidationError::Execution {
                                source: source.into(),
                            })?
                            .and_then(|capture| capture.get(1).map(|value| value.as_bytes().len()))
                    } else {
                        None
                    }
                } else {
                    text.filter(|_| value.is_string() || value.is_number())
                        .filter(|text| text.bytes().all(|byte| byte.is_ascii_digit()))
                        .map(|text| text.len())
                };
                length.is_some_and(|length| {
                    let length = bigdecimal::BigDecimal::from(
                        u64::try_from(length).expect("字符串长度可表示为u64"),
                    );
                    minimum.as_ref().is_none_or(|minimum| length >= *minimum)
                        && maximum.as_ref().is_none_or(|maximum| length <= *maximum)
                })
            }
            Self::NotRegex(expression) => {
                if !(value.is_string() || value.is_number()) {
                    false
                } else if let Some(text) = standard_rules::text(value) {
                    !expression.is_match(text.as_bytes()).map_err(|source| {
                        super::ValidationError::Execution {
                            source: source.into(),
                        }
                    })?
                } else {
                    false
                }
            }
            Self::Affix { name, parameters } => {
                if !(value.is_string() || value.is_number()) {
                    false
                } else if let Some(text) = standard_rules::text(value) {
                    let needles: Vec<_> = parameters.iter().map(String::as_str).collect();
                    let matched = if matches!(*name, "starts_with" | "doesnt_start_with") {
                        crate::support::Str::starts_with(&text, &needles)
                    } else {
                        crate::support::Str::ends_with(&text, &needles)
                    };
                    matched != name.starts_with("doesnt_")
                } else {
                    false
                }
            }
            Self::Ulid => value.as_str().is_some_and(crate::support::Str::is_ulid),
            Self::Timezone { group, country } => value.as_str().is_some_and(|value| {
                crate::support::calendar::identifiers::IDENTIFIERS
                    .iter()
                    .any(|(name, canonical, region)| {
                        if *name != value {
                            return false;
                        }
                        match group.as_str() {
                            "ALL_WITH_BC" => true,
                            "PER_COUNTRY" => country.as_deref() == Some(*region),
                            "ALL" => {
                                *canonical
                                    && (name.contains('/') && !name.starts_with("Etc/")
                                        || *name == "UTC")
                            }
                            "UTC" => *canonical && name.starts_with("UTC"),
                            group => {
                                *canonical
                                    && name
                                        .split('/')
                                        .next()
                                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(group))
                            }
                        }
                    })
            }),
            Self::MultipleOf(parameter) => {
                let parameter = Value::String(parameter.clone());
                standard_rules::check_exponent(value)?;
                standard_rules::check_exponent(&parameter)?;
                match (
                    standard_rules::number(value),
                    standard_rules::number(&parameter),
                ) {
                    (Some(numerator), Some(denominator)) if denominator != 0 => {
                        numerator % denominator == 0
                    }
                    _ => false,
                }
            }
            Self::DateFormat(parameters) => {
                if !(value.is_string() || value.is_number()) {
                    false
                } else {
                    let text = standard_rules::text(value).expect("字符串或数值可格式化");
                    parameters.iter().any(|format| {
                        crate::support::calendar::parse_input_date(
                            Value::String(text.clone()),
                            Some(&format!("!{format}")),
                            "UTC",
                            crate::support::calendar::DateReference::Frozen(
                                jiff::Timestamp::UNIX_EPOCH,
                            ),
                        )
                        .ok()
                        .and_then(|date| date.format(format).ok())
                        .is_some_and(|formatted| {
                            crate::support::php_loose_string_value_equal(&formatted, value)
                        })
                    })
                }
            }
            Self::Array { name, parameters } => {
                let entries: Option<Vec<_>> = match value {
                    Value::Array(items) => Some(
                        items
                            .iter()
                            .enumerate()
                            .map(|(key, value)| (key.to_string(), value))
                            .collect(),
                    ),
                    Value::Object(items) => Some(
                        items
                            .iter()
                            .map(|(key, value)| (key.clone(), value))
                            .collect(),
                    ),
                    _ => None,
                };
                entries.is_some_and(|entries| match *name {
                    "array_keys" => entries.iter().all(|(key, _)| parameters.contains(key)),
                    "required_array_keys" => parameters
                        .iter()
                        .all(|key| entries.iter().any(|(candidate, _)| candidate == key)),
                    "in_array_keys" => parameters
                        .iter()
                        .any(|key| entries.iter().any(|(candidate, _)| candidate == key)),
                    "contains" => parameters.iter().all(|needle| {
                        entries.iter().any(|(_, value)| {
                            crate::support::php_loose_string_value_equal(needle, value)
                        })
                    }),
                    _ => parameters.iter().all(|needle| {
                        !entries
                            .iter()
                            .any(|(_, value)| value.as_str() == Some(needle.as_str()))
                    }),
                })
            }
            Self::Case(name) => value.as_str().is_some_and(|text| {
                text == if *name == "lowercase" {
                    crate::support::Str::lower(text)
                } else {
                    crate::support::Str::upper(text)
                }
            }),
            Self::Ascii => value.as_str().is_some_and(str::is_ascii),
            Self::Pattern {
                expression,
                numeric,
                ..
            } => {
                if !(value.is_string() || *numeric && value.is_number()) {
                    false
                } else if let Some(text) = standard_rules::text(value) {
                    expression.is_match(text.as_bytes()).map_err(|source| {
                        super::ValidationError::Execution {
                            source: source.into(),
                        }
                    })?
                } else {
                    false
                }
            }
            Self::List => {
                value.is_array()
                    || value.as_object().is_some_and(|items| {
                        items
                            .keys()
                            .enumerate()
                            .all(|(index, key)| *key == index.to_string())
                    })
            }
            Self::Ip(name) => value
                .as_str()
                .and_then(|value| value.parse::<IpAddr>().ok())
                .is_some_and(|address| match *name {
                    "ipv4" => address.is_ipv4(),
                    "ipv6" => address.is_ipv6(),
                    _ => true,
                }),
            Self::Json => {
                !value.is_null()
                    && !value.is_array()
                    && !value.is_object()
                    && standard_rules::text(value)
                        .is_some_and(|text| serde_json::from_str::<Value>(&text).is_ok())
            }
        })
    }
}
