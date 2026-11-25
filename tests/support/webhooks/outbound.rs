use std::sync::Arc;

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, scheduling::Schedule,
    },
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
    register_replay_policy,
};
use openrust_webhook_server::{EventType, PublishOptions};
use openrust_webhook_server::{ReplayLimits, ReplayPhase, ReplayPolicy, ReplayRequest};
use serde_json::json;

const SCOPE: &str = "webhook-outbound-test";
const ALTERNATE_LIMIT_SCOPE: &str = "webhook-outbound-limit-alternate";
const REFERENCE: &str = "fixture-outbound-ref";
const SECRET: &str = "outbound-isolated-test-key";
const ROTATED: &str = "outbound-isolated-rotated-key";
const ROTATED_REFERENCE: &str = "fixture-rotated-ref";

struct FixturePolicy;
impl DeliveryPolicy for FixturePolicy {
    fn authorize<'a>(
        &'a self,
        context: DeliveryContext<'a>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>> {
        self.authorize_transport(context, None)
    }

    fn authorize_transport<'a>(
        &'a self,
        context: DeliveryContext<'a>,
        requested_proxy: Option<&'a str>,
    ) -> HttpFuture<'a, Result<Option<DeliveryPermission>>> {
        Box::pin(async move {
            let permitted_scope = context.scope == SCOPE
                || context.scope == ALTERNATE_LIMIT_SCOPE
                    && std::env::var_os("FIXTURE_SEND_LIMITS").is_some();
            if !permitted_scope || std::env::var_os("FIXTURE_SEND_REVOKED").is_some() {
                return Ok(None);
            }
            if context.message_id == "endpoint-late-pause"
                && std::env::var_os("FIXTURE_ENDPOINT_WAIT_FOR_PERMISSION").is_some()
            {
                let root = std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
                tokio::fs::write(root.join("endpoint-policy-entered"), b"entered")
                    .await
                    .map_err(|_| Error::InvalidRecord)?;
                while !root.join("endpoint-policy-release").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
            let secret = match context.credential {
                Some(REFERENCE) => SECRET,
                Some(ROTATED_REFERENCE)
                    if std::env::var_os("FIXTURE_REPLAY_REVOKE_CREDENTIAL").is_none() =>
                {
                    ROTATED
                }
                _ => return Ok(None),
            };
            let (url, policy) = target()?;
            let fixture_backup = std::env::var_os("FIXTURE_SEND_FANOUT").is_some()
                && context.url == format!("{url}?endpoint=backup");
            if context.url != url && !fixture_backup {
                return Ok(None);
            }
            let mut permission = DeliveryPermission::new(policy).with_secret(secret.to_owned());
            if let Some(directory) = std::env::var_os("FIXTURE_SEND_TLS_DIR") {
                let directory = std::path::PathBuf::from(directory);
                let read =
                    |name| std::fs::read(directory.join(name)).map_err(|_| Error::InvalidRecord);
                match context.message_id {
                    "tls-insecure-denied" | "tls-default" => {}
                    "tls-insecure" | "tls-verified-again" => {
                        permission = permission.allow_unverified_tls();
                    }
                    "tls-wrong-ca" => {
                        permission = permission.with_tls_ca(&read("other-ca.pem")?)?
                    }
                    _ => permission = permission.with_tls_ca(&read("ca.pem")?)?,
                }
                let pair = match context.message_id {
                    "tls-client" | "tls-client-unverified" => Some(("client.pem", "client.key")),
                    "tls-client-wrong" => Some(("other-client.pem", "other-client.key")),
                    "tls-client-expired" => Some(("expired-client.pem", "expired-client.key")),
                    "tls-client-mismatched" => Some(("client.pem", "other-client.key")),
                    _ => None,
                };
                if let Some((certificate, key)) = pair {
                    permission = permission.with_mutual_tls(&read(certificate)?, &read(key)?)?;
                }
            }
            if let Ok(address) = std::env::var("FIXTURE_SEND_PROXY_ADDRESS")
                && requested_proxy == Some("fixture-egress")
                && context.message_id != "proxy-missing"
            {
                let address = address.parse().map_err(|_| Error::InvalidRecord)?;
                let password = if context.message_id == "proxy-wrong-password" {
                    "invalid"
                } else {
                    "local p@ss/%"
                };
                use openrust::http::client::{
                    PinnedHttpProxy, PinnedProxy, PinnedSocksProxy, TlsCertificate,
                };
                let proxy = match std::env::var("FIXTURE_SEND_PROXY_PROTOCOL").as_deref() {
                    Ok("connect-tls") => {
                        let directory =
                            std::env::var_os("FIXTURE_SEND_TLS_DIR").ok_or(Error::InvalidRecord)?;
                        let ca = if context.message_id == "proxy-tls-wrong-ca" {
                            "ca.pem"
                        } else {
                            "proxy-ca.pem"
                        };
                        let authority = if context.message_id == "proxy-tls-wrong-host" {
                            "wrong-proxy.invalid"
                        } else {
                            "proxy-fixture.invalid"
                        };
                        let ca = std::fs::read(std::path::Path::new(&directory).join(ca))
                            .map_err(|_| Error::InvalidRecord)?;
                        let certificate =
                            TlsCertificate::from_pem(&ca).map_err(|_| Error::InvalidRecord)?;
                        PinnedHttpProxy::https(address, authority, vec![certificate])
                            .and_then(|proxy| proxy.with_credentials("fixture-user", password))
                            .map(PinnedProxy::from)
                    }
                    Ok("connect") => PinnedHttpProxy::new(address)
                        .and_then(|proxy| proxy.with_credentials("fixture-user", password))
                        .map(PinnedProxy::from),
                    _ => PinnedSocksProxy::new(address)
                        .and_then(|proxy| proxy.with_credentials("fixture-user", password))
                        .map(PinnedProxy::from),
                }
                .map_err(|_| Error::InvalidRecord)?;
                let name = if context.message_id == "proxy-wrong-name" {
                    "other-egress"
                } else {
                    "fixture-egress"
                };
                permission = permission.with_proxy(name, proxy)?;
            }
            Ok(Some(permission))
        })
    }
}

