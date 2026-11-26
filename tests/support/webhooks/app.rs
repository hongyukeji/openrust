#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
mod mass_pruning;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;
mod model_pruning;
mod outbound;
#[path = "../../../bootstrap/packages.rs"]
mod packages;
mod receipt_diagnostics;
use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    container::ServiceKey,
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Facade, Route},
};
use openrust_webhook_client::{
    DefaultRespondsTo, DefaultSignatureValidator, RespondsToWebhook, SignatureValidator,
    StandardSignatureValidator, WebhookConfigRepository, WebhookRequest,
};
use openrust_webhook_server::{DefaultSigner, Signer, SigningContext, StandardWebhookSigner};
use serde_json::json;
use std::{collections::BTreeMap, process::ExitCode};
struct Configs;
impl Facade for Configs {
    type Service = WebhookConfigRepository;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<WebhookConfigRepository>()
    }
}
fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
fn verify(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request.route("profile").unwrap_or_default().to_owned();
        let config = Configs::get_facade_root()
            .map_err(internal)?
            .get_config(&name)
            .map_err(internal)?
            .ok_or(HttpError::NotFound)?;
        let uri = request.uri().clone();
        let method = request.method().clone();
        let headers = request.headers().clone();
        let body = axum::body::to_bytes(request.take_body(), config.max_body_bytes())
            .await
            .map_err(|_| HttpError::PayloadTooLarge)?;
        let snapshot = WebhookRequest::new(method, headers, body.to_vec())
            .map_err(internal)?
            .with_uri(uri);
        let validator: &dyn SignatureValidator = if name == "standard" {
            &StandardSignatureValidator
        } else {
            &DefaultSignatureValidator
        };
        if !validator.is_valid(&snapshot, &config).map_err(internal)? {
            return Response::json(StatusCode::FORBIDDEN, json!({"verified":false}));
        }
        // 本夹具只验证公开签名与响应契约，不作为可靠接收端点发布。
        DefaultRespondsTo
            .respond_to_valid_webhook(&snapshot, &config)
            .map_err(internal)
    })
}
struct Databases;
impl Facade for Databases {
    type Service = openrust::database::DatabaseManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<openrust::database::DatabaseManager>()
    }
}
struct ReceiveJob {
    fail: bool,
}
impl openrust_webhook_client::WebhookJobDispatcher for ReceiveJob {
    fn dispatch<'a>(
        &'a self,
        call: &'a openrust_webhook_client::models::WebhookCall,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            if self.fail {
                return Err(openrust_webhook_client::Error::InvalidConfig(
                    "fixture dispatch failure",
                ));
            }
            openrust::support::facades::Artisan::queue(
                "webhooks:record",
                &[call.id().into(), call.scope().into()],
            )
            .on_connection("database")
            .on_queue("webhook-receipts")
            .dispatch()
            .await
            .map_err(|source| openrust_webhook_client::Error::Runtime {
                source: Box::new(source),
            })?;
            Ok(())
        })
    }
}
struct Ignore;
impl openrust_webhook_client::WebhookProfile for Ignore {
    fn should_process(
        &self,
        _: &WebhookRequest,
        _: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        Ok(false)
    }
}
struct FixtureRateFilter;
impl openrust_webhook_client::WebhookProfile for FixtureRateFilter {
    fn should_process(
        &self,
        request: &WebhookRequest,
        _: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        Ok(request.body() != b"ignore-for-quota")
    }
}
fn receive(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request.route("profile").unwrap_or_default().to_owned();
        let config = Configs::get_facade_root()
            .map_err(internal)?
            .get_config(&name)
            .map_err(internal)?
            .ok_or(HttpError::NotFound)?;
        let uri = request.uri().clone();
        let method = request.method().clone();
        let headers = request.headers().clone();
        let bytes = axum::body::to_bytes(request.take_body(), config.max_body_bytes())
            .await
            .map_err(|_| HttpError::PayloadTooLarge)?;
        let snapshot = WebhookRequest::new(method, headers, bytes.to_vec())
            .map_err(internal)?
            .with_uri(uri);
        let store = std::sync::Arc::new(
            openrust_webhook_client::DatabaseWebhookStore::new(
                Databases::get_facade_root()
                    .map_err(internal)?
                    .connection(None)
                    .map_err(internal)?,
                "webhook-acceptance",
            )
            .map_err(internal)?,
        );
        let mut runtime = openrust_webhook_client::WebhookRuntime::new(
            store,
            std::sync::Arc::new(ReceiveJob {
                fail: name == "dispatch-fails",
            }),
            openrust::support::facades::Event::dispatcher().map_err(internal)?,
        );
        if name == "standard" {
            runtime =
                runtime.with_signature_validator(std::sync::Arc::new(StandardSignatureValidator));
        }
        if name == "ignored" {
            runtime = runtime.with_webhook_profile(std::sync::Arc::new(Ignore));
        }
        if name == "verified-other" {
            runtime = runtime.with_verified_rate_limiter(
                openrust::support::facades::RateLimiter::get_facade_root().map_err(internal)?,
            );
        }
        let config = config
            .as_ref()
            .clone()
            .with_store_headers(&["X-Source", "X-Duplicate", "X-Opaque"])
            .map_err(internal)?;
        match openrust_webhook_client::WebhookProcessor::new(&snapshot, &config, &runtime)
            .process()
            .await
        {
            Err(openrust_webhook_client::Error::InvalidSignature) => {
                Response::json(StatusCode::FORBIDDEN, json!({"verified":false}))
            }
            Err(openrust_webhook_client::Error::Http(error)) => Err(error),
            other => other.map_err(internal),
        }
    })
}
fn attachment_json(value: openrust::http::UploadedFileValue) -> serde_json::Value {
    use openrust::http::UploadedFileValue;
    match value {
        UploadedFileValue::Null => serde_json::Value::Null,
        UploadedFileValue::File(file) => json!({
            "name": file.get_client_original_name(),
            "mime": file.get_client_mime_type(),
            "bytes": file.get().as_ref(),
        }),
        UploadedFileValue::Files(files) => serde_json::Value::Object(
            files
                .into_iter()
                .map(|(key, file)| (key, attachment_json(file)))
                .collect(),
        ),
    }
}

