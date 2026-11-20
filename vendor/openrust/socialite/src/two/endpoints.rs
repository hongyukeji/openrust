use super::abstract_provider::json;
use crate::{Error, Result};
use serde_json::Value;

/// Explicit endpoint overrides for self-hosted or controlled protocol services.
#[derive(Clone)]
pub struct ProviderEndpoints {
    pub authorize: String,
    pub token: String,
    pub user: String,
    pub email: Option<String>,
}
impl ProviderEndpoints {
    pub fn new(
        authorize: impl Into<String>,
        token: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            authorize: authorize.into(),
            token: token.into(),
            user: user.into(),
            email: None,
        }
    }
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }
}
pub(super) fn value(raw: &serde_json::Value, path: &[&str]) -> serde_json::Value {
    path.iter()
        .try_fold(raw, |raw, k| match raw {
            serde_json::Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get(i)),
            _ => raw.get(*k),
        })
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}
pub(super) fn mapped(
    raw: serde_json::Value,
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> super::User {
    super::User::from_attributes(
        raw,
        fields.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    )
}

pub(super) fn checked(response: openrust::http::client::Response) -> Result<Value> {
    let value = json(response)?;
    if !value.is_object()
        || value.get("error").is_some_and(|v| !v.is_null())
        || value.get("errcode").is_some_and(|v| v.as_i64() != Some(0))
        || value.get("ret").is_some_and(|v| v.as_i64() != Some(0))
    {
        return Err(Error::InvalidResponse);
    }
    Ok(value)
}
pub(super) fn identifier<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or(Error::InvalidResponse)
}
