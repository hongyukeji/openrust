use std::sync::Arc;

use openrust::{
    console::{ConsoleError, scheduling::Schedule},
    contracts::{
        events::{Event, Listener},
        foundation::Application as _,
    },
    database::DatabaseManager,
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::QueueManager,
    support::{
        ProviderResult,
        facades::{Event as Events, Facade, Queue},
    },
};
use openrust_webhook_server::{
    BackoffStrategy, CallWebhookJob, DeliveryContext, DeliveryPermission, DeliveryPolicy,
    DeliveryService, DeliveryStore, Error, Result, TargetPolicy, WebhookCall, WebhookJobAttempt,
    events::{
        DispatchingWebhookCallEvent, FinalWebhookCallFailedEvent, WebhookCallEvent,
        WebhookCallFailedEvent, WebhookCallSucceededEvent,
    },
    register_backoff_strategy, register_call_webhook_job, register_delivery_policy,
};
use serde_json::json;

const SCOPE: &str = "webhook-outbound-test";
const REFERENCE: &str = "fixture-outbound-ref";
const SECRET: &str = "outbound-isolated-test-key";

struct FixturePolicy;
impl DeliveryPolicy for FixturePolicy {
    fn authorize<'a>(
        &'a self,
        context: DeliveryContext<'a>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>> {
        Box::pin(async move {
            if context.scope != SCOPE
                || context.credential != Some(REFERENCE)
                || std::env::var_os("FIXTURE_SEND_REVOKED").is_some()
            {
                return Ok(None);
            }
            let (url, policy) = target()?;
            if context.url != url {
                return Ok(None);
            }
            Ok(Some(
                DeliveryPermission::new(policy).with_secret(SECRET.to_owned()),
            ))
        })
    }
}

struct FixtureBackoff;
impl BackoffStrategy for FixtureBackoff {
    fn wait_in_seconds_after_attempt(&self, _: u32) -> u64 {
        0
    }
}

struct RecordDeliveryEvent(std::path::PathBuf);

fn event_error(error: impl std::error::Error + Send + Sync + 'static) -> EventError {
    EventError::Listener {
        source: Box::new(error),
    }
}

impl Listener for RecordDeliveryEvent {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            use tokio::io::AsyncWriteExt as _;
            let Some(payload) = event_payload(event) else {
                return Ok(None);
            };
            assert!(event.properties().is_none());
            let manager = super::Databases::get_facade_root().map_err(event_error)?;
            let connection = manager.connection(None).map_err(event_error)?;
            let state = connection
                .table("webhook_deliveries")
                .r#where("id", "=", payload.delivery_id.as_str())
                .select(["state"])
                .first()
                .await
                .map_err(event_error)?
                .and_then(|row| match row.get("state") {
                    Some(openrust::database::Value::Text(state)) => Some(state.clone()),
                    _ => None,
                });
            let jobs = connection
                .table("jobs")
                .r#where("queue", "=", "webhook-outgoing")
                .r#where("payload", "like", format!("%{}%", payload.delivery_id))
                .count("*")
                .await
                .map_err(event_error)?;
            let body = payload.body().map_err(event_error)?;
            let row = json!({
                "kind": event.name().rsplit("::").next(),
                "id": payload.delivery_id, "scope": payload.scope, "message_id": payload.message_id(),
                "method": payload.method(), "url": payload.url(), "body_length": body.len(),
                "headers": payload.header_names().collect::<Vec<_>>(), "meta": payload.meta(),
                "tags": payload.tags(), "state": state, "jobs": jobs,
                "attempt": payload.attempt.as_ref().map(|attempt| json!({
                    "number": attempt.number, "status": attempt.status, "outcome": attempt.outcome,
                    "progress": attempt.progress,
                })),
            });
            if payload.message_id() == "scheduler-crash"
                && payload.attempt.is_none()
                && std::env::var_os("FIXTURE_CRASH_OUTBOUND_WAKE").is_some()
            {
                std::process::exit(82);
            }
            let line = format!("{row}\n");
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.0)
                .await
                .map_err(event_error)?;
            file.write_all(line.as_bytes()).await.map_err(event_error)?;
            if (payload.message_id() == "dispatch-listener-error" && payload.attempt.is_none())
                || (payload.message_id() == "attempt-listener-error" && payload.attempt.is_some())
            {
                return Err(event_error(std::io::Error::other(
                    "fixture-sensitive-listener-sentinel",
                )));
            }
            Ok(None)
        })
    }
}

fn event_payload(event: &dyn Event) -> Option<&WebhookCallEvent> {
    if let Some(event) = event.downcast_ref::<DispatchingWebhookCallEvent>() {
        Some(&event.0)
    } else if let Some(event) = event.downcast_ref::<WebhookCallSucceededEvent>() {
        Some(&event.0)
    } else if let Some(event) = event.downcast_ref::<WebhookCallFailedEvent>() {
        Some(&event.0)
    } else {
        event
            .downcast_ref::<FinalWebhookCallFailedEvent>()
            .map(|event| event.0.as_ref())
    }
}

