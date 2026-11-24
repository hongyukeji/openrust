//! 日志准备与脱敏策略；数据库映射及持久化由 Repository 处理。
use crate::{Activity, ActivityRecord, Error, Result};
use std::collections::BTreeSet;

/// Top-level property and changed-attribute allowlists. Nothing is recorded by default.
/// Secret-shaped keys are redacted recursively even when explicitly allowed.
#[derive(Clone, Default)]
pub struct Redaction {
    properties: BTreeSet<String>,
    attributes: BTreeSet<String>,
    excluded: BTreeSet<String>,
}
impl Redaction {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn log_properties(mut self, keys: &[&str]) -> Self {
        self.properties.extend(keys.iter().map(|s| (*s).into()));
        self
    }
    pub fn log_only(mut self, attributes: &[&str]) -> Self {
        self.attributes
            .extend(attributes.iter().map(|s| (*s).into()));
        self
    }
    pub fn log_except(mut self, attributes: &[&str]) -> Self {
        self.excluded
            .extend(attributes.iter().map(|s| normalize(s)));
        self
    }
    fn clean(
        &self,
        value: &serde_json::Value,
        allowed: &BTreeSet<String>,
    ) -> Result<serde_json::Value> {
        let object = value.as_object().ok_or(Error::InvalidActivity)?;
        let mut clean = serde_json::Map::new();
        for (key, value) in object {
            if allowed.contains(key) {
                clean.insert(key.clone(), self.redact(key, value, 0)?);
            }
        }
        Ok(clean.into())
    }
    fn redact(
        &self,
        key: &str,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<serde_json::Value> {
        if depth > 16 {
            return Err(Error::InvalidActivity);
        }
        let normalized = normalize(key);
        if self.excluded.contains(&normalized)
            || [
                "password",
                "secret",
                "token",
                "authorization",
                "cookie",
                "privatekey",
                "apikey",
                "sessionkey",
                "credential",
                "otp",
                "verificationcode",
            ]
            .iter()
            .any(|s| normalized.contains(s))
        {
            return Ok("[REDACTED]".into());
        }
        match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(k, v)| Ok((k.clone(), self.redact(k, v, depth + 1)?)))
                .collect::<Result<serde_json::Map<_, _>>>()
                .map(Into::into),
            serde_json::Value::Array(values) => values
                .iter()
                .map(|v| self.redact("", v, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Into::into),
            _ => Ok(value.clone()),
        }
    }
}
fn normalize(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn prepare(
    activity: Activity,
    enabled: bool,
    policy: Option<&Redaction>,
) -> Result<Option<ActivityRecord>> {
    activity.validate()?;
    if !enabled {
        return Ok(None);
    }
    let (properties, changes) = if let Some(policy) = policy {
        let properties = policy.clean(&activity.properties, &policy.properties)?;
        let mut changes = serde_json::Map::new();
        for (key, value) in activity.changes.as_object().ok_or(Error::InvalidActivity)? {
            changes.insert(key.clone(), policy.clean(value, &policy.attributes)?);
        }
        (properties, changes.into())
    } else {
        (activity.properties.clone(), activity.changes.clone())
    };
    let record = ActivityRecord {
        id: None,
        log_name: activity.log_name,
        description: activity.description.unwrap_or_default(),
        event: activity.event,
        subject: activity.subject,
        causer: activity.causer,
        request_id: activity.request_id,
        properties,
        attribute_changes: changes,
        created_at: activity
            .created_at
            .unwrap_or_else(|| jiff::Timestamp::now().as_millisecond()),
    };
    if policy.is_some()
        && serde_json::to_vec(&record)
            .map_err(|_| Error::InvalidActivity)?
            .len()
            > 16_384
    {
        return Err(Error::InvalidActivity);
    }
    Ok(Some(record))
}
