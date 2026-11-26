use crate::validation::{Rule, rule::Kind};

/// 显式值列表，不通过逗号拼接丢失成员边界。
#[derive(Clone)]
pub struct In {
    values: Vec<String>,
}
impl In {
    pub fn new<T: Into<serde_json::Value>>(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|value| scalar(value.into()))
                .collect(),
        }
    }
    pub fn values(&self) -> &[String] {
        &self.values
    }
}
impl From<In> for Rule {
    fn from(rule: In) -> Self {
        Self(
            Kind::In(rule.values.clone()),
            Some(("in".into(), rule.values)),
        )
    }
}

#[derive(Clone)]
pub struct NotIn {
    values: Vec<String>,
}
impl NotIn {
    pub fn new<T: Into<serde_json::Value>>(values: impl IntoIterator<Item = T>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|value| scalar(value.into()))
                .collect(),
        }
    }
    pub fn values(&self) -> &[String] {
        &self.values
    }
}
impl From<NotIn> for Rule {
    fn from(rule: NotIn) -> Self {
        Self(
            Kind::NotIn(rule.values.clone()),
            Some(("not_in".into(), rule.values)),
        )
    }
}

pub(super) fn scalar(value: serde_json::Value) -> String {
    if let serde_json::Value::Number(number) = &value
        && number.is_f64()
    {
        return crate::support::php_float_string(number.as_f64().expect("已核定浮点值"));
    }
    crate::support::input_scalars::string(value)
}
