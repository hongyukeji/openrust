use std::sync::Arc;

use openrust::{
    contracts::foundation::Application as _,
    database::DatabaseManager,
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::QueueManager,
    support::{ProviderResult, facades::Facade},
};
use openrust_webhook_server::{
    BackoffStrategy, DeliveryContext, DeliveryPermission, DeliveryPolicy, DeliveryService,
    DeliveryStore, Error, Result, TargetPolicy, WebhookCall, register_backoff_strategy,
    register_delivery_policy,
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

pub(super) async fn register(app: &Application) -> ProviderResult {
    if std::env::var_os("FIXTURE_SEND_TARGET").is_some() {
        let policy = Arc::new(FixturePolicy);
        register_delivery_policy(app.container(), policy.clone())?;
        register_backoff_strategy(app.container(), "fixture-rapid", Arc::new(FixtureBackoff))?;
        let manager = app.make::<DatabaseManager>().await?;
        let queue = app.make::<QueueManager>().await?;
        let store = DeliveryStore::new(manager.connection(None)?, SCOPE)?;
        DeliveryService::new(store, policy).register_worker(&queue, "default")?;
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
            .on_connection(Some("database".into()))
            .on_queue(Some("webhook-outgoing".into()))
            .timeout_in_seconds(if mode == "timeout" { 1 } else { 4 })
            .maximum_tries(2);
        if mode == "inline" {
            call = call.use_secret(SECRET);
        }
        if mode == "exclude-503" {
            call = call.retry_on(Some(vec![StatusCode::TOO_MANY_REQUESTS]));
        }
        let prepared = call.prepare(&policy).map_err(super::internal)?;
        let manager = super::Databases::get_facade_root().map_err(super::internal)?;
        let connection = manager.connection(None).map_err(super::internal)?;
        let store = DeliveryStore::new(connection.clone(), SCOPE).map_err(super::internal)?;
        let lifetime = if mode == "expired" { 1 } else { 120 };
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