struct RestoreLegacyAttachments(CommandDefinition);
impl Command for RestoreLegacyAttachments {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let path = input.matches().get_one::<String>("export").unwrap();
            let exported: serde_json::Value = serde_json::from_reader(
                std::fs::File::open(path).map_err(ConsoleError::from_source)?,
            )
            .map_err(ConsoleError::from_source)?;
            let files = openrust_webhook_client::models::WebhookCall::restore_legacy_attachments(
                exported.get("attachments"),
                exported.get("payload"),
            )
            .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{}\n", attachment_json(files)))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct Record(CommandDefinition);
impl Command for Record {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let id = input.matches().get_one::<String>("id").unwrap();
            let scope = input.matches().get_one::<String>("scope").unwrap();
            let store = openrust_webhook_client::DatabaseWebhookStore::new(
                Databases::get_facade_root()
                    .map_err(ConsoleError::from_source)?
                    .connection(None)
                    .map_err(ConsoleError::from_source)?,
                scope,
            )
            .map_err(ConsoleError::from_source)?;
            let mut call = openrust_webhook_client::models::WebhookCall::find(&store, id)
                .await
                .map_err(ConsoleError::from_source)?
                .ok_or_else(|| ConsoleError::new("未找到当前范围的接收记录"))?;
            if let Some(value) = input.matches().get_one::<String>("save-exception") {
                call.save_exception(
                    &store,
                    serde_json::from_str(value).map_err(ConsoleError::from_source)?,
                )
                .await
                .map_err(ConsoleError::from_source)?;
            }
            if input.matches().get_flag("clear-exception") {
                call.clear_exception(&store)
                    .await
                    .map_err(ConsoleError::from_source)?;
            }
            let value = read_receipt(&call, &store)
                .await
                .map_err(ConsoleError::from_source)?;
            write_receipt(application, &value)
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{value}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
struct WakeDeferred(CommandDefinition);
impl Command for WakeDeferred {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let id = input.matches().get_one::<String>("id").unwrap();
            let scope = input.matches().get_one::<String>("scope").unwrap();
            let manager = application
                .make::<openrust::database::DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let queue = application
                .make::<openrust::queue::QueueManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let store = openrust_webhook_client::DatabaseWebhookStore::new(
                manager
                    .connection(None)
                    .map_err(ConsoleError::from_source)?,
                scope,
            )
            .map_err(ConsoleError::from_source)?;
            let woke =
                openrust_webhook_client::jobs::QueuedWebhookJob::wake_deferred(&store, queue, id)
                    .await
                    .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{woke}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
async fn read_receipt(
    call: &openrust_webhook_client::models::WebhookCall,
    store: &dyn openrust_webhook_client::WebhookStore,
) -> openrust_webhook_client::Result<serde_json::Value> {
    let headers: Vec<_> = call
        .headers()
        .iter()
        .map(|(name, value)| json!([name.as_str(), value.as_bytes()]))
        .collect();
    let payload = call.payload(store).await?;
    let attachments = attachment_json(call.get_attachments(store).await?);
    Ok(
        json!({"payload":payload,"attachments":attachments,"id":call.id(),"scope":call.scope(),"name":call.name(),"revision":call.revision(),"url":call.url(),"method":call.method().as_str(),"body":call.raw_body(),"headers":headers,"exception":call.exception()}),
    )
}
async fn write_receipt(
    application: &Application,
    value: &serde_json::Value,
) -> openrust_webhook_client::Result<()> {
    let env = application.make::<openrust::config::Environment>().await?;
    if let Some(path) = env
        .get_utf8("WEBHOOK_TEST_RECEIPT_OUTPUT")
        .map_err(runtime_error)?
    {
        std::fs::write(path, serde_json::to_vec(value).map_err(runtime_error)?)
            .map_err(runtime_error)?;
    }
    Ok(())
}
fn runtime_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> openrust_webhook_client::Error {
    openrust_webhook_client::Error::Runtime {
        source: Box::new(source),
    }
}
struct FixtureJob;
struct FixtureControlledJob(std::sync::Arc<dyn openrust::queue::middleware::JobMiddleware>);
impl openrust_webhook_client::jobs::ProcessWebhookJob for FixtureControlledJob {
    fn middleware(&self) -> Vec<std::sync::Arc<dyn openrust::queue::middleware::JobMiddleware>> {
        vec![self.0.clone()]
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut openrust::queue::JobContext<'_>,
        call: &'a openrust_webhook_client::models::WebhookCall,
        store: &'a dyn openrust_webhook_client::WebhookStore,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            if call.raw_body() == b"hold-worker-lock" {
                let gate = std::path::PathBuf::from(
                    std::env::var_os("FIXTURE_QUEUE_LOCK_BARRIER").expect("队列锁验收目录"),
                );
                tokio::fs::write(gate.join("entered"), b"entered")
                    .await
                    .map_err(runtime_error)?;
                while !gate.join("release").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }
            FixtureJob.handle(context, call, store).await
        })
    }
}

