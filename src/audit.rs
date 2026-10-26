#![doc = include_str!("audit/README.md")]
//! Explicit audit records with bounded, redacted details and replaceable sinks.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::IpAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    BoxFuture, Error, Plugin, Result,
    core::AppBuilder,
    db::{DatabaseTransaction, Db},
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;
use serde_json::Value;

const MAX_LABEL: usize = 128;
const MAX_DETAILS: usize = 32;
const MAX_DETAIL_STRING: usize = 1024;
const MAX_DETAILS_BYTES: usize = 8192;

#[cfg(feature = "test-util")]
pub mod testing;

/// Result of an explicitly audited operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOutcome {
    /// The operation completed successfully.
    Succeeded,
    /// Authorization or validation denied the operation.
    Denied,
    /// The operation failed before its result was committed.
    Failed,
    /// The enclosing transaction was rolled back.
    RolledBack,
    /// The caller could not determine whether the operation committed.
    Indeterminate,
}

impl AuditOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Denied => "denied",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
            Self::Indeterminate => "indeterminate",
        }
    }
}

/// Neutral actor or object reference, for example `("user", "42")`.
#[derive(Clone, PartialEq, Eq)]
pub struct AuditSubject {
    kind: String,
    id: String,
}

impl AuditSubject {
    /// Creates a reference with bounded kind and identifier fields.
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Result<Self> {
        let kind = kind.into();
        let id = id.into();
        Ok(Self {
            kind: label(&kind, "subject kind")?,
            id: label(&id, "subject ID")?,
        })
    }
    /// Returns the subject kind.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// Returns the subject identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for AuditSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditSubject")
            .field("kind", &self.kind)
            .field("id", &"[REDACTED]")
            .finish()
    }
}

/// Application-supplied audit data. It is validated before reaching a sink.
pub struct AuditEvent {
    actor: Option<AuditSubject>,
    action: String,
    object: AuditSubject,
    outcome: AuditOutcome,
    occurred_at: SystemTime,
    request_id: Option<String>,
    source_ip: Option<IpAddr>,
    details: BTreeMap<String, Value>,
}

impl AuditEvent {
    /// Creates an event with an action, object and outcome; captures the current time.
    pub fn new(
        action: impl Into<String>,
        object: AuditSubject,
        outcome: AuditOutcome,
    ) -> Result<Self> {
        let action = action.into();
        Ok(Self {
            actor: None,
            action: label(&action, "action")?,
            object,
            outcome,
            occurred_at: SystemTime::now(),
            request_id: None,
            source_ip: None,
            details: BTreeMap::new(),
        })
    }
    /// Sets the actor. Omit it for anonymous or system initiated operations.
    #[must_use]
    pub fn actor(mut self, actor: AuditSubject) -> Self {
        self.actor = Some(actor);
        self
    }
    /// Sets an opaque request correlation ID; it is not used for authorization.
    pub fn request_id(mut self, id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        self.request_id = Some(label(&id, "request ID")?);
        Ok(self)
    }
    /// Sets a source address already resolved through the trusted proxy policy.
    #[must_use]
    pub fn source_ip(mut self, ip: IpAddr) -> Self {
        self.source_ip = Some(ip);
        self
    }
    /// Adds a named JSON detail. `AuditPolicy` checks the allowlist during preparation.
    #[must_use]
    pub fn detail(mut self, key: impl Into<String>, value: Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }
}

