# Audit

`openrust::audit` records explicit security and business events through a replaceable `AuditSink`. The built-in `DatabaseAuditSink` stores records in the application's primary database. Add `DatabaseAuditMigration` to the application migrator; migrations are never run automatically.

Events contain an optional actor, action, object, outcome, timestamp, optional request ID and trusted source IP, and explicitly supplied details. Use stable, non-secret IDs for actors and objects; do not use email addresses, credentials, or user-provided free text as identifiers. `AuditPolicy` denies details by default and allowlists bounded scalar fields. Sensitive detail names are stored as `[REDACTED]`. Event and record debug output hides identifiers and detail values. Do not pass passwords, tokens, verification codes, raw requests/responses, SQL, or arbitrary error text as details.

```rust,no_run
use openrust::audit::{Audit, AuditEvent, AuditOutcome, AuditSubject};

async fn record_change(audit: &Audit, actor_id: i64, account_id: i64) -> openrust::Result<()> {
    audit.record(AuditEvent::new(
        "account.updated",
        AuditSubject::new("account", account_id.to_string())?,
        AuditOutcome::Succeeded,
    )?.actor(AuditSubject::new("user", actor_id.to_string())?)).await
}
```

For a critical database mutation, prepare the sanitized record first, then call `DatabaseAuditSink::record_in_transaction` with the same `DatabaseTransaction` as the business change. The outer transaction controls commit. If either write fails, propagate the error so both roll back. Records for denied, failed, or rolled-back operations must be written after the failed transaction ends. A post-commit sink error does not undo the business change.

Custom sinks implement the public `AuditSink` trait and can be registered with `AuditPlugin::with_sink`. Tests can use `openrust::audit::testing::FakeAuditSink` with the `test-util` feature. Sink failures propagate; no fallback, automatic retry, deduplication, or exactly-once guarantee is provided. The application owns audit query authorization and retention policy. `purge_before` removes at most the requested batch from the primary. The framework does not promise tamper-proof storage or legal compliance.