impl openrust_webhook_client::jobs::ProcessWebhookJob for FixtureJob {
    fn handle<'a>(
        &'a self,
        context: &'a mut openrust::queue::JobContext<'_>,
        call: &'a openrust_webhook_client::models::WebhookCall,
        store: &'a dyn openrust_webhook_client::WebhookStore,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            if matches!(
                call.name(),
                "recovery"
                    | "processing-recovery"
                    | "deferred-recovery"
                    | "deferred-recovery-tracked"
            ) {
                let expected = std::env::var("FIXTURE_RECOVERY_QUEUE")
                    .unwrap_or_else(|_| "foreign-database".into());
                assert_eq!(context.connection(), expected);
                assert_eq!(context.queue(), "webhook-recovery");
            } else {
                assert_eq!(context.connection(), "database");
                assert_eq!(context.queue(), "webhook-receipts");
            }
            if (call.raw_body() == b"retry-binary-receipt" && context.attempts() == 1)
                || call.raw_body() == b"terminal-binary-receipt"
            {
                return Err(runtime_error(std::io::Error::other(
                    "private-business-error-sentinel",
                )));
            }
            if call.raw_body() == b"leased-crash"
                && std::env::var_os("FIXTURE_LEASE_CRASH").is_some()
            {
                std::process::exit(79);
            }
            if call.raw_body() == b"leased-release" && context.attempts() == 1 {
                context.release(std::time::Duration::ZERO);
                return Ok(());
            }
            if call.raw_body() == b"leased-stop-on-error" {
                context.release(std::time::Duration::ZERO);
                context.fail_without_retry();
                return Err(runtime_error(std::io::Error::other(
                    "private-terminal-sentinel",
                )));
            }
            if call.raw_body() == b"leased-stop-on-success" {
                context.fail_without_retry();
            }
            if call.raw_body() == b"leased-stale"
                && let Some(marker) = std::env::var_os("FIXTURE_OLD_ENTERED")
            {
                let gate = std::path::PathBuf::from(
                    std::env::var_os("FIXTURE_OLD_GATE").expect("旧 Worker 放行标记"),
                );
                tokio::fs::write(marker, b"entered")
                    .await
                    .map_err(runtime_error)?;
                while !gate.exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                return Err(runtime_error(std::io::Error::other(
                    "old-worker-private-error",
                )));
            }
            if call.raw_body() == b"leased-concurrent" {
                let marker = std::env::var_os("FIXTURE_LEASE_ENTERED").expect("领取验收标记");
                tokio::fs::write(marker, b"entered")
                    .await
                    .map_err(runtime_error)?;
                tokio::time::sleep(std::time::Duration::from_millis(1900)).await;
            }
            let app = openrust::support::facades::App::get_facade_root()?;
            write_receipt(&app, &read_receipt(call, store).await?).await
        })
    }

    fn failed<'a>(
        &'a self,
        context: &'a openrust::queue::JobContext<'_>,
        call: &'a openrust_webhook_client::models::WebhookCall,
        _: &'a dyn openrust_webhook_client::WebhookStore,
        error: &'a openrust::queue::QueueError,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            let Some(path) = std::env::var_os("FIXTURE_WEBHOOK_FAILED_HOOK") else {
                return Ok(());
            };
            let manager = Databases::get_facade_root().map_err(runtime_error)?;
            let failed_jobs = manager
                .connection(None)?
                .table("failed_jobs")
                .use_write_pdo()
                .r#where("payload", "like", format!("%{}%", call.id()))
                .count("*")
                .await?;
            let record = json!({
                "id": call.id(), "scope": call.scope(), "profile": call.name(),
                "queue": context.queue(), "connection": context.connection(),
                "attempts": context.attempts(), "failed_jobs": failed_jobs,
                "exception": call.exception(),
                "redacted_error": !error.to_string().contains("private-"),
                "expected_body": matches!(call.raw_body(), b"leased-stop-on-error" | b"terminal-binary-receipt"),
            });
            tokio::fs::write(path, record.to_string())
                .await
                .map_err(runtime_error)?;
            if call.raw_body() == b"leased-stop-on-error" {
                return Err(runtime_error(std::io::Error::other(
                    "private-failed-hook-sentinel",
                )));
            }
            Ok(())
        })
    }

    fn handle_outcome<'a>(
        &'a self,
        context: &'a mut openrust::queue::JobContext<'_>,
        call: &'a openrust_webhook_client::models::WebhookCall,
        store: &'a dyn openrust_webhook_client::WebhookStore,
    ) -> HttpFuture<
        'a,
        openrust_webhook_client::Result<openrust_webhook_client::jobs::WebhookProcessingOutcome>,
    > {
        Box::pin(async move {
            use openrust_webhook_client::jobs::WebhookProcessingOutcome;
            match call.raw_body() {
                b"recovery-deferred-wake"
                | b"recovery-deferred-expire"
                | b"recovery-deferred-scheduled"
                    if std::env::var_os("FIXTURE_DEFERRED_READY").is_none() =>
                {
                    return Ok(WebhookProcessingOutcome::Deferred { delay_seconds: 60 });
                }
                b"deferred-wait" if std::env::var_os("FIXTURE_DEFERRED_READY").is_none() => {
                    return Ok(WebhookProcessingOutcome::Deferred { delay_seconds: 2 });
                }
                b"deferred-wake" if std::env::var_os("FIXTURE_DEFERRED_READY").is_none() => {
                    return Ok(WebhookProcessingOutcome::Deferred { delay_seconds: 6 });
                }
                b"deferred-exhaust" | b"deferred-expire" => {
                    return Ok(WebhookProcessingOutcome::Deferred { delay_seconds: 2 });
                }
                b"deferred-permanent" => return Ok(WebhookProcessingOutcome::PermanentFailure),
                b"deferred-retryable" => return Ok(WebhookProcessingOutcome::RetryableFailure),
                _ => {}
            }
            self.handle(context, call, store).await?;
            Ok(WebhookProcessingOutcome::Completed)
        })
    }
}
struct AtomicFixtureDispatcher {
    inner: openrust_webhook_client::jobs::QueuedWebhookJob,
    crash: Option<String>,
}
impl openrust_webhook_client::WebhookJobDispatcher for AtomicFixtureDispatcher {
    fn dispatch<'a>(
        &'a self,
        call: &'a openrust_webhook_client::models::WebhookCall,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        self.inner.dispatch(call)
    }

    fn dispatch_in_transaction<'a>(
        &'a self,
        call: &'a openrust_webhook_client::models::WebhookCall,
        transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            if self.crash.as_deref() == Some("before-enqueue") {
                std::process::exit(77);
            }
            self.inner
                .dispatch_in_transaction(call, transaction)
                .await?;
            if self.crash.as_deref() == Some("after-enqueue") {
                std::process::exit(78);
            }
            if self.crash.as_deref() == Some("hold-after-enqueue") {
                let marker = std::env::var_os("FIXTURE_ATOMIC_HELD").expect("入队验收标记");
                tokio::fs::write(marker, b"held")
                    .await
                    .map_err(runtime_error)?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Ok(())
        })
    }
}
struct SignedBodyIdentity;
impl openrust_webhook_client::SignatureValidator for SignedBodyIdentity {
    fn is_valid(
        &self,
        request: &openrust_webhook_client::WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        openrust_webhook_client::DefaultSignatureValidator.is_valid(request, config)
    }

    fn verified_event_id<'a>(
        &self,
        request: &'a openrust_webhook_client::WebhookRequest,
        _: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<Option<std::borrow::Cow<'a, str>>> {
        let body: serde_json::Value =
            serde_json::from_slice(request.body()).map_err(runtime_error)?;
        Ok(body
            .get("event")
            .and_then(serde_json::Value::as_str)
            .map(|id| std::borrow::Cow::Owned(id.to_owned())))
    }
}
struct FixtureHint;
impl openrust_webhook_client::hints::WebhookHintPolicy for FixtureHint {
    fn resolve<'a>(
        &'a self,
        scope: &'a str,
        hint: openrust_webhook_client::hints::UnverifiedHint<'a>,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<Option<String>>> {
        Box::pin(async move {
            assert_eq!(scope, "webhook-acceptance");
            Ok((hint.request().body() == b"known").then(|| "local-task".to_owned()))
        })
    }
}
impl openrust_webhook_client::hints::ProcessWebhookHint for FixtureHint {
    fn is_registered<'a>(
        &'a self,
        hint: &'a openrust_webhook_client::hints::WebhookHint,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<bool>> {
        Box::pin(async move {
            Ok(hint.task_id() == "local-task"
                && hint.scope() == "webhook-acceptance"
                && std::env::var_os("FIXTURE_HINT_REVOKED").is_none())
        })
    }

    fn handle<'a>(
        &'a self,
        hint: &'a openrust_webhook_client::hints::WebhookHint,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            let marker = std::env::var_os("FIXTURE_HINT_MARKER").expect("提示任务验收标记");
            tokio::fs::write(marker, hint.task_id())
                .await
                .map_err(runtime_error)
        })
    }
}

fn fixture_hint_queue(
    manager: &openrust::database::DatabaseManager,
    profile: &str,
) -> openrust_webhook_client::Result<openrust_webhook_client::hints::QueuedWebhookHint> {
    openrust_webhook_client::hints::QueuedWebhookHint::new(
        manager.connection(None)?,
        "webhook-acceptance",
        profile,
        profile,
        "database",
        "webhook-hints",
    )
}