impl fmt::Debug for AuditEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditEvent")
            .field("actor", &self.actor)
            .field("action", &self.action)
            .field("object", &self.object)
            .field("outcome", &self.outcome)
            .field("occurred_at", &self.occurred_at)
            .field(
                "request_id",
                &self.request_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("source_ip", &self.source_ip)
            .field("details", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Allowlist applied before audit data reaches a sink.
#[derive(Debug, Clone, Default)]
pub struct AuditPolicy {
    allowed_detail_keys: BTreeSet<String>,
}

impl AuditPolicy {
    /// Creates a policy with no permitted details.
    pub fn new() -> Self {
        Self::default()
    }
    /// Allows a bounded scalar detail key; sensitive names remain redacted.
    pub fn allow_detail(mut self, key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        self.allowed_detail_keys.insert(label(&key, "detail key")?);
        Ok(self)
    }
}

/// A validated record. Private fields prevent callers bypassing policy checks.
pub struct AuditRecord {
    actor_kind: Option<String>,
    actor_id: Option<String>,
    action: String,
    object_kind: String,
    object_id: String,
    outcome: AuditOutcome,
    occurred_at_ms: i64,
    request_id: Option<String>,
    source_ip: Option<String>,
    details_json: String,
}

impl AuditRecord {
    /// Returns the recorded action.
    pub fn action(&self) -> &str {
        &self.action
    }
    /// Returns the operation outcome.
    pub fn outcome(&self) -> AuditOutcome {
        self.outcome
    }
    /// Returns sanitized detail JSON.
    pub fn details_json(&self) -> &str {
        &self.details_json
    }
    /// Returns the request correlation ID, if present.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

impl fmt::Debug for AuditRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditRecord")
            .field("actor_kind", &self.actor_kind)
            .field("actor_id", &self.actor_id.as_ref().map(|_| "[REDACTED]"))
            .field("action", &self.action)
            .field("object_kind", &self.object_kind)
            .field("object_id", &"[REDACTED]")
            .field("outcome", &self.outcome)
            .field("occurred_at_ms", &self.occurred_at_ms)
            .field(
                "request_id",
                &self.request_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("source_ip", &self.source_ip)
            .field("details", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Replaceable asynchronous destination for validated audit records.
pub trait AuditSink: Send + Sync + 'static {
    /// Appends a record. Errors propagate; the framework does not silently retry or fall back.
    fn append<'a>(&'a self, record: &'a AuditRecord) -> BoxFuture<'a, Result<()>>;
}

/// Explicit audit service registered in application state.
#[derive(Clone)]
pub struct Audit {
    sink: Arc<dyn AuditSink>,
    policy: AuditPolicy,
}

impl fmt::Debug for Audit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Audit")
    }
}

impl Audit {
    /// Creates the service with a custom sink and data policy.
    pub fn new(sink: impl AuditSink, policy: AuditPolicy) -> Self {
        Self {
            sink: Arc::new(sink),
            policy,
        }
    }
    /// Validates, filters and sanitizes an event without persisting it.
    pub fn prepare(&self, event: AuditEvent) -> Result<AuditRecord> {
        prepare_record(event, &self.policy)
    }
    /// Validates and appends an event through the configured sink.
    pub async fn record(&self, event: AuditEvent) -> Result<()> {
        self.sink.append(&self.prepare(event)?).await
    }
}

/// Registers the built-in database sink or a caller-provided implementation.
pub struct AuditPlugin {
    sink: Option<Arc<dyn AuditSink>>,
    policy: AuditPolicy,
}

impl AuditPlugin {
    /// Registers the database sink; register `DatabasePlugin` before this plugin.
    pub fn database() -> Self {
        Self {
            sink: None,
            policy: AuditPolicy::default(),
        }
    }
    /// Registers a custom public sink.
    pub fn with_sink(sink: impl AuditSink, policy: AuditPolicy) -> Self {
        Self {
            sink: Some(Arc::new(sink)),
            policy,
        }
    }
}

impl Plugin for AuditPlugin {
    fn name(&self) -> &'static str {
        "audit"
    }
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<Audit>().is_some() {
            return Err(Error::custom(
                "audit_service_already_registered",
                "register only one audit plugin per application",
            ));
        }
        let sink: Arc<dyn AuditSink> = match &self.sink {
            Some(sink) => Arc::clone(sink),
            None => Arc::new(DatabaseAuditSink::new(
                app.get::<Db>()
                    .ok_or_else(|| {
                        Error::custom(
                            "audit_database_missing",
                            "AuditPlugin::database requires DatabasePlugin to be registered first",
                        )
                    })?
                    .as_ref()
                    .clone(),
            )),
        };
        app.insert(Audit {
            sink,
            policy: self.policy.clone(),
        });
        Ok(())
    }
}

/// Database-backed sink using the primary connection and configured table resolver.
#[derive(Clone)]
pub struct DatabaseAuditSink {
    db: Db,
}

