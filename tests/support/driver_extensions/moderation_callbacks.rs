use std::{path::PathBuf, sync::Arc};

use openrust::{
    config::{ConfigError, Environment},
    contracts::foundation::Application as _,
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    queue::{JobContext, QueueManager},
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use openrust_webhook_client::{
    DatabaseWebhookStore, Error, Result, WebhookRouteOptions, WebhookRoutes as _, WebhookRuntime,
    WebhookStore,
    jobs::{ProcessWebhookJob, QueuedWebhookJob},
    models::WebhookCall,
};
use serde_json::{Value, json};

const JOB: &str = "moderation-callback-fixture";
const SCOPE: &str = "moderation-callback-scope";

pub(super) fn configuration(env: &Environment) -> std::result::Result<Value, ConfigError> {
    let revision = env
        .get_utf8("MODERATION_CALLBACK_FIXTURE_REVISION")?
        .unwrap_or("r2");
    let previous = if env.get_utf8("MODERATION_CALLBACK_FIXTURE_RETIRED")? == Some("1") {
        vec![]
    } else {
        vec![json!({"name":"sight-callback", "revision":"r1"})]
    };
    Ok(json!({
        "configs": [
            {
                "name": "sightengine-video", "revision": "callbacks-v1", "scope": SCOPE,
                "signature_validator": "moderation-sightengine", "webhook_profile": "process-everything",
                "process_webhook_job": JOB, "webhook_model": "database",
                "moderation_callback_credential": {"name":"sight-callback", "revision":revision},
                "moderation_previous_callback_credentials": previous,
                "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 2048
            },
            {
                "name": "other-account", "revision": "callbacks-v1", "scope": SCOPE,
                "signature_validator": "moderation-sightengine", "webhook_profile": "process-everything",
                "process_webhook_job": JOB, "webhook_model": "database",
                "moderation_callback_credential": {"name":"other-callback", "revision":"r1"},
                "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 2048
            }
        ]
    }))
}

pub(super) fn routes() -> std::result::Result<(), openrust::routing::RoutingError> {
    Route::webhooks(
        "/drivers/moderation/callback",
        WebhookRouteOptions::new().name("sightengine-video"),
    )?;
    Route::webhooks(
        "/drivers/moderation/callback-other",
        WebhookRouteOptions::new().name("other-account"),
    )?;
    Ok(())
}

pub(super) struct CallbackFixtureProvider;

impl ServiceProvider for CallbackFixtureProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            WebhookRuntime::register_job_dispatcher(app.container(), JOB, |_, _| {
                Ok(Arc::new(
                    QueuedWebhookJob::new(JOB)?
                        .on_connection("database")
                        .on_queue(JOB),
                ))
            })?;
            let connection = app.make::<DatabaseManager>().await?.connection(None)?;
            let store = Arc::new(DatabaseWebhookStore::new(connection, SCOPE)?);
            let queue = app.make::<QueueManager>().await?;
            QueuedWebhookJob::register(queue.as_ref(), JOB, store, Arc::new(CallbackFixture))?;
            Ok(())
        })
    }
}

struct CallbackFixture;

impl ProcessWebhookJob for CallbackFixture {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        call: &'a WebhookCall,
        _: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            let path = std::env::var_os("MODERATION_CALLBACK_FIXTURE_RESULT")
                .map(PathBuf::from)
                .ok_or(Error::InvalidConfig("fixture result"))?;
            let body: Value =
                serde_json::from_slice(call.raw_body()).map_err(|_| Error::InvalidRecord)?;
            std::fs::write(
                path,
                json!({"id":call.id(), "media":body["media"]["id"]}).to_string(),
            )
            .map_err(|source| Error::Runtime {
                source: Box::new(source),
            })
        })
    }
}