struct RejectAuditListener;
impl openrust::contracts::events::Listener for RejectAuditListener {
    fn handle<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async {
            let marker = std::env::var_os("FIXTURE_AUDIT_LISTENER_FAIL").expect("监听器验收标记");
            tokio::fs::write(marker, b"ran").await.map_err(|source| {
                openrust::events::EventError::Listener {
                    source: Box::new(source),
                }
            })?;
            Err(openrust::events::EventError::Listener {
                source: Box::new(std::io::Error::other("verification-audit-listener-fixture")),
            })
        })
    }
}
struct ControlAfterRedisEnqueue;
impl openrust::contracts::events::Listener for ControlAfterRedisEnqueue {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            if event
                .downcast_ref::<openrust::queue::events::JobQueued>()
                .is_some_and(|event| event.queue == "webhook-recovery")
            {
                if std::env::var_os("FIXTURE_CRASH_AFTER_REDIS_ENQUEUE").is_some() {
                    std::process::exit(79);
                }
                if let Some(directory) = std::env::var_os("FIXTURE_HOLD_AFTER_REDIS_ENQUEUE") {
                    let path = std::path::PathBuf::from(directory);
                    tokio::fs::write(path.join("entered"), b"queued")
                        .await
                        .map_err(|source| openrust::events::EventError::Listener {
                            source: Box::new(source),
                        })?;
                    while !path.join("release").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
            }
            Ok(None)
        })
    }
}
struct RecordPruning(std::path::PathBuf);
impl openrust::contracts::events::Listener for RecordPruning {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            use openrust::database::events::{
                ModelPruningFinished, ModelPruningStarting, ModelsPruned,
            };
            use std::io::Write as _;

            let record = if let Some(event) = event.downcast_ref::<ModelPruningStarting>() {
                json!(["starting", event.models])
            } else if let Some(event) = event.downcast_ref::<ModelsPruned>() {
                json!(["pruned", event.model, event.count])
            } else if let Some(event) = event.downcast_ref::<ModelPruningFinished>() {
                json!(["finished", event.models])
            } else {
                return Ok(None);
            };
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.0)
                .and_then(|mut file| writeln!(file, "{record}"))
                .map_err(|source| openrust::events::EventError::Listener {
                    source: Box::new(source),
                })?;
            Ok(None)
        })
    }
}

struct FixtureReplayPolicy;

impl openrust_webhook_client::replay::ReplayPolicy for FixtureReplayPolicy {
    fn authorize<'a>(
        &'a self,
        request: &'a openrust_webhook_client::replay::ReplayRequest,
    ) -> HttpFuture<
        'a,
        openrust_webhook_client::Result<Option<openrust_webhook_client::replay::ReplayLimits>>,
    > {
        Box::pin(async move {
            Ok((request.actor == "fixture-operator"
                && request.receipt.scope == "webhook-recovery-scope"
                && request.receipt.profile == "processing-recovery"
                && request.receipt.revision == "1")
                .then_some(openrust_webhook_client::replay::ReplayLimits {
                    max_generations: 1,
                    lifetime_seconds: 120,
                }))
        })
    }
}

