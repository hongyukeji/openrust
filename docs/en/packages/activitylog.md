# Activity Log (Extension)

## Installation and configuration {#configuration}

Enable the application `activitylog` feature, or add `openrust-activitylog` and use package discovery. `ActivitylogServiceProvider` registers services and migrations without accessing the database during boot. Add `activitylog` to the application `[features].default`, retaining existing entries, then run `./artisan migrate`.

`ACTIVITYLOG_ENABLED` controls the default enabled state; `ACTIVITYLOG_BUFFER_ENABLED` controls `buffer.enabled` (default false). `include_soft_deleted_subjects` defaults to false. Invalid boolean values fail configuration loading; a configuration cache keeps its saved values.

Publish configuration with `./artisan vendor:publish --tag=activitylog-config`, register its module and configuration group in the application loader, and rebuild. Repeated publication preserves customizations; `--force` overwrites them. Defaults are `enabled: true`, `default_log_name: "default"`, `default_auth_driver: null`, `connection: null`, and `clean_after_days: 365`. Log names may be null. Configuration cache uses the saved snapshot. Restart persistent processes after changing configuration.

For `activitylog.connection`, explicitly register `ActivitylogServiceProvider::migrations()` on that connection. The provider skips default-connection migration registration.

## Recording activities {#recording}

`ActivityLogger` provides fluent recording; `ActivityRepository` handles queries, transaction writes and cleanup. Use trusted identities for subjects and causers. Explicit properties are retained by default; the application chooses which data may be logged.

The upstream Support namespace is available as `openrust_activitylog::support::{ActivityLogger, PendingActivityLog, ChangeDetector}`; crate-root imports remain available.

```rust
use openrust_activitylog::{ActivityLogger, CauserResolver, Subject};
use serde_json::json;

let mut logger = ActivityLogger::new(connection.clone());
logger.setCauserResolver(CauserResolver::new(Some(
    Subject::new("user", authenticated_user_id)?,
)));
let record = logger.on(Subject::new("settings", "1")?)
    .inLog("settings").event("updated")
    .withProperties(json!({"module": "site"}))
    .withChanges(json!({"old": {"enabled": false}, "attributes": {"enabled": true}}))
    .log("settings.updated").await?;
```

After a successful write, the same logger resets its subject, event, properties, timestamp and explicit causer. Subsequent entries use the default log name and causer. `byAnonymous()` explicitly selects anonymity; `by(None)` preserves default resolution. `tap(callback, event)` resolves the default causer before invoking the callback, which may modify pending data. `createdAt` accepts `jiff::Timestamp`.

`onModel(&model)` and `byModel(&model)?` capture the model’s actual morph identity and attributes for placeholders such as `:subject.name` and `:causer.name`. Within an application scope, `byId(id).await?` looks up the user through the configured authentication provider; an unknown ID fails without logging.

## Scopes and model events {#model-events}

`activity()` and `facades::Activitylog::logger()` return fresh loggers. HTTP requests, Artisan commands and queue jobs share status and causer resolution through the native container execution scope. Standalone callers can use `logger.scope(async { ... })` to share an explicitly created context among helpers and model events. `setLogStatus` and `setCauserResolver` accept explicit scope objects; there is no process-global current user.

`ActivityLogStatus` enables or disables logging. `withoutLogging(|| async { ... })` temporarily disables an enabled scope and restores it on completion, error, panic or cancellation. If already disabled, the callback controls its status. `CauserResolver::withCauser` supports nested overrides and restoration. The package resolves the identity already authenticated in the current request, using `default_auth_driver` or the selected/default guard and the model’s actual morph mapping. Resolve authentication through middleware or the guard before logging; synchronous model callbacks cannot perform an unresolved asynchronous authentication lookup. Standalone callers supply an explicit resolver.

Implement `models::concerns::LogsActivity` and register `ModelType::bootLogsActivity(app, repository)` in the application Provider. Return `LogOptions` from `getActivitylogOptions`: `logAll`, `logOnly`, `logExcept`, `logFillable`, `logUnguarded`, `logOnlyDirty`, `dontLogEmptyChanges`, `dontLogIfAttributesChangedOnly`, `useAttributeRawValues`, and a description callback are available. Defaults select no attributes and permit empty change logs. Native Eloquent events are used. Same-connection writes inside an explicit transaction roll back with the business write; another logging connection writes after the business commit.

Each model also implements `Clone`, stores a `models::LoggingState` initialized with `Default::default()`, and returns it from `activitylogState`. `disableLogging()` and `enableLogging()` affect that instance; cloning copies its current switch value. Select related attributes with registered paths such as `owner.name`; the old foreign key is used for the old snapshot, including within an explicit transaction. Register aliases explicitly with native named relationships when the application uses different names.

