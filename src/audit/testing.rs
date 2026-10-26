//! Fake audit sink for application and integration tests.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::{AuditOutcome, AuditRecord, AuditSink};
use crate::{BoxFuture, Error, Result};

/// In-memory sink that captures sanitized records and can simulate a write failure.
#[derive(Clone, Default)]
pub struct FakeAuditSink {
    records: Arc<Mutex<Vec<FakeAuditRecord>>>,
    fail: Arc<AtomicBool>,
}

impl std::fmt::Debug for FakeAuditSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FakeAuditSink")
    }
}

impl FakeAuditSink {
    /// Creates an empty sink that accepts appended records.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of captured sanitized records.
    pub fn records(&self) -> Result<Vec<FakeAuditRecord>> {
        self.records
            .lock()
            .map(|records| records.clone())
            .map_err(|_| Error::internal(std::io::Error::other("fake audit sink lock poisoned")))
    }

    /// Makes future appends fail, for testing sink error handling.
    pub fn fail_appends(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl AuditSink for FakeAuditSink {
    fn append<'a>(&'a self, record: &'a AuditRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::internal(std::io::Error::other(
                    "fake audit sink failure",
                )));
            }
            let captured = FakeAuditRecord {
                actor_kind: record.actor_kind.clone(),
                actor_id: record.actor_id.clone(),
                action: record.action.clone(),
                object_kind: record.object_kind.clone(),
                object_id: record.object_id.clone(),
                outcome: record.outcome,
                request_id: record.request_id.clone(),
                source_ip: record.source_ip.clone(),
                details_json: record.details_json.clone(),
            };
            self.records
                .lock()
                .map_err(|_| {
                    Error::internal(std::io::Error::other("fake audit sink lock poisoned"))
                })?
                .push(captured);
            Ok(())
        })
    }
}

/// Sanitized snapshot captured by [`FakeAuditSink`].
#[derive(Clone, PartialEq, Eq)]
pub struct FakeAuditRecord {
    actor_kind: Option<String>,
    actor_id: Option<String>,
    action: String,
    object_kind: String,
    object_id: String,
    outcome: AuditOutcome,
    request_id: Option<String>,
    source_ip: Option<String>,
    details_json: String,
}

impl std::fmt::Debug for FakeAuditRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeAuditRecord")
            .field("actor_kind", &self.actor_kind)
            .field("actor_id", &self.actor_id.as_ref().map(|_| "[REDACTED]"))
            .field("action", &self.action)
            .field("object_kind", &self.object_kind)
            .field("object_id", &"[REDACTED]")
            .field("outcome", &self.outcome)
            .field(
                "request_id",
                &self.request_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("source_ip", &self.source_ip)
            .field("details", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl FakeAuditRecord {
    /// Returns the actor kind when an actor was supplied.
    pub fn actor_kind(&self) -> Option<&str> {
        self.actor_kind.as_deref()
    }
    /// Returns the actor ID when an actor was supplied.
    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }
    /// Returns the recorded action.
    pub fn action(&self) -> &str {
        &self.action
    }
    /// Returns the object kind.
    pub fn object_kind(&self) -> &str {
        &self.object_kind
    }
    /// Returns the object ID.
    pub fn object_id(&self) -> &str {
        &self.object_id
    }
    /// Returns the recorded outcome.
    pub fn outcome(&self) -> AuditOutcome {
        self.outcome
    }
    /// Returns the request correlation ID.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    /// Returns the trusted source IP string.
    pub fn source_ip(&self) -> Option<&str> {
        self.source_ip.as_deref()
    }
    /// Returns details after policy filtering and redaction.
    pub fn details_json(&self) -> &str {
        &self.details_json
    }
}
