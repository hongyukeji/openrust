use super::{constraints::parameters, in_rule::scalar};
use crate::{
    contracts::validation::{CompilableRules, ParameterizedRule},
    validation::{Rule, RuleSet, ValidationResult, Value},
};

/// 对应可选标量或多个比较值；数组与集合保留成员边界。
#[derive(Clone)]
pub struct ComparisonValues(Vec<Value>);
impl From<Value> for ComparisonValues {
    fn from(value: Value) -> Self {
        Self(match value {
            Value::Array(values) => values,
            Value::Object(values) => values.into_values().collect(),
            value => vec![value],
        })
    }
}
impl<T: Into<Value>> From<Vec<T>> for ComparisonValues {
    fn from(values: Vec<T>) -> Self {
        Self(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>, const N: usize> From<[T; N]> for ComparisonValues {
    fn from(values: [T; N]) -> Self {
        Self(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>> From<crate::support::Collection<T>> for ComparisonValues {
    fn from(values: crate::support::Collection<T>) -> Self {
        Self(values.into_iter().map(|(_, value)| value.into()).collect())
    }
}
impl<T: Into<Value>> From<Option<T>> for ComparisonValues {
    fn from(value: Option<T>) -> Self {
        Self(vec![value.map_or(Value::Null, Into::into)])
    }
}
macro_rules! single_values {($($ty:ty),+)=>{$(impl From<$ty> for ComparisonValues{fn from(value:$ty)->Self{Self(vec![Value::from(value)])}})+};}
single_values!(
    &str,
    String,
    bool,
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    usize,
    isize,
    f32,
    f64,
    ()
);
impl IntoIterator for ComparisonValues {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

fn condition_value(value: Value, json_numbers: bool) -> String {
    match value {
        Value::String(value) => value,
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_owned(),
        Value::Number(number) if json_numbers => json_number(&number),
        value => scalar(value),
    }
}
// JSON数值采用最短往返有效数字，类型转换字符串仍走既有十四位规则。
fn json_number(number: &serde_json::Number) -> String {
    let raw = number.to_string();
    if !number.is_f64() {
        return raw;
    }
    let (sign, body) = raw
        .strip_prefix('-')
        .map_or(("", raw.as_str()), |value| ("-", value));
    let (coefficient, exponent) = body
        .split_once(['e', 'E'])
        .map_or((body, 0), |(c, e)| (c, e.parse::<i32>().unwrap_or(0)));
    let decimal = coefficient.find('.').unwrap_or(coefficient.len());
    let all = coefficient.replace('.', "");
    let Some(first) = all.find(|c| c != '0') else {
        return format!("{sign}0");
    };
    let digits = all[first..].trim_end_matches('0');
    let exponent =
        exponent + i32::try_from(decimal).unwrap_or(0) - i32::try_from(first).unwrap_or(0) - 1;
    if !(-4..17).contains(&exponent) {
        let fraction = if digits.len() > 1 { &digits[1..] } else { "0" };
        return format!("{sign}{}.{fraction}e{exponent:+}", &digits[..1]);
    }
    let decimal = exponent + 1;
    if decimal <= 0 {
        return format!(
            "{sign}0.{}{digits}",
            "0".repeat(usize::try_from(-decimal).unwrap_or(0))
        );
    }
    let decimal = usize::try_from(decimal).unwrap_or(0);
    if decimal >= digits.len() {
        format!("{sign}{digits}{}", "0".repeat(decimal - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..decimal], &digits[decimal..])
    }
}
fn display_parameterized(values: Vec<String>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let Some((name, args)) = values.split_first() else {
        return Err(std::fmt::Error);
    };
    f.write_str(name)?;
    if !args.is_empty() {
        f.write_str(":")?;
        let args = args
            .iter()
            .map(|value| {
                if value.contains([',', '"']) {
                    format!("\"{}\"", value.replace('"', "\"\""))
                } else {
                    value.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        f.write_str(&args)?;
    }
    Ok(())
}
macro_rules! parameterized {
    ($name:ident) => {
        impl From<$name> for Rule {
            fn from(rule: $name) -> Self {
                Self::parameterized(rule)
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                display_parameterized(self.to_array(), f)
            }
        }
    };
}
macro_rules! field_rule {
    ($name:ident,$rule:literal) => {
        #[derive(Clone)]
        pub struct $name {
            field: String,
        }
        impl $name {
            pub fn new(field: impl Into<String>) -> Self {
                Self {
                    field: field.into(),
                }
            }
        }
        impl ParameterizedRule for $name {
            fn to_array(&self) -> Vec<String> {
                vec![$rule.into(), self.field.clone()]
            }
        }
        parameterized!($name);
    };
}
macro_rules! fields_rule {
    ($name:ident,$rule:literal) => {
        #[derive(Clone)]
        pub struct $name {
            fields: Vec<String>,
        }
        impl $name {
            pub fn new(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
                Self {
                    fields: fields.into_iter().map(Into::into).collect(),
                }
            }
        }
        impl ParameterizedRule for $name {
            fn to_array(&self) -> Vec<String> {
                std::iter::once($rule.to_owned())
                    .chain(self.fields.clone())
                    .collect()
            }
        }
        parameterized!($name);
    };
}
macro_rules! comparison_rule {
    ($name:ident,$rule:literal,$json:expr) => {
        #[derive(Clone)]
        pub struct $name {
            field: String,
            values: Vec<String>,
        }
        impl $name {
            pub fn new(field: impl Into<String>, values: impl Into<ComparisonValues>) -> Self {
                Self {
                    field: field.into(),
                    values: values
                        .into()
                        .into_iter()
                        .map(|value| condition_value(value, $json))
                        .collect(),
                }
            }
        }
        impl ParameterizedRule for $name {
            fn to_array(&self) -> Vec<String> {
                std::iter::once($rule.to_owned())
                    .chain(std::iter::once(self.field.clone()))
                    .chain(self.values.clone())
                    .collect()
            }
        }
        parameterized!($name);
    };
}
field_rule!(ExcludeWith, "exclude_with");
field_rule!(ExcludeWithout, "exclude_without");
field_rule!(InArray, "in_array");
field_rule!(ProhibitedIfAccepted, "prohibited_if_accepted");
field_rule!(ProhibitedIfDeclined, "prohibited_if_declined");
field_rule!(RequiredIfAccepted, "required_if_accepted");
field_rule!(RequiredIfDeclined, "required_if_declined");
fields_rule!(InArrayKeys, "in_array_keys");
fields_rule!(MissingWith, "missing_with");
fields_rule!(MissingWithAll, "missing_with_all");
fields_rule!(PresentWith, "present_with");
fields_rule!(PresentWithAll, "present_with_all");
fields_rule!(Prohibits, "prohibits");
fields_rule!(RequiredArrayKeys, "required_array_keys");
fields_rule!(RequiredWith, "required_with");
fields_rule!(RequiredWithAll, "required_with_all");
fields_rule!(RequiredWithout, "required_without");
fields_rule!(RequiredWithoutAll, "required_without_all");
comparison_rule!(AcceptedIf, "accepted_if", true);
comparison_rule!(DeclinedIf, "declined_if", false);
comparison_rule!(MissingIf, "missing_if", false);
comparison_rule!(MissingUnless, "missing_unless", false);
comparison_rule!(PresentIf, "present_if", false);
comparison_rule!(PresentUnless, "present_unless", false);
#[derive(Clone)]
pub struct Regex {
    pattern: String,
}
impl Regex {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
        }
    }
}
impl ParameterizedRule for Regex {
    fn to_array(&self) -> Vec<String> {
        vec!["regex".into(), self.pattern.clone()]
    }
}
parameterized!(Regex);
#[derive(Clone)]
pub struct NotRegex {
    pattern: String,
}
impl NotRegex {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
        }
    }
}
impl ParameterizedRule for NotRegex {
    fn to_array(&self) -> Vec<String> {
        vec!["not_regex".into(), self.pattern.clone()]
    }
}
parameterized!(NotRegex);
#[derive(Clone, Default)]
pub struct Confirmed {
    confirmation_field: Option<String>,
}
impl Confirmed {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn custom_field(mut self, field: impl Into<String>) -> Self {
        self.confirmation_field = Some(field.into());
        self
    }
}
impl ParameterizedRule for Confirmed {
    fn to_array(&self) -> Vec<String> {
        std::iter::once("confirmed".to_owned())
            .chain(self.confirmation_field.clone())
            .collect()
    }
}
parameterized!(Confirmed);
#[derive(Clone, Default)]
pub struct CurrentPassword {
    guard: Option<String>,
}
impl CurrentPassword {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn guard(mut self, field: impl Into<String>) -> Self {
        self.guard = Some(field.into());
        self
    }
}
impl ParameterizedRule for CurrentPassword {
    fn to_array(&self) -> Vec<String> {
        std::iter::once("current_password".to_owned())
            .chain(self.guard.clone())
            .collect()
    }
}
parameterized!(CurrentPassword);
#[derive(Clone)]
pub struct ArrayKeys {
    values: Vec<String>,
}
impl ArrayKeys {
    pub fn new<T: Into<Value>>(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|value| scalar(value.into()))
                .collect(),
        }
    }
}
impl std::fmt::Display for ArrayKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "array_keys:{}", self.values.join(","))
    }
}
impl CompilableRules for ArrayKeys {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        _: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        RuleSet::new().field(attribute, [Rule::parse(&self.to_string())?])
    }
}
impl From<ArrayKeys> for Rule {
    fn from(rule: ArrayKeys) -> Self {
        Self::compilable(rule)
    }
}
#[derive(Clone)]
pub struct Contains {
    values: Vec<String>,
}
impl Contains {
    pub fn new<T: Into<Value>>(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|value| scalar(value.into()))
                .collect(),
        }
    }
}
impl std::fmt::Display for Contains {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "contains:{}", parameters(&self.values))
    }
}
impl CompilableRules for Contains {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        _: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        RuleSet::new().field(attribute, [Rule::parse(&self.to_string())?])
    }
}
impl From<Contains> for Rule {
    fn from(rule: Contains) -> Self {
        Self::compilable(rule)
    }
}
#[derive(Clone)]
pub struct DoesntContain {
    values: Vec<String>,
}
impl DoesntContain {
    pub fn new<T: Into<Value>>(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|value| scalar(value.into()))
                .collect(),
        }
    }
}
impl std::fmt::Display for DoesntContain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "doesnt_contain:{}", parameters(&self.values))
    }
}
impl CompilableRules for DoesntContain {
    fn compile(
        &self,
        attribute: &str,
        _: Option<&Value>,
        _: &serde_json::Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        RuleSet::new().field(attribute, [Rule::parse(&self.to_string())?])
    }
}
impl From<DoesntContain> for Rule {
    fn from(rule: DoesntContain) -> Self {
        Self::compilable(rule)
    }
}