struct FixtureReplayPolicy;

impl ReplayPolicy for FixtureReplayPolicy {
    fn authorize<'a>(
        &'a self,
        request: ReplayRequest<'a>,
    ) -> HttpFuture<'a, Result<Option<ReplayLimits>>> {
        Box::pin(async move {
            if request.phase == ReplayPhase::Request
                && std::env::var_os("FIXTURE_REPLAY_WAIT_FOR_PRUNE").is_some()
            {
                let root = std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
                tokio::fs::write(root.join("retention-replay-ready"), b"ready")
                    .await
                    .map_err(|_| Error::InvalidRecord)?;
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    while !root.join("retention-replay-release").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .map_err(|_| Error::InvalidRecord)?;
            }
            let allowed = request.scope == SCOPE
                && (request.original_credential == Some(REFERENCE)
                    || request.original_credential == Some(ROTATED_REFERENCE)
                        && std::env::var_os("FIXTURE_SEND_FANOUT").is_some())
                && matches!(request.actor, "fixture-operator" | "fixture-supervisor")
                && request.reason.starts_with("test-")
                && std::env::var_os("FIXTURE_REPLAY_FORBIDDEN").is_none()
                && (request.phase == ReplayPhase::Attempt
                    || std::env::var_os("FIXTURE_REPLAY_NO_ADMISSION").is_none());
            Ok(allowed.then(|| ReplayLimits {
                max_generations: 2,
                max_attempts: 1,
                lifetime_seconds: 60,
                allow_succeeded: request.actor == "fixture-supervisor",
                credential_reference: std::env::var_os("FIXTURE_REPLAY_KEEP_ORIGINAL")
                    .is_none()
                    .then(|| ROTATED_REFERENCE.into()),
            }))
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
        if std::env::var_os("FIXTURE_SEND_LIMITS").is_some() {
            use openrust_webhook_server::{
                DeliveryLimit, DeliveryLimits, register_delivery_limits,
            };
            register_delivery_limits(
                app.container(),
                DeliveryLimits {
                    sends_per_scope: Some(DeliveryLimit::new(2, 60)?),
                    sends_per_endpoint: Some(DeliveryLimit::new(1, 60)?),
                    replays_per_scope: Some(DeliveryLimit::new(1, 60)?),
                },
            )?;
        }
        if (std::env::var_os("FIXTURE_SEND_REPLAY").is_some()
            || std::env::var_os("FIXTURE_SEND_ENDPOINTS").is_some()
            || std::env::var_os("FIXTURE_SEND_LIMITS").is_some()
            || std::env::var_os("FIXTURE_SEND_FANOUT").is_some()
            || std::env::var_os("FIXTURE_SEND_RETENTION").is_some())
            && std::env::var_os("FIXTURE_REPLAY_UNREGISTERED").is_none()
        {
            register_replay_policy(app.container(), Arc::new(FixtureReplayPolicy))?;
        }
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
        let body_limit = if mode.starts_with("publish") {
            1024 * 1024 + 1024
        } else {
            1024
        };
        let data = axum::body::to_bytes(request.take_body(), body_limit)
            .await
            .map_err(super::internal)?;
        if mode.starts_with("publish") {
            return publish(&key, &mode, &data, &policy).await;
        }
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
            .timeout_in_seconds(if mode == "timeout" {
                1
            } else if key == "endpoint-late-pause" || key == "endpoint-inflight" {
                10
            } else {
                4
            })
            .maximum_tries(if mode == "fail-once" || key == "endpoint-replay-source" {
                1
            } else {
                2
            });
        if mode == "proxy" {
            call = call.use_proxy(Some("fixture-egress".to_owned()));
        }
        if mode == "tls-insecure" {
            call = call.do_not_verify_ssl();
        }
        if mode == "tls-verified-again" {
            call = call.do_not_verify_ssl().verify_ssl(true);
        }
        if mode == "inline" {
            call = call.use_secret(SECRET);
        }
        if mode == "exclude-503" {
            call = call.retry_on(Some(vec![StatusCode::TOO_MANY_REQUESTS]));
        }
        let manager = super::Databases::get_facade_root().map_err(super::internal)?;
        let connection = manager.connection(None).map_err(super::internal)?;
        let scope = if mode == "limit-other-scope" {
            ALTERNATE_LIMIT_SCOPE
        } else {
            SCOPE
        };
        let store = DeliveryStore::new(connection.clone(), scope).map_err(super::internal)?;
        if let Some(name) = mode.strip_prefix("endpoint-") {
            let endpoint_store = if name == "cross-scope" {
                DeliveryStore::new(connection.clone(), "other-endpoint-scope")
                    .map_err(super::internal)?
                    .endpoints()
            } else {
                store.endpoints()
            };
            let endpoint = endpoint_store
                .find(name)
                .await
                .map_err(super::internal)?
                .ok_or(HttpError::NotFound)?;
            call = call.use_endpoint(&endpoint);
            if name == "override" {
                call = call.use_secret_reference(ROTATED_REFERENCE);
            }
        }
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
            Err(Error::EndpointRevoked) => Response::json(StatusCode::CONFLICT, json!({})),
            Err(Error::InvalidConfig(_)) => {
                Response::json(StatusCode::UNPROCESSABLE_ENTITY, json!({}))
            }
            Err(error) => Err(super::internal(error)),
        }
    })
}