struct FixtureCustomJob;

impl CallWebhookJob for FixtureCustomJob {
    fn should_be_removed_from_queue(&self, attempt: &WebhookJobAttempt<'_>) -> bool {
        record_hook("should_remove", attempt).expect("记录测试 Job 重试判定");
        attempt.message_id().starts_with("custom-stop")
    }

    fn failed(&self, attempt: &WebhookJobAttempt<'_>) -> Result<()> {
        record_hook("failed", attempt).map_err(|_| Error::InvalidConfig("fixture job event"))?;
        if attempt.message_id() == "custom-stop-sync" {
            return Err(Error::InvalidConfig("fixture-custom-hook-sensitive"));
        }
        Ok(())
    }
}

fn record_hook(kind: &str, attempt: &WebhookJobAttempt<'_>) -> std::io::Result<()> {
    use std::io::Write as _;
    let path = std::env::var_os("FIXTURE_JOB_HOOK_LOG").expect("测试 Job 日志");
    let body_matches = attempt.get_body().expect("测试 Job 正文") == b"\x00sealed\x80\xff";
    let matches_peer = attempt.get_response().is_some_and(|response| {
        response
            .body()
            .is_ok_and(|body| body.as_ref() == b"fixture-remote-response")
    });
    let entry = json!({"kind": kind, "id": attempt.delivery_id, "number": attempt.number,
        "scope": attempt.scope, "outcome": attempt.outcome, "message": attempt.message_id(),
        "url": attempt.get_webhook_url(), "meta": attempt.meta(), "tags": attempt.tags(),
        "status": attempt.get_response().map(|value| value.status().as_u16()),
        "request_matches": body_matches, "response_matches": matches_peer });
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(log, "{entry}")
}

pub(super) async fn register(app: &Application) -> ProviderResult {
    if std::env::var_os("FIXTURE_SEND_TARGET").is_some() {
        let policy = Arc::new(FixturePolicy);
        register_delivery_policy(app.container(), policy.clone())?;
        register_backoff_strategy(app.container(), "fixture-rapid", Arc::new(FixtureBackoff))?;
        let manager = app.make::<DatabaseManager>().await?;
        let queue = app.make::<QueueManager>().await?;
        let store = DeliveryStore::new(manager.connection(None)?, SCOPE)?;
        let service = DeliveryService::new(store, policy);
        service.register_worker(&queue, "default")?;
        service.register_worker(&queue, "fixture-custom")?;
        if std::env::var_os("FIXTURE_HIDE_JOB_HOOK").is_none() {
            register_call_webhook_job(
                app.container(),
                "fixture-custom",
                Arc::new(FixtureCustomJob),
            )?;
        }
        if let Some(path) = std::env::var_os("FIXTURE_SEND_EVENTS") {
            Events::listen(
                "openrust_webhook_server::events::*",
                Arc::new(RecordDeliveryEvent(path.into())),
                Delivery::Sync,
            )?;
        }
    }
    Ok(())
}

fn target() -> Result<(String, TargetPolicy)> {
    let url = std::env::var("FIXTURE_SEND_TARGET").map_err(|_| Error::TargetDenied)?;
    let address = std::env::var("FIXTURE_SEND_ADDRESS")
        .map_err(|_| Error::TargetDenied)?
        .parse()
        .map_err(|_| Error::TargetDenied)?;
    let policy = TargetPolicy::trusted_endpoint(&url, vec![address])?;
    Ok((url, policy))
}

pub(super) fn submit(
    request: &mut Request,
) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        let key = request.route("key").ok_or(HttpError::NotFound)?.to_owned();
        let mode = request.route("mode").ok_or(HttpError::NotFound)?.to_owned();
        let (url, policy) = target().map_err(super::internal)?;
        let data = axum::body::to_bytes(request.take_body(), 1024)
            .await
            .map_err(super::internal)?;
        let mut call = WebhookCall::create()
            .map_err(super::internal)?
            .url(url)
            .uuid(&key)
            .send_raw_body(data.to_vec())
            .use_secret_reference(REFERENCE)
            .use_timestamp(true)
            .use_backoff_strategy("fixture-rapid")
            .respect_retry_after(true)
            .on_connection(Some(
                std::env::var("FIXTURE_SEND_QUEUE").unwrap_or_else(|_| "database".into()),
            ))
            .on_queue(Some("webhook-outgoing".into()))
            .timeout_in_seconds(if mode == "timeout" { 1 } else { 4 })
            .maximum_tries(2);
        if mode == "inline" {
            call = call.use_secret(SECRET);
        }
        if mode == "exclude-503" {
            call = call.retry_on(Some(vec![StatusCode::TOO_MANY_REQUESTS]));
        }
        let manager = super::Databases::get_facade_root().map_err(super::internal)?;
        let connection = manager.connection(None).map_err(super::internal)?;
        let store = DeliveryStore::new(connection.clone(), SCOPE).map_err(super::internal)?;
        let lifetime = if mode == "expired" { 1 } else { 120 };
        if let Some(operation) = mode.strip_prefix("builder-") {
            return dispatch_builder(operation, &key, call, store, &policy, lifetime).await;
        }
        let prepared = call.prepare(&policy).map_err(super::internal)?;
        let id = if mode == "rollback" {
            let mut tx = connection
                .begin_transaction()
                .await
                .map_err(super::internal)?;
            let id = store
                .submit_in_transaction(&mut tx, &key, &prepared, lifetime)
                .await
                .map_err(super::internal)?;
            tx.roll_back(None).await.map_err(super::internal)?;
            Ok(id)
        } else {
            store.submit(&key, &prepared, lifetime).await
        };
        match id {
            Ok(id) => Response::json(StatusCode::ACCEPTED, json!({"id":id})),
            Err(Error::IdempotencyConflict) => Response::json(StatusCode::CONFLICT, json!({})),
            Err(Error::InvalidConfig(_)) => {
                Response::json(StatusCode::UNPROCESSABLE_ENTITY, json!({}))
            }
            Err(error) => Err(super::internal(error)),
        }
    })
}