impl fmt::Debug for DatabaseAuditSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseAuditSink")
    }
}

impl DatabaseAuditSink {
    /// Creates a sink bound to the application's database.
    pub fn new(db: Db) -> Self {
        Self { db }
    }
    /// Inserts within the caller's transaction; does not commit or create a nested transaction.
    pub async fn record_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        record: &AuditRecord,
    ) -> Result<()> {
        insert_record(transaction, &self.db, record).await
    }
    /// Deletes up to `batch_size` records strictly older than `cutoff` from the primary.
    pub async fn purge_before(&self, cutoff: SystemTime, batch_size: u16) -> Result<u64> {
        if batch_size == 0 {
            return Err(Error::BadRequest(
                "audit purge batch size must be positive".into(),
            ));
        }
        let cutoff = timestamp_ms(cutoff)?;
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let table = self.db.quoted_table_name("audit_records")?;
        let sql = match backend {
            DbBackend::Postgres => {
                format!("SELECT id FROM {table} WHERE occurred_at_ms < $1 ORDER BY id LIMIT $2")
            }
            DbBackend::MySql => {
                format!("SELECT id FROM {table} WHERE occurred_at_ms < ? ORDER BY id LIMIT ?")
            }
            _ => return Err(unsupported_backend()),
        };
        let rows = connection
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [cutoff.into(), i64::from(batch_size).into()],
            ))
            .await
            .map_err(Error::internal)?;
        let ids = rows
            .iter()
            .map(|row| row.try_get::<i64>("", "id").map_err(Error::internal))
            .collect::<Result<Vec<_>>>()?;
        if ids.is_empty() {
            return Ok(0);
        }
        let params = ids
            .iter()
            .enumerate()
            .map(|(i, _)| match backend {
                DbBackend::Postgres => format!("${}", i + 1),
                _ => "?".into(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM {table} WHERE id IN ({params})");
        Ok(connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                ids.into_iter().map(Into::into),
            ))
            .await
            .map_err(Error::internal)?
            .rows_affected())
    }
}

impl AuditSink for DatabaseAuditSink {
    fn append<'a>(&'a self, record: &'a AuditRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            insert_record(&connection, &self.db, record).await
        })
    }
}

/// Creates the audit table using the application's table-name resolver.
pub struct DatabaseAuditMigration;

impl MigrationName for DatabaseAuditMigration {
    fn name(&self) -> &'static str {
        "m20261026_000004_openrust_audit_records"
    }
}

#[derive(DeriveIden)]
#[allow(dead_code)]
enum AuditTable {
    Table,
    Id,
    ActorKind,
    ActorId,
    Action,
    ObjectKind,
    ObjectId,
    Outcome,
    OccurredAtMs,
    RequestId,
    SourceIp,
    Details,
}

#[async_trait::async_trait]
impl MigrationTrait for DatabaseAuditMigration {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        manager
            .create_table(
                Table::create()
                    .table(migration_table("audit_records", backend)?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AuditTable::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AuditTable::ActorKind).string_len(128).null())
                    .col(ColumnDef::new(AuditTable::ActorId).string_len(128).null())
                    .col(
                        ColumnDef::new(AuditTable::Action)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuditTable::ObjectKind)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuditTable::ObjectId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuditTable::Outcome)
                            .string_len(24)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AuditTable::OccurredAtMs)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuditTable::RequestId).string_len(128).null())
                    .col(ColumnDef::new(AuditTable::SourceIp).string_len(45).null())
                    .col(ColumnDef::new(AuditTable::Details).text().not_null())
                    .to_owned(),
            )
            .await
    }
    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(migration_table(
                        "audit_records",
                        manager.get_database_backend(),
                    )?)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

fn migration_table(logical: &str, backend: DbBackend) -> std::result::Result<Alias, DbErr> {
    let names = crate::db::DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration("audit migration requires MigrationPlugin or TestDatabase scope".into())
    })?;
    names
        .quote(logical, backend)
        .map_err(|e| DbErr::Migration(e.to_string()))?;
    names
        .resolve(logical)
        .map(Alias::new)
        .map_err(|e| DbErr::Migration(e.to_string()))
}

