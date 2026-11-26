use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    pub kind: String,
    pub id: String,
}
impl Subject {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Result<Self> {
        let subject = Self {
            kind: kind.into(),
            id: id.into(),
        };
        subject.validate()?;
        Ok(subject)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if !label(&self.kind, 255) || !label(&self.id, 255) {
            return Err(Error::InvalidActivity);
        }
        Ok(())
    }
}
impl std::fmt::Debug for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subject")
            .field("kind", &self.kind)
            .field("id", &"[REDACTED]")
            .finish()
    }
}
pub(crate) fn label(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

/// Explicit operation data. Supply trusted identities and a fixed description;
/// put user-supplied values in properties subject to the logger's allowlist.
#[derive(Clone)]
pub struct Activity {
    pub log_name: Option<String>,
    pub event: Option<String>,
    pub subject: Option<Subject>,
    pub causer: Option<Subject>,
    pub properties: Value,
    pub changes: Value,
    pub request_id: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<i64>,
    pub subject_attributes: Value,
    pub causer_attributes: Value,
}
impl Activity {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            log_name: Some("default".into()),
            event: None,
            subject: None,
            causer: None,
            properties: serde_json::json!({}),
            changes: serde_json::json!({}),
            request_id: None,
            description: Some(description.into()),
            created_at: None,
            subject_attributes: serde_json::json!({}),
            causer_attributes: serde_json::json!({}),
        }
    }
    pub fn use_log(mut self, name: impl IntoLogName) -> Self {
        self.log_name = name.into_log_name();
        self
    }
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }
    pub fn performed_on(mut self, subject: Subject) -> Self {
        self.subject = Some(subject);
        self
    }
    pub fn caused_by(mut self, causer: Subject) -> Self {
        self.causer = Some(causer);
        self
    }
    pub fn caused_by_anonymous(mut self) -> Self {
        self.causer = None;
        self
    }
    pub fn with_properties(mut self, properties: Value) -> Self {
        self.properties = properties;
        self
    }
    /// Attribute changes are stored as {"old": ..., "attributes": ...}.
    pub fn with_changes(mut self, old: Value, attributes: Value) -> Self {
        self.changes = serde_json::json!({"old":old,"attributes":attributes});
        self
    }
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.log_name.as_ref().is_some_and(|name| name.len() > 255)
            || self.event.as_deref().is_some_and(|v| !label(v, 255))
            || self.request_id.as_deref().is_some_and(|v| !label(v, 255))
            || !self.properties.is_object()
            || !self.changes.is_object()
        {
            return Err(Error::InvalidActivity);
        }
        for subject in [&self.subject, &self.causer].into_iter().flatten() {
            subject.validate()?;
        }
        Ok(())
    }
}
impl std::fmt::Debug for Activity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Activity")
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

/// Sanitized persisted data. Query authorization is the application's responsibility.
#[derive(Clone, Serialize, Deserialize)]
pub struct ActivityRecord {
    pub id: Option<i64>,
    pub log_name: Option<String>,
    pub description: String,
    pub event: Option<String>,
    pub subject: Option<Subject>,
    pub causer: Option<Subject>,
    pub request_id: Option<String>,
    pub properties: Value,
    pub attribute_changes: Value,
    /// UTC Unix milliseconds decoded from native SQL timestamps (whole seconds by default).
    pub created_at: i64,
}
impl ActivityRecord {
    pub fn get_property(&self, path: &str, default: Value) -> Value {
        crate::models::property(&self.properties, path)
            .cloned()
            .unwrap_or(default)
    }
}
impl std::fmt::Debug for ActivityRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityRecord")
            .field("id", &self.id)
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

/// Accepts a name or an explicit null log name, as in the upstream API.
pub trait IntoLogName {
    fn into_log_name(self) -> Option<String>;
}
impl IntoLogName for String {
    fn into_log_name(self) -> Option<String> {
        Some(self)
    }
}
impl IntoLogName for &str {
    fn into_log_name(self) -> Option<String> {
        Some(self.into())
    }
}
impl IntoLogName for Option<String> {
    fn into_log_name(self) -> Option<String> {
        self
    }
}