async fn dispatch_builder(
    operation: &str,
    key: &str,
    call: WebhookCall,
    store: DeliveryStore,
    target: &TargetPolicy,
    lifetime: u32,
) -> std::result::Result<Response, HttpError> {
    let service = DeliveryService::new(store, Arc::new(FixturePolicy));
    let queue = Queue::get_facade_root().map_err(super::internal)?;
    let mut call = call
        .meta(json!({"trace":"fixture"}))
        .with_tags(vec!["audit".into()]);
    let skip = operation.ends_with("-skip");
    if skip {
        call = call.url("cannot prepare this URL");
    }
    let result = match operation {
        "async-if" | "async-if-skip" => call
            .dispatch_if(!skip, &service, &queue, target, key, lifetime)
            .await
            .map(|receipt| receipt.map(|receipt| receipt.delivery_id)),
        "async-unless" | "async-unless-skip" => call
            .dispatch_unless(skip, &service, &queue, target, key, lifetime)
            .await
            .map(|receipt| receipt.map(|receipt| receipt.delivery_id)),
        "sync-if" | "sync-if-skip" => call
            .dispatch_sync_if(!skip, &service, target, key, lifetime)
            .await
            .map(|receipt| receipt.map(|receipt| receipt.delivery_id)),
        "sync-unless" | "sync-unless-skip" => call
            .dispatch_sync_unless(skip, &service, target, key, lifetime)
            .await
            .map(|receipt| receipt.map(|receipt| receipt.delivery_id)),
        "sync-throw" => call
            .maximum_tries(1)
            .throw_exception_on_failure(true)
            .dispatch_sync(&service, target, key, lifetime)
            .await
            .map(|receipt| Some(receipt.delivery_id)),
        "async-throw" => call
            .maximum_tries(1)
            .throw_exception_on_failure(true)
            .dispatch(&service, &queue, target, key, lifetime)
            .await
            .map(|receipt| Some(receipt.delivery_id)),
        "custom-async" => call
            .use_job("fixture-custom")
            .dispatch(&service, &queue, target, key, lifetime)
            .await
            .map(|receipt| Some(receipt.delivery_id)),
        "custom-sync" => call
            .use_job("fixture-custom")
            .throw_exception_on_failure(true)
            .dispatch_sync(&service, target, key, lifetime)
            .await
            .map(|receipt| Some(receipt.delivery_id)),
        "async-missing" => call
            .use_job("fixture-unregistered")
            .dispatch(&service, &queue, target, key, lifetime)
            .await
            .map(|receipt| Some(receipt.delivery_id)),
        _ => return Err(HttpError::NotFound),
    };
    match result {
        Ok(Some(id)) => Response::json(StatusCode::ACCEPTED, json!({"id":id})),
        Ok(None) => Response::json(StatusCode::OK, json!({"skipped":true})),
        Err(Error::SynchronousDeliveryFailed { delivery_id }) => {
            Response::json(StatusCode::BAD_GATEWAY, json!({"id":delivery_id}))
        }
        Err(Error::DispatchEventFailed { delivery_id }) => {
            Response::json(StatusCode::SERVICE_UNAVAILABLE, json!({"id":delivery_id}))
        }
        Err(Error::InvalidConfig(_)) => Response::json(StatusCode::UNPROCESSABLE_ENTITY, json!({})),
        Err(error) => Err(super::internal(error)),
    }
}

pub(super) fn schedule(schedule: &mut Schedule) -> std::result::Result<(), ConsoleError> {
    if std::env::var_os("FIXTURE_SEND_SCHEDULE").is_some() {
        schedule
            .command(
                "webhook-server:recover",
                ["--scope=webhook-outbound-test".into(), "--limit=20".into()],
            )
            .every_minute();
    }
    Ok(())
}