async fn insert_record<C: ConnectionTrait>(
    connection: &C,
    db: &Db,
    record: &AuditRecord,
) -> Result<()> {
    let backend = connection.get_database_backend();
    let table = db.quoted_table_name("audit_records")?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (actor_kind, actor_id, action, object_kind, object_id, outcome, occurred_at_ms, request_id, source_ip, details) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (actor_kind, actor_id, action, object_kind, object_id, outcome, occurred_at_ms, request_id, source_ip, details) VALUES (?,?,?,?,?,?,?,?,?,?)"
        ),
        _ => return Err(unsupported_backend()),
    };
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            [
                record.actor_kind.clone().into(),
                record.actor_id.clone().into(),
                record.action.clone().into(),
                record.object_kind.clone().into(),
                record.object_id.clone().into(),
                record.outcome.as_str().into(),
                record.occurred_at_ms.into(),
                record.request_id.clone().into(),
                record.source_ip.clone().into(),
                record.details_json.clone().into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

fn prepare_record(event: AuditEvent, policy: &AuditPolicy) -> Result<AuditRecord> {
    let occurred_at_ms = timestamp_ms(event.occurred_at)?;
    if event.details.len() > MAX_DETAILS {
        return Err(Error::BadRequest("too many audit detail fields".into()));
    }
    let mut details = BTreeMap::new();
    for (key, value) in event.details {
        let key = label(&key, "detail key")?;
        if !policy.allowed_detail_keys.contains(&key) {
            return Err(Error::BadRequest(
                "audit detail key is not allowed by policy".into(),
            ));
        }
        let value = if sensitive(&key) {
            Value::String("[REDACTED]".into())
        } else {
            scalar(value)?
        };
        details.insert(key, value);
    }
    let details_json = serde_json::to_string(&details).map_err(Error::internal)?;
    if details_json.len() > MAX_DETAILS_BYTES {
        return Err(Error::BadRequest(
            "audit details exceed the size limit".into(),
        ));
    }
    Ok(AuditRecord {
        actor_kind: event.actor.as_ref().map(|v| v.kind.clone()),
        actor_id: event.actor.map(|v| v.id),
        action: event.action,
        object_kind: event.object.kind,
        object_id: event.object.id,
        outcome: event.outcome,
        occurred_at_ms,
        request_id: event.request_id,
        source_ip: event.source_ip.map(|ip| ip.to_string()),
        details_json,
    })
}

fn label(value: &str, name: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_LABEL || value.chars().any(char::is_control) {
        return Err(Error::BadRequest(format!("invalid audit {name}")));
    }
    Ok(value.to_owned())
}
fn scalar(value: Value) -> Result<Value> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(value),
        Value::String(v) if v.len() <= MAX_DETAIL_STRING => Ok(Value::String(v)),
        _ => Err(Error::BadRequest(
            "audit detail values must be bounded scalars".into(),
        )),
    }
}
fn sensitive(key: &str) -> bool {
    let key = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "password",
        "passwd",
        "token",
        "secret",
        "credential",
        "code",
        "authorization",
    ]
    .iter()
    .any(|part| key.contains(part))
}
fn timestamp_ms(value: SystemTime) -> Result<i64> {
    <i64 as TryFrom<u128>>::try_from(
        value
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::BadRequest("audit timestamp predates Unix epoch".into()))?
            .as_millis(),
    )
    .map_err(|_| Error::BadRequest("audit timestamp exceeds supported range".into()))
}
fn unsupported_backend() -> Error {
    Error::custom(
        "audit_database_backend_unsupported",
        "database audit supports PostgreSQL and MySQL connections",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Clone, Default)]
    struct MemorySink {
        records: Arc<Mutex<Vec<String>>>,
        fail: Arc<AtomicBool>,
    }
    impl AuditSink for MemorySink {
        fn append<'a>(&'a self, record: &'a AuditRecord) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::internal(std::io::Error::other("sink unavailable")));
                }
                self.records
                    .lock()
                    .map_err(|_| Error::internal(std::io::Error::other("memory sink poisoned")))?
                    .push(record.details_json().to_owned());
                Ok(())
            })
        }
    }
    fn event() -> Result<AuditEvent> {
        AuditEvent::new(
            "account.password_reset",
            AuditSubject::new("account", "private-id")?,
            AuditOutcome::Succeeded,
        )?
        .detail("changed", Value::Bool(true))
        .detail("password", Value::String("private-secret".into()))
        .request_id("req-private")
    }
    #[test]
    fn policy_filters_redacts_and_hides_debug_values() -> Result<()> {
        let audit = Audit::new(
            MemorySink::default(),
            AuditPolicy::new()
                .allow_detail("changed")?
                .allow_detail("password")?,
        );
        let record = audit.prepare(event()?)?;
        assert_eq!(
            record.details_json(),
            r#"{"changed":true,"password":"[REDACTED]"}"#
        );
        let debug = format!("{record:?}");
        assert!(!debug.contains("private-id"));
        assert!(!debug.contains("req-private"));
        assert!(!debug.contains("private-secret"));
        Ok(())
    }
    #[tokio::test]
    async fn sink_errors_propagate_without_fallback() -> Result<()> {
        let sink = MemorySink::default();
        sink.fail.store(true, Ordering::SeqCst);
        let audit = Audit::new(
            sink.clone(),
            AuditPolicy::new()
                .allow_detail("changed")?
                .allow_detail("password")?,
        );
        assert!(audit.record(event()?).await.is_err());
        assert!(
            sink.records
                .lock()
                .map_err(|_| Error::internal(std::io::Error::other("memory sink poisoned")))?
                .is_empty()
        );
        Ok(())
    }
    #[test]
    fn details_are_allowlisted_and_bounded() -> Result<()> {
        let audit = Audit::new(MemorySink::default(), AuditPolicy::default());
        assert!(audit.prepare(event()?).is_err());
        let event = AuditEvent::new(
            "account.update",
            AuditSubject::new("account", "1")?,
            AuditOutcome::Succeeded,
        )?
        .detail("large", Value::String("x".repeat(MAX_DETAIL_STRING + 1)));
        assert!(
            Audit::new(
                MemorySink::default(),
                AuditPolicy::new().allow_detail("large")?
            )
            .prepare(event)
            .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn plugin_registers_the_database_sink_service() -> Result<()> {
        let mut builder = crate::AppBuilder::new(
            crate::config::FrameworkConfig::default(),
            figment::Figment::new(),
        );
        builder
            .insert(Db::default())
            .plugin(AuditPlugin::database());
        let app = crate::lifecycle::finalize(builder).await?;
        assert!(app.state().get::<Audit>().is_some());
        Ok(())
    }

    #[tokio::test]
    async fn plugin_registers_a_custom_sink() -> Result<()> {
        let mut builder = crate::AppBuilder::new(
            crate::config::FrameworkConfig::default(),
            figment::Figment::new(),
        );
        builder.plugin(AuditPlugin::with_sink(
            MemorySink::default(),
            AuditPolicy::default(),
        ));
        let app = crate::lifecycle::finalize(builder).await?;
        assert!(app.state().get::<Audit>().is_some());
        Ok(())
    }

    #[cfg(feature = "test-util")]
    #[tokio::test]
    async fn public_fake_captures_only_prepared_records_and_injects_failures() -> Result<()> {
        let sink = testing::FakeAuditSink::new();
        let audit = Audit::new(
            sink.clone(),
            AuditPolicy::new()
                .allow_detail("changed")?
                .allow_detail("password")?,
        );
        audit.record(event()?).await?;
        let record = sink.records()?.pop().ok_or_else(|| {
            Error::internal(std::io::Error::other("fake audit record was not captured"))
        })?;
        assert_eq!(record.action(), "account.password_reset");
        assert_eq!(
            record.details_json(),
            r#"{"changed":true,"password":"[REDACTED]"}"#
        );
        assert!(!format!("{record:?}").contains("private-secret"));
        sink.fail_appends(true);
        assert!(audit.record(event()?).await.is_err());
        assert_eq!(sink.records()?.len(), 1);
        Ok(())
    }
}