struct FixtureProvider;
impl openrust::support::ServiceProvider for FixtureProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            use openrust_webhook_client::WebhookRuntime;
            WebhookRuntime::register_unverified_hint_receiver(
                openrust::contracts::foundation::Application::container(app),
                "fixture-hint",
                |config, container| {
                    use openrust::contracts::container::Container as _;
                    let manager = container.make::<openrust::database::DatabaseManager>()?;
                    let limiter = container.make::<openrust::cache::RateLimiter>()?;
                    Ok(std::sync::Arc::new(
                        fixture_hint_queue(&manager, config.name())?.receiver(
                            limiter,
                            4,
                            std::sync::Arc::new(FixtureHint),
                        )?,
                    ))
                },
            )?;
            for name in [
                "fixture",
                "fixture-custom",
                "unregistered",
                "leased",
                "deferred",
                "worker-rate",
                "worker-lock",
            ] {
                WebhookRuntime::register_job_dispatcher(
                    openrust::contracts::foundation::Application::container(app),
                    name,
                    move |config, _| {
                        Ok(std::sync::Arc::new(
                            openrust_webhook_client::jobs::QueuedWebhookJob::new(name)?
                                .on_connection(if config.name() == "discarded" {
                                    "null"
                                } else {
                                    "database"
                                })
                                .on_queue("webhook-receipts")
                                .with_options(openrust::queue::JobOptions {
                                    max_tries: Some(match name {
                                        "leased" | "worker-rate" | "worker-lock" => 4,
                                        "deferred" => 1,
                                        _ => 2,
                                    }),
                                    timeout: Some(std::time::Duration::from_secs(match name {
                                        "worker-lock" => 10,
                                        "leased" | "deferred" | "worker-rate" => 4,
                                        _ => 12,
                                    })),
                                    ..Default::default()
                                }),
                        ))
                    },
                )?;
            }
            WebhookRuntime::register_job_dispatcher(
                openrust::contracts::foundation::Application::container(app),
                "atomic",
                |config, container| {
                    use openrust::contracts::container::Container as _;
                    let env = container.make::<openrust::config::Environment>()?;
                    let connection = match config.name() {
                        "atomic-null" => "null",
                        "atomic-foreign" => "foreign-database",
                        _ => "database",
                    };
                    let leased = config.name() == "retention-digest";
                    Ok(std::sync::Arc::new(AtomicFixtureDispatcher {
                        inner: openrust_webhook_client::jobs::QueuedWebhookJob::new(if leased {
                            "leased"
                        } else {
                            "fixture"
                        })?
                        .on_connection(connection)
                        .on_queue("webhook-receipts")
                        .with_options(openrust::queue::JobOptions {
                            max_tries: Some(2),
                            timeout: Some(std::time::Duration::from_secs(if leased {
                                4
                            } else {
                                12
                            })),
                            ..Default::default()
                        }),
                        crash: env
                            .get_utf8("FIXTURE_ATOMIC_CRASH")
                            .map_err(runtime_error)?
                            .map(str::to_owned),
                    }))
                },
            )?;
            WebhookRuntime::register_job_dispatcher(
                openrust::contracts::foundation::Application::container(app),
                "recovery",
                |config, container| {
                    use openrust::contracts::container::Container as _;
                    let env = container.make::<openrust::config::Environment>()?;
                    let connection = match config.name() {
                        "recovery-null" => "null",
                        "recovery-failover" => "failover",
                        _ => env
                            .get_utf8("FIXTURE_RECOVERY_QUEUE")
                            .map_err(runtime_error)?
                            .unwrap_or("foreign-database"),
                    };
                    let deferred = matches!(
                        config.name(),
                        "deferred-recovery" | "deferred-recovery-tracked"
                    );
                    let job = if deferred {
                        "recover-deferred"
                    } else {
                        "accepted"
                    };
                    Ok(std::sync::Arc::new(
                        openrust_webhook_client::jobs::QueuedWebhookJob::new(job)?
                            .on_connection(connection)
                            .on_queue("webhook-recovery")
                            .with_options(openrust::queue::JobOptions {
                                max_tries: Some(if deferred { 1 } else { 2 }),
                                timeout: Some(std::time::Duration::from_secs(4)),
                                ..Default::default()
                            }),
                    ))
                },
            )?;
            WebhookRuntime::register_job_dispatcher(
                openrust::contracts::foundation::Application::container(app),
                "factory-fails",
                |_, _| {
                    Err(openrust_webhook_client::Error::InvalidConfig(
                        "fixture job factory",
                    ))
                },
            )?;
            WebhookRuntime::register_webhook_profile(
                openrust::contracts::foundation::Application::container(app),
                "ignored",
                |_, _| Ok(std::sync::Arc::new(Ignore)),
            )?;
            WebhookRuntime::register_webhook_profile(
                openrust::contracts::foundation::Application::container(app),
                "fixture-rate-filter",
                |_, _| Ok(std::sync::Arc::new(FixtureRateFilter)),
            )?;
            WebhookRuntime::register_signature_validator(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |_, container| {
                    use openrust::contracts::container::Container as _;
                    Ok(std::sync::Arc::new(CustomValidator {
                        request: container.get("request")?,
                    }))
                },
            )?;
            WebhookRuntime::register_signature_validator(
                openrust::contracts::foundation::Application::container(app),
                "signed-body",
                |_, _| Ok(std::sync::Arc::new(SignedBodyIdentity)),
            )?;
            WebhookRuntime::register_webhook_response(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |_, _| Ok(std::sync::Arc::new(CustomResponse)),
            )?;
            WebhookRuntime::register_webhook_response(
                openrust::contracts::foundation::Application::container(app),
                "recovery-response",
                |_, _| Ok(std::sync::Arc::new(RecoveryResponse)),
            )?;
            WebhookRuntime::register_webhook_store(
                openrust::contracts::foundation::Application::container(app),
                "custom",
                |config, container| {
                    use openrust::contracts::container::Container as _;
                    let manager = container.make::<openrust::database::DatabaseManager>()?;
                    Ok(std::sync::Arc::new(
                        openrust_webhook_client::DatabaseWebhookStore::new(
                            manager.connection(None)?,
                            config.options()["scope"].as_str().unwrap(),
                        )?,
                    ))
                },
            )?;
            if std::env::var_os("FIXTURE_OVERRIDE_DATABASE_STORE").is_some() {
                WebhookRuntime::register_webhook_store(
                    openrust::contracts::foundation::Application::container(app),
                    "database",
                    |_, _| {
                        Err(openrust_webhook_client::Error::InvalidConfig(
                            "fixture-store-override",
                        ))
                    },
                )?;
            }
            Ok(())
        })
    }
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            outbound::register(app).await?;
            mass_pruning::register(app).await?;
            model_pruning::register(app).await?;
            if let Some(path) = std::env::var_os("FIXTURE_MODEL_PRUNING_EVENTS") {
                use openrust::contracts::events::Dispatcher as _;
                use openrust::database::events::{
                    ModelPruningFinished, ModelPruningStarting, ModelsPruned,
                };
                let events = app.make::<openrust::events::Dispatcher>().await?;
                let listener = std::sync::Arc::new(RecordPruning(path.into()));
                for event in [
                    ModelPruningStarting::NAME,
                    ModelsPruned::NAME,
                    ModelPruningFinished::NAME,
                ] {
                    events.listen(event, listener.clone(), openrust::events::Delivery::Sync)?;
                }
            }
            if std::env::var_os("FIXTURE_AUDIT_LISTENER_FAIL").is_some() {
                use openrust::contracts::{
                    container::Container as _, events::Dispatcher as _,
                    foundation::Application as _,
                };
                app.container()
                    .get::<openrust::events::Dispatcher>("events")?
                    .listen(
                        "openrust_webhook_client::events::InvalidWebhookSignatureEvent",
                        std::sync::Arc::new(RejectAuditListener),
                        openrust::events::Delivery::Sync,
                    )?;
            }
            if std::env::var_os("FIXTURE_CRASH_AFTER_REDIS_ENQUEUE").is_some()
                || std::env::var_os("FIXTURE_HOLD_AFTER_REDIS_ENQUEUE").is_some()
            {
                use openrust::contracts::{
                    container::Container as _, events::Dispatcher as _,
                    foundation::Application as _,
                };
                app.container()
                    .get::<openrust::events::Dispatcher>("events")?
                    .listen(
                        openrust::queue::events::JobQueued::NAME,
                        std::sync::Arc::new(ControlAfterRedisEnqueue),
                        openrust::events::Delivery::Sync,
                    )?;
            }
            let container = openrust::contracts::foundation::Application::container(app);
            openrust_webhook_server::register_signer(
                container,
                "fixture-signer",
                std::sync::Arc::new(FixtureSendSigner),
            )?;
            openrust_webhook_server::register_backoff_strategy(
                container,
                "fixture-backoff",
                std::sync::Arc::new(FixtureSendBackoff),
            )?;
            if std::env::var_os("FIXTURE_REPLAY_ALLOW").is_some() {
                openrust_webhook_client::replay::register_policy(
                    openrust::contracts::foundation::Application::container(app),
                    std::sync::Arc::new(FixtureReplayPolicy),
                )?;
            }
            let queue = app.make::<openrust::queue::QueueManager>().await?;
            let manager = app.make::<openrust::database::DatabaseManager>().await?;
            app.make::<openrust::cache::RateLimiter>()
                .await?
                .for_job("fixture-webhook-worker", |_| {
                    openrust::cache::rate_limiting::Limit::per_minute(1).by("webhook-acceptance")
                });
            use openrust::queue::middleware::{JobMiddleware, RateLimited, WithoutOverlapping};
            use std::{sync::Arc, time::Duration};
            let controls: [(&str, Arc<dyn JobMiddleware>); 2] = [
                (
                    "worker-rate",
                    Arc::new(RateLimited::new("fixture-webhook-worker")),
                ),
                (
                    "worker-lock",
                    Arc::new(
                        WithoutOverlapping::new("webhook-acceptance")
                            .release_after(Duration::from_secs(20))
                            .expire_after(Duration::from_secs(20)),
                    ),
                ),
            ];
            for (name, middleware) in controls {
                openrust_webhook_client::jobs::QueuedWebhookJob::register_with_database_lease(
                    &queue,
                    name,
                    Arc::new(openrust_webhook_client::DatabaseWebhookStore::new(
                        manager.connection(None)?,
                        "webhook-acceptance",
                    )?),
                    Arc::new(FixtureControlledJob(middleware)),
                    15,
                )?;
            }
            for profile in ["hint", "hint-standard"] {
                fixture_hint_queue(&manager, profile)?
                    .register(&queue, std::sync::Arc::new(FixtureHint))?;
            }
            for (name, scope) in [
                ("fixture", "webhook-acceptance"),
                ("fixture-custom", "webhook-custom"),
            ] {
                let store = openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    scope,
                )?;
                openrust_webhook_client::jobs::QueuedWebhookJob::register(
                    &queue,
                    name,
                    std::sync::Arc::new(store),
                    std::sync::Arc::new(FixtureJob),
                )?;
            }
            openrust_webhook_client::jobs::QueuedWebhookJob::register_with_database_recovery(
                &queue,
                "accepted",
                std::sync::Arc::new(openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    "webhook-recovery-scope",
                )?),
                std::sync::Arc::new(FixtureJob),
                6,
            )?;
            openrust_webhook_client::jobs::QueuedWebhookJob::register_with_database_recovery_and_deferred_processing(
                &queue,
                "recover-deferred",
                std::sync::Arc::new(openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    "webhook-recovery-scope",
                )?),
                std::sync::Arc::new(FixtureJob),
                6,
                openrust_webhook_client::jobs::DeferredProcessingPolicy {
                    max_deferrals: 1,
                    max_age_seconds: 120,
                },
            )?;
            openrust_webhook_client::jobs::QueuedWebhookJob::register_with_database_lease(
                &queue,
                "leased",
                std::sync::Arc::new(openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    "webhook-acceptance",
                )?),
                std::sync::Arc::new(FixtureJob),
                6,
            )?;
            openrust_webhook_client::jobs::QueuedWebhookJob::register_with_database_deferred_processing(
                &queue,
                "deferred",
                std::sync::Arc::new(openrust_webhook_client::DatabaseWebhookStore::new(
                    manager.connection(None)?,
                    "webhook-acceptance",
                )?),
                std::sync::Arc::new(FixtureJob),
                6,
                openrust_webhook_client::jobs::DeferredProcessingPolicy {
                    max_deferrals: 1,
                    max_age_seconds: 8,
                },
            )?;
            Ok(())
        })
    }
}
struct CustomValidator {
    request: std::sync::Arc<openrust::support::facades::RequestHandle>,
}
impl SignatureValidator for CustomValidator {
    fn is_valid(
        &self,
        request: &WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<bool> {
        assert!(self.request.with(|request| {
            request
                .route_name()
                .unwrap_or_default()
                .contains("webhook-client-custom")
        })?);
        DefaultSignatureValidator.is_valid(request, config)
    }
}
struct RecoveryResponse;
impl RespondsToWebhook for RecoveryResponse {
    fn respond_to_valid_webhook(
        &self,
        request: &WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<Response> {
        if std::env::var_os("FIXTURE_CRASH_AFTER_DURABLE_COMMIT").is_some() {
            std::process::exit(80);
        }
        openrust_webhook_client::DefaultRespondsTo.respond_to_valid_webhook(request, config)
    }
}
struct CustomResponse;
impl RespondsToWebhook for CustomResponse {
    fn respond_to_valid_webhook(
        &self,
        _: &WebhookRequest,
        config: &openrust_webhook_client::WebhookConfig,
    ) -> openrust_webhook_client::Result<Response> {
        Response::json(StatusCode::ACCEPTED, json!({"profile":config.name()})).map_err(|source| {
            openrust_webhook_client::Error::Runtime {
                source: Box::new(source),
            }
        })
    }
}
struct ResolverSwap;
impl openrust::contracts::http::Middleware for ResolverSwap {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        request.set_route_resolver(openrust::http::RouteResolver(std::sync::Arc::new(|| {
            openrust::http::ResolvedRoute {
                name: Some("webhook-client-missing-config".into()),
                ..Default::default()
            }
        })));
        assert_eq!(request.route_name(), Some("webhook-client-missing-config"));
        Box::pin(async move { Ok(next.run(request).await) })
    }
}
struct Stamp;
impl openrust::contracts::http::Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: openrust::http::Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Ok(next.run(request).await.header(
                "x-webhook-group".parse().unwrap(),
                "preserved".parse().unwrap(),
            ))
        })
    }
}
fn api_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust_webhook_client::{WebhookRouteOptions as Options, WebhookRouter, WebhookRoutes};
    Route::post("/webhooks/{profile}", verify)?;
    Route::post("/received/{profile}", receive)?;
    Route::post("/outbound/{mode}/{key}", outbound::submit)?;
    Route::get("/outbound-inspect/{scope}/{kind}/{id}", outbound::inspect)?;
    Route::get(
        "/inbound-inspect/{scope}/{kind}/{id}",
        receipt_diagnostics::inspect,
    )?;
    for route in [
        "atomic",
        "atomic-null",
        "atomic-foreign",
        "recovery",
        "processing-recovery",
        "deferred-recovery",
        "deferred-recovery-tracked",
        "recovery-null",
        "recovery-failover",
        "leased",
        "deferred",
        "verified-quota",
        "verified-other",
        "worker-rate",
        "worker-lock",
        "event-dedup",
        "event-other",
        "event-unsigned",
        "event-custom",
    ] {
        Route::webhooks(&format!("/package/{route}"), Options::new().name(route))?;
    }
    Route::webhooks(
        "/package/digest-window",
        Options::new()
            .name("digest-window")
            .methods(["get", "post"]),
    )?;
    Route::webhooks(
        "/package/digest-invalid",
        Options::new().name("digest-invalid"),
    )?;
    Route::webhooks(
        "/package/retention-digest",
        Options::new().name("retention-digest"),
    )?;
    for profile in ["hint", "hint-standard", "hint-wrong-scope"] {
        Route::webhooks(&format!("/package/{profile}"), Options::new().name(profile))?;
    }
    for profile in ["audit-required", "audit-error"] {
        Route::webhooks(&format!("/package/{profile}"), Options::new().name(profile))?;
    }
    for profile in ["attachment-disk", "attachment-plain", "attachment-disabled"] {
        Route::webhooks(&format!("/package/{profile}"), Options::new().name(profile))?;
    }
    Route::webhooks("/package/default", Options::default())?;
    Route::webhooks("/package/unregistered", Options::new().name("unregistered"))?;
    Route::webhooks("/package/discarded", Options::new().name("discarded"))?;
    Route::webhooks("/package/post", Options::new().name("spatie"))?;
    Route::webhooks(
        "/package/no-attachments",
        Options::new().name("no-attachments"),
    )?;
    Route::webhooks("/package/standard", Options::new().name("standard"))?;
    Route::webhooks(
        "/package/methods",
        Options::new()
            .name("spatie")
            .methods(["get", "query", "post", "put", "patch", "delete"]),
    )?;
    Route::webhooks("/package/provider/{account}", Options::new().name("spatie"))?;
    Route::webhooks("/package/custom", Options::new().name("custom"))?;
    Route::webhooks("/package/ignored", Options::new().name("package-ignored"))?;
    Route::webhooks("/package/unknown", Options::new().name("missing-config"))?;
    Route::webhooks(
        "/package/factory-fails",
        Options::new().name("factory-fails"),
    )?;
    Route::webhooks_with(
        "/package/resolved",
        Options::new().name("spatie"),
        |route| {
            route.middleware(std::sync::Arc::new(ResolverSwap));
            Ok(())
        },
    )?;
    Route::webhooks("/package/overlap", Options::new().name("default"))?;
    Route::webhooks("/package/overlap", Options::new().name("spatie"))?;
    Route::group(
        openrust::routing::RouteGroup::new()
            .prefix("package/group")
            .name("team.")
            .middleware(std::sync::Arc::new(Stamp)),
        |router| {
            router.webhooks("/direct", Options::new().name("spatie"))?;
            Ok(())
        },
    )?;
    Route::group(
        openrust::routing::RouteGroup::new()
            .prefix("package/domain")
            .name("domain.")
            .domain("{tenant}.webhook.test"),
        |router| {
            router.webhooks("/callback", Options::new().name("spatie"))?;
            Ok(())
        },
    )?;
    Route::webhooks_with(
        "/package/optional/{id?}",
        Options::new().name("spatie"),
        |route| {
            route.defaults("id", "42").where_number("id")?;
            Ok(())
        },
    )?;
    // 注册期无效方法必须失败且不留下半条路由。
    for method in ["POST", "head", "options", "trace", "connect", ""] {
        assert!(
            Route::webhooks(
                "/package/must-not-register",
                Options::new().methods([method])
            )
            .is_err()
        );
    }
    Ok(())
}
fn web_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust_webhook_client::{WebhookRouteOptions as Options, WebhookRoutes};
    Route::webhooks("/webhook-web/protected", Options::new().name("spatie"))?;
    Route::webhooks("/webhook-web/except", Options::new().name("spatie"))?;
    Route::post("/webhook-web/ordinary", ordinary)
}
fn ordinary(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"ordinary":true})) })
}
struct FixtureSendSigner;