async fn publish(
    key: &str,
    mode: &str,
    data: &[u8],
    policy: &TargetPolicy,
) -> std::result::Result<Response, HttpError> {
    let manager = super::Databases::get_facade_root().map_err(super::internal)?;
    let connection = manager.connection(None).map_err(super::internal)?;
    let store = DeliveryStore::new(connection.clone(), SCOPE).map_err(super::internal)?;
    let (name, version) = match mode {
        "publish-overflow" => ("invoice.overflow", 1),
        "publish-savepoint" => ("invoice.savepoint", 1),
        "publish-hold" | "publish-concurrent" => ("invoice.concurrent", 1),
        "publish-schema-two" => ("invoice.paid", 2),
        _ => ("invoice.paid", 1),
    };
    let topic = EventType::new(name, version).map_err(super::internal)?;
    let payload = serde_json::from_slice(data).map_err(super::internal)?;
    let options = PublishOptions {
        lifetime_seconds: 120,
        connection: Some("database".into()),
        queue: Some("webhook-outgoing".into()),
    };
    let result = if matches!(
        mode,
        "publish-rollback" | "publish-savepoint" | "publish-hold"
    ) {
        let mut tx = connection
            .begin_transaction()
            .await
            .map_err(super::internal)?;
        let published = store
            .publish_in_transaction(&mut tx, key, &topic, payload, &options, policy)
            .await;
        if mode == "publish-hold"
            && let Ok(receipt) = &published
        {
            let root = std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
            tokio::fs::write(root.join("publication-ready"), &receipt.message_id)
                .await
                .map_err(super::internal)?;
            tokio::time::timeout(std::time::Duration::from_secs(12), async {
                while !root.join("publication-release").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .map_err(super::internal)?;
        }
        if mode == "publish-rollback" {
            tx.roll_back(None).await.map_err(super::internal)?;
        } else {
            tx.commit().await.map_err(super::internal)?;
        }
        published
    } else {
        store.publish(key, &topic, payload, &options, policy).await
    };
    match result {
        Ok(receipt) => Response::json(StatusCode::ACCEPTED, receipt),
        Err(Error::IdempotencyConflict) => Response::json(StatusCode::CONFLICT, json!({})),
        Err(Error::FanoutLimit | Error::TargetDenied | Error::InvalidPayload) => {
            Response::json(StatusCode::UNPROCESSABLE_ENTITY, json!({}))
        }
        Err(error) => Err(super::internal(error)),
    }
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

pub(super) fn commands(artisan: &mut Artisan) -> std::result::Result<(), ConsoleError> {
    let command = clap::Command::new("webhooks:fixture-endpoint")
        .arg(clap::Arg::new("operation").required(true))
        .arg(clap::Arg::new("name").required(true))
        .arg(
            clap::Arg::new("revision")
                .long("revision")
                .default_value("1")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(clap::Arg::new("scope").long("scope").default_value(SCOPE))
        .arg(
            clap::Arg::new("rotated")
                .long("rotated")
                .action(clap::ArgAction::SetTrue),
        );
    let command = command
        .arg(
            clap::Arg::new("topic")
                .long("topic")
                .default_value("invoice.paid"),
        )
        .arg(
            clap::Arg::new("schema")
                .long("schema")
                .default_value("1")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            clap::Arg::new("backup")
                .long("backup")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("untrusted")
                .long("untrusted")
                .action(clap::ArgAction::SetTrue),
        );
    artisan.add_command(Box::new(FixtureEndpointCommand(CommandDefinition::new(
        command,
    ))))
}

struct FixtureEndpointCommand(CommandDefinition);

impl Command for FixtureEndpointCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, std::result::Result<i32, ConsoleError>> {
        Box::pin(async move {
            let input = input.matches();
            let name = input.get_one::<String>("name").unwrap();
            let scope = input.get_one::<String>("scope").unwrap();
            let revision = *input.get_one::<u32>("revision").unwrap();
            let manager = app
                .make::<DatabaseManager>()
                .await
                .map_err(ConsoleError::from_source)?;
            let connection = manager
                .connection(None)
                .map_err(ConsoleError::from_source)?;
            let operation = input.get_one::<String>("operation").unwrap();
            if operation == "hold-before-prune" {
                let root = std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
                let mut tx = connection
                    .begin_transaction()
                    .await
                    .map_err(ConsoleError::from_source)?;
                let count = tx
                    .table("webhook_deliveries")
                    .r#where("id", "=", name.as_str())
                    .update(
                        [("retention_hold_at".into(), 1_i64.into())]
                            .into_iter()
                            .collect(),
                    )
                    .await
                    .map_err(ConsoleError::from_source)?;
                if count != 1 {
                    return Err(ConsoleError::from_source(Error::InvalidRecord));
                }
                tokio::fs::write(root.join("prune-hold-ready"), b"ready")
                    .await
                    .map_err(ConsoleError::from_source)?;
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    while !root.join("prune-hold-release").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .map_err(ConsoleError::from_source)?;
                tx.commit().await.map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            let deliveries =
                DeliveryStore::new(connection, scope).map_err(ConsoleError::from_source)?;
            let store = deliveries.endpoints();
            let (mut url, _) = target().map_err(ConsoleError::from_source)?;
            if input.get_flag("backup") {
                url.push_str("?endpoint=backup");
            }
            if input.get_flag("untrusted") {
                url = url.replace("/receiver", "/denied");
            }
            let credential = if input.get_flag("rotated") {
                ROTATED_REFERENCE
            } else {
                REFERENCE
            };
            let topic = EventType::new(
                input.get_one::<String>("topic").unwrap(),
                *input.get_one::<u32>("schema").unwrap(),
            )
            .map_err(ConsoleError::from_source)?;
            let result = match operation.as_str() {
                "subscribe" | "unsubscribe" => {
                    let endpoint = store
                        .find(name)
                        .await
                        .map_err(ConsoleError::from_source)?
                        .ok_or_else(|| ConsoleError::from_source(Error::EndpointRevoked))?;
                    if operation == "subscribe" {
                        deliveries
                            .subscribe(&topic, &endpoint)
                            .await
                            .map(|()| revision)
                    } else {
                        deliveries
                            .unsubscribe(&topic, &endpoint)
                            .await
                            .map(|_| revision)
                    }
                }
                "bulk" => {
                    for index in 0..33 {
                        let target = store
                            .create(&format!("{name}-{index}"), &url, Some(credential))
                            .await
                            .map_err(ConsoleError::from_source)?;
                        deliveries
                            .subscribe(&topic, &target)
                            .await
                            .map_err(ConsoleError::from_source)?;
                    }
                    Ok(revision)
                }
                "create" => store
                    .create(name, &url, Some(credential))
                    .await
                    .map(|endpoint| endpoint.revision()),
                "replace" => store.replace(name, revision, &url, Some(credential)).await,
                "pause" => store.pause(name, revision).await.map(|()| revision),
                "resume" => store.resume(name, revision).await.map(|()| revision),
                "delete" => store.delete(name, revision).await.map(|()| revision),
                _ => Err(Error::InvalidConfig("fixture.endpoint.operation")),
            };
            let revision = result.map_err(ConsoleError::from_source)?;
            output.info(&format!("revision {revision}"));
            Ok(0)
        })
    }
}

pub(super) fn inspect(
    request: &mut Request,
) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        let scope = request.route("scope").ok_or(HttpError::NotFound)?;
        if scope != SCOPE
            || request
                .headers()
                .get("x-fixture-inspector")
                .and_then(|v| v.to_str().ok())
                != Some("fixture-operator")
        {
            return Response::json(StatusCode::FORBIDDEN, json!({}));
        }
        let id = request.route("id").ok_or(HttpError::NotFound)?;
        let operation = request.route("kind").ok_or(HttpError::NotFound)?;
        let manager = super::Databases::get_facade_root().map_err(super::internal)?;
        let store = DeliveryStore::new(manager.connection(None).map_err(super::internal)?, scope)
            .map_err(super::internal)?;
        let headers = request.headers();
        let options = |header, default| {
            headers.get(header).map_or(Ok(default), |value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or(Error::InvalidConfig("fixture"))
            })
        };
        let result = async {
            let result = match operation {
                "delivery" => serde_json::to_value(store.inspect_delivery(id).await?),
                "message" => serde_json::to_value(store.inspect_message(id).await?),
                "attempts" => serde_json::to_value(
                    store
                        .inspect_attempts(
                            id,
                            options("x-fixture-after", 0)?,
                            options("x-fixture-limit", 50)?,
                        )
                        .await?,
                ),
                _ => return Err(Error::InvalidConfig("fixture")),
            };
            result.map_err(|_| Error::InvalidRecord)
        }
        .await;
        match result {
            Ok(serde_json::Value::Null) => Response::json(StatusCode::NOT_FOUND, json!({})),
            Ok(value) => Response::json(StatusCode::OK, value),
            Err(Error::InvalidConfig(_) | Error::InvalidRecord) => {
                Response::json(StatusCode::UNPROCESSABLE_ENTITY, json!({}))
            }
            Err(error) => Err(super::internal(error)),
        }
    })
}