To use a custom activity model, register `Arc::new(models::ActivityModel::of::<CustomActivity>())` with `app.container().instance_named("application.activity", value)` in the application Provider and set `activitylog.activity_model` to `"application.activity"`. Keep the activity columns and JSON casts; the model metadata chooses the table and optional connection unless `activitylog.connection` overrides it. Ordinary writes use native Eloquent save events. For custom actions, register a `LogActivityAction` or `CleanActivityLogAction` under a native container name and select that name in `actions.log_activity` or `actions.clean_log`. Use `beforeLogging` for log customization and `CleanActivityLogAction::using` for a custom cleaner.

## Transaction boundaries {#transactions}

Prepare an explicit transaction log with `repository.prepare(activity)` and use `PreparedActivity::logIn` inside the business transaction. The supplied transaction selects the database, which must already contain the activity table. Failure or cancellation rolls back the log. `repository.logIn(tx, activity)` combines preparation and writing. Independent `repository.log(activity)` does not join a business transaction. Applications own external notifications and fulfillment.

```rust
use openrust_activitylog::{Activity, ActivityRepository};

let repository = ActivityRepository::new(connection.clone());
let prepared = repository.prepare(Activity::new("settings.updated"))?;
connection.transaction(move |tx| {
    let prepared = prepared.clone();
    Box::pin(async move {
        // Apply the business mutation through tx.table(...).
        if let Some(record) = prepared { record.logIn(tx).await?; }
        Ok(())
    })
}, 1).await?;
```

## Redaction and storage boundaries {#redaction}

Explicit properties are retained without automatic masking. Opt in with `withRedaction(Redaction::new().logProperties(&["module"]).logOnly(&["name"]))`, or enable `activitylog.redaction.enabled` and configure `properties`, `attributes`, and `except`. This policy allows selected top-level fields and recursively masks password, token and key names; `logExcept` adds names to mask. The 16 KiB and 16-level limits apply only with this policy. Arbitrary secrets inside ordinary strings cannot be detected.

The default migration uses an auto-increment key, nullable numeric subject/causer morph keys, nullable JSON `properties` and `attribute_changes`, and native `created_at`/`updated_at` timestamps. `Subject` represents IDs as strings at the API boundary; applications with UUID or string primary keys must customize both morph ID columns before migration. `ActivityRecord::created_at` is UTC Unix milliseconds decoded from storage; default SQL timestamp precision discards subsecond values. The additional nullable `request_id` column supports request correlation. Queries apply no tenant filter; authorize every read. Debug output hides payloads and identities.

## Queries and retention {#queries}

`models::Activity` is an Eloquent model with JSON casts. `getProperty("order.number", default)` reads nested properties (also available on `ActivityRecord`). `subject::<Order>()` and `causer::<User>()` return native typed morph relationships; use them within an application scope. `include_soft_deleted_subjects: true` includes soft-deleted subjects; causers retain their ordinary soft-delete filter.

```rust
let page = repository.query().inLog("settings")
    .forSubject(&Subject::new("settings", "1")?)
    .limit(50).get().await?;
// Pass the final record's id to .before(id) for the next page.
```

Filters include `inLog`, `inLogs`, `forEvent`, `forSubject`, `causedBy`, and `forRequest`, with exact identity matching. Page size is 1–1000, default 50. The package exposes no automatic admin or HTTP interface.

`./artisan activitylog:clean accounts --days=365` deletes expired records in the optional log group; omit `accounts` to clean all groups. Omitted days use `clean_after_days`; days must be a positive integer. Production commands require interactive confirmation or `--force`. `cleanBefore(utc_milliseconds, limit)` rejects future cutoffs. The package does not provide tamper-proof storage.

Enable `activitylog.buffer.enabled` to share one buffer across helpers and model events in each HTTP request, Artisan command or queue attempt. It flushes the captured buffer after the lifecycle completes, including failed responses, commands and attempts. Buffered records have no ID until persistence. Flush is an atomic bulk insert and bypasses individual Activity model save events. Model-event logs enter the buffer only after the business transaction commits; rolled-back transactions or savepoints discard their entries and nested commits preserve logging order. Explicit logger calls without a transaction handle remain independent. Use `PreparedActivity::logIn` for explicit writes that must join the business transaction.

Cancellation before a response, process termination and hard kills cannot guarantee a flush. A hard-timeout worker child flushes locally within its existing deadline. Standalone processes use `ActivityBuffer::new(repository)` and `logger.withBuffer(buffer)` and explicitly await `buffer.flush()`; there is no asynchronous process-exit hook. Failed flushes retain pending records for an explicit retry.