impl Signer for FixtureSendSigner {
    fn signature_header_name(&self) -> &str {
        "x-fixture-signature"
    }

    fn calculate_signature(
        &self,
        context: &SigningContext<'_>,
    ) -> openrust_webhook_server::Result<String> {
        Ok(format!(
            "{}:{}:{}",
            context.message_id,
            context.timestamp,
            context.body.len()
        ))
    }
}

struct FixtureSendBackoff;

impl openrust_webhook_server::BackoffStrategy for FixtureSendBackoff {
    fn wait_in_seconds_after_attempt(&self, attempt: u32) -> u64 {
        u64::from(attempt) * 7
    }
}

struct PrepareSend(CommandDefinition);

impl Command for PrepareSend {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            use openrust_webhook_server::{TargetPolicy, WebhookCall};

            let profile = input.matches().get_one::<String>("profile").unwrap();
            let call = WebhookCall::create()
                .map_err(ConsoleError::from_source)?
                .uuid("outgoing-fixture")
                .use_timestamp(true);
            let call = if profile == "missing-url" {
                call
            } else {
                call.url("https://example.test/callback?old=1")
            };
            let call = match profile.as_str() {
                "missing-url" | "missing-secret" => call,
                "zero-secret" => call.use_secret("0"),
                _ => call.use_secret("outgoing-fixture-credential"),
            };
            let call = match profile.as_str() {
                "json" => call.payload(json!({"z": "中/🙂", "a": 2})),
                "raw" => call
                    .send_raw_body(b"\x00\xffraw\n".to_vec())
                    .do_not_sign()
                    .with_webhook_id_header(None),
                "get" => call
                    .use_http_verb(openrust::http::Method::GET)
                    .payload(json!({"label": "a b", "settings": {"enabled": true, "label": null}})),
                "custom" => call
                    .send_raw_body(b"test".to_vec())
                    .sign_using("fixture-signer")
                    .use_backoff_strategy("fixture-backoff"),
                "blocked" => call.url("https://127.0.0.1/callback"),
                "missing-url" | "missing-secret" | "zero-secret" => call
                    .use_http_verb(openrust::http::Method::GET)
                    .send_raw_body(b"private-body-sentinel".to_vec()),
                "bad-signer" => call.sign_using("unregistered-signer"),
                "bad-backoff" => call.use_backoff_strategy("unregistered-backoff"),
                _ => return Err(ConsoleError::new("未知 Webhook 发送夹具")),
            };
            let prepared = call
                .prepare(&TargetPolicy::default())
                .map_err(ConsoleError::from_source)?;
            let headers = prepared
                .headers_at(1_700_000_000, None)
                .map_err(ConsoleError::from_source)?;
            let headers: BTreeMap<_, _> = headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
                .collect();
            let snapshot = json!({"url": prepared.get_url(), "id": prepared.get_uuid(),
                "method": prepared.method().as_str(), "body": prepared.body(), "signed": prepared.signing_bytes(),
                "headers": headers, "tries": prepared.tries(), "timeout": prepared.timeout().as_secs(),
                "backoff": prepared.backoff_seconds(2), "queue": prepared.queue()});
            output
                .write_raw(&format!("{snapshot}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct Sign(CommandDefinition);
impl Command for Sign {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let name = input.matches().get_one::<String>("profile").unwrap();
            let body = input.matches().get_one::<String>("body").unwrap();
            let config = Configs::get_facade_root()
                .map_err(ConsoleError::from_source)?
                .get_config(name)
                .map_err(ConsoleError::from_source)?
                .ok_or_else(|| ConsoleError::new("未知配置"))?;
            let context = SigningContext {
                webhook_url: "https://example.test/callback",
                body: body.as_bytes(),
                secret: config.signing_secret(),
                message_id: "message-fixture",
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
            };
            let signer: &dyn Signer = if name == "standard" {
                &StandardWebhookSigner
            } else {
                &DefaultSigner::default()
            };
            let headers: BTreeMap<_, _> = signer
                .headers(&context)
                .map_err(ConsoleError::from_source)?
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_owned()))
                .collect();
            let environment = application
                .make::<openrust::config::Environment>()
                .await
                .map_err(ConsoleError::from_source)?;
            if let Some(path) = environment
                .get_utf8("WEBHOOK_TEST_OUTPUT")
                .map_err(ConsoleError::from_source)?
            {
                std::fs::write(
                    path,
                    serde_json::to_vec(&json!({"headers":headers}))
                        .map_err(ConsoleError::from_source)?,
                )
                .map_err(ConsoleError::from_source)?;
            }
            output
                .write_raw(&format!("{}\n", json!({"headers":headers})))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
struct Enqueue(CommandDefinition);
impl Command for Enqueue {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let profile = input.matches().get_one::<String>("profile").unwrap();
            let body = input.matches().get_one::<String>("body").unwrap();
            openrust::support::facades::Artisan::queue(
                "webhooks:sign",
                &[profile.into(), body.into()],
            )
            .on_connection("database")
            .on_queue("webhook-primitives")
            .dispatch()
            .await
            .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    mass_pruning::commands(artisan)?;
    model_pruning::commands(artisan)?;
    outbound::commands(artisan)?;
    let syntax = |name| {
        clap::Command::new(name)
            .arg(clap::Arg::new("profile").required(true))
            .arg(clap::Arg::new("body").required(true))
    };
    artisan.add_command(Box::new(PrepareSend(CommandDefinition::new(
        clap::Command::new("webhooks:prepare-send").arg(clap::Arg::new("profile").required(true)),
    ))))?;
    artisan.add_command(Box::new(Sign(CommandDefinition::new(syntax(
        "webhooks:sign",
    )))))?;
    artisan.add_command(Box::new(Enqueue(CommandDefinition::new(syntax(
        "webhooks:enqueue",
    )))))?;
    artisan.add_command(Box::new(RestoreLegacyAttachments(CommandDefinition::new(
        clap::Command::new("webhooks:fixture-restore-legacy-attachments")
            .arg(clap::Arg::new("export").required(true)),
    ))))?;
    artisan.add_command(Box::new(WakeDeferred(CommandDefinition::new(
        clap::Command::new("webhooks:wake-fixture")
            .arg(clap::Arg::new("id").required(true))
            .arg(clap::Arg::new("scope").required(true)),
    ))))?;
    artisan.add_command(Box::new(Record(CommandDefinition::new(
        clap::Command::new("webhooks:record")
            .arg(clap::Arg::new("id").required(true))
            .arg(clap::Arg::new("scope").required(true))
            .arg(clap::Arg::new("save-exception").long("save-exception"))
            .arg(
                clap::Arg::new("clear-exception")
                    .long("clear-exception")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("WEBHOOK_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root=Application::infer_base_path(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root).with_config(|root,env| {
            let mut config=config::load(root,env)?;
            let database=config["database"]["default"].as_str().unwrap().to_owned();
            let attachment_disk = env.get_utf8("FIXTURE_ATTACHMENT_DISK")?.unwrap_or("local");
            let rate_scope = env.get_utf8("FIXTURE_VERIFIED_RATE_SCOPE")?.unwrap_or("webhook-acceptance");
            let rate_revision = env.get_utf8("FIXTURE_VERIFIED_RATE_REVISION")?.unwrap_or("one");
            let rate_job = if rate_scope == "webhook-custom" { "fixture-custom" } else { "fixture" };
            let alias=config["database"]["connections"][&database].clone();
            config.get_mut("database").unwrap()["connections"]["fixture-foreign"]=alias;
            // 原子派发须覆盖普通连接的提交后派发默认值。
            config.get_mut("queue").unwrap()["connections"]["database"]["after_commit"]=json!(true);
            config.get_mut("queue").unwrap()["connections"]["foreign-database"]=json!({"driver":"database","connection":"fixture-foreign","table":env.get_utf8("FIXTURE_FOREIGN_QUEUE_TABLE")?.unwrap_or("jobs"),"queue":"webhook-receipts"});
            config.insert("webhook-client".into(),json!({"add_unique_token_to_route_name": env.get_utf8("FIXTURE_UNIQUE_ROUTES")?==Some("true"),"configs":[
                {"name":"spatie","revision":"one","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"no-attachments","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","store_attachments":false},
                {"name":"standard","revision":"one","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"standard","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"ignored","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"}},
                {"name":"dispatch-fails","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"}},
                {"name":"default","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"custom","webhook_model":"database","scope":"webhook-custom"},
                {"name":"verified-quota","revision":rate_revision,"signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":rate_scope,"process_webhook_job":rate_job,"signature_validator":"default","webhook_profile":"fixture-rate-filter","webhook_model":"database","verified_requests_per_minute":3},
                {"name":"verified-other","revision":rate_revision,"signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":rate_scope,"process_webhook_job":rate_job,"signature_validator":"default","webhook_profile":"fixture-rate-filter","webhook_model":"database","verified_requests_per_minute":1},
                {"name":"custom","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"signature_header_name":"X-Webhook","scope":"webhook-custom","process_webhook_job":"fixture-custom","signature_validator":"custom","webhook_model":"custom","webhook_response":"custom","webhook_profile":"process-everything"},
                {"name":"package-ignored","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"webhook_profile":"ignored","webhook_model":"missing","process_webhook_job":"fixture","signature_validator":"default"},
                {"name":"discarded","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"unregistered","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"unregistered","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"event-dedup","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"standard","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_verified_events":true},
                {"name":"event-other","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"standard","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_verified_events":true},
                {"name":"recovery","revision":"recover-v1","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true,"deduplicate_body_seconds":300,"webhook_response":"recovery-response"},
                {"name":"processing-recovery","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true,"recover_processing":{"after_seconds":60,"max_requeues":2}},
                {"name":"deferred-recovery","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true},
                {"name":"deferred-recovery-tracked","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true,"recover_processing":{"after_seconds":60,"max_requeues":2}},
                {"name":"recovery-null","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true},
                {"name":"recovery-failover","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-recovery-scope","process_webhook_job":"recovery","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","recover_dispatch":true},
                {"name":"attachment-disk","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"attachment_disk":attachment_disk,"deduplicate_body_seconds":30},
                {"name":"attachment-plain","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","attachment_disk":"local"},
                {"name":"attachment-disabled","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","attachment_disk":"local","store_attachments":false},
                {"name":"audit-required","revision":"audit-v1","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","verification_audit":true},
                {"name":"audit-error","revision":"audit-error-v1","signing_secret":{"env":"FIXTURE_EMPTY_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","verification_audit":true},
                {"name":"hint","revision":"hint-one","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","unverified_hint_receiver":"fixture-hint","verification_audit":true},
                {"name":"hint-wrong-scope","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"different-account","process_webhook_job":"fixture","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","unverified_hint_receiver":"fixture-hint"},
                {"name":"hint-standard","signing_secret":{"env":"FIXTURE_STANDARD_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"fixture","signature_validator":"standard","webhook_profile":"process-everything","webhook_model":"database","unverified_hint_receiver":"fixture-hint","verification_audit":true},
                {"name":"retention-digest","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_body_seconds":5},
                {"name":"digest-window","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_body_seconds":5},
                {"name":"digest-invalid","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","deduplicate_body_seconds":5},
                {"name":"event-custom","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"signed-body","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_verified_events":true},
                {"name":"event-unsigned","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true,"deduplicate_verified_events":true},
                {"name":"deferred","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"deferred","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"leased","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"leased","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"},
                {"name":"atomic","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true},
                {"name":"atomic-null","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true},
                {"name":"atomic-foreign","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"atomic","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database","atomic_database_queue":true},
                {"name":"factory-fails","signing_secret":{"env":"FIXTURE_SPATIE_SECRET"},"scope":"webhook-acceptance","process_webhook_job":"factory-fails","signature_validator":"default","webhook_profile":"process-everything","webhook_model":"database"}
            ]}));
            let profiles = config.get_mut("webhook-client").expect("Webhook 测试配置")["configs"]
                .as_array_mut().expect("Webhook 测试 profile 列表");
            for name in ["worker-rate", "worker-lock"] {
                profiles.push(json!({
                    "name": name, "signing_secret": {"env":"FIXTURE_SPATIE_SECRET"},
                    "scope": "webhook-acceptance", "process_webhook_job": name,
                    "signature_validator": "default", "webhook_profile": "process-everything",
                    "webhook_model": "database",
                }));
            }
            if let Some(value) = env.get_utf8("FIXTURE_PRUNE_POLICY")? {
                config.get_mut("webhook-client").unwrap()["delete_after_days"] = serde_json::from_str(value).expect("夹具配置必须是有效 JSON");
            }
            Ok(config)
        }).with_packages(packages::manifest()).with_providers(vec![Box::new(FixtureProvider)]).with_commands(commands).with_migrations(migrations::entries())
        .with_middleware(|middleware| { middleware.prevent_request_forgery(["webhook-web/except"],false,false); })
        .with_schedule(outbound::schedule)
        .with_routing(Routing::default().api(api_routes).web(web_routes))
        .create().map_err(Into::into)
    }).await
}
