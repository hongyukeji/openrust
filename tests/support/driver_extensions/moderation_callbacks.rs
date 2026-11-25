use std::{path::PathBuf, sync::Arc, time::Duration};

use openrust::{
    cache::RateLimiter,
    config::{ConfigError, Environment},
    console::{Application as Artisan, AsyncCommandCallback, ConsoleError},
    contracts::{container::Container as _, foundation::Application as _},
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    queue::{JobContext, JobOptions, QueueManager},
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use openrust_content_moderation::{
    AliyunCallbackProcessor, HiveLookupHint, ModerationManager, ModerationStore,
    SightengineCallbackProcessor, TencentCallbackProcessor,
};
use openrust_webhook_client::{
    DatabaseWebhookStore, Error, Result, WebhookRouteOptions, WebhookRoutes as _, WebhookRuntime,
    WebhookStore,
    hints::QueuedWebhookHint,
    jobs::{DeferredProcessingPolicy, ProcessWebhookJob, QueuedWebhookJob},
    models::WebhookCall,
};
use serde_json::{Value, json};

const JOB: &str = "moderation-callback-fixture";
const SCOPE: &str = "moderation-callback-scope";
const HIVE_HINT: &str = "moderation-hive-fixture";
const HIVE_ROUTE: &str = "hive-video-hint";

pub(super) fn configuration(env: &Environment) -> std::result::Result<Value, ConfigError> {
    if env.get_utf8("MODERATION_CALLBACK_FIXTURE_HIVE")? == Some("1") {
        return Ok(json!({
            "configs": [{
                "name": HIVE_ROUTE, "revision": "callbacks-v1", "scope": SCOPE,
                "signature_validator": "moderation-hive-hint", "webhook_profile": "process-everything",
                "process_webhook_job": JOB, "webhook_model": "database",
                "unverified_hint_receiver": HIVE_HINT, "verification_audit": true, "max_body_bytes": 4096
            }]
        }));
    }
    if env.get_utf8("MODERATION_CALLBACK_FIXTURE_ALIYUN")? == Some("1") {
        return Ok(json!({
            "configs": [
                {
                    "name": "aliyun-video", "revision": "callbacks-v1", "scope": SCOPE,
                    "signature_validator": "moderation-aliyun", "webhook_profile": "process-everything",
                    "process_webhook_job": JOB, "webhook_model": "database",
                    "moderation_callback_account_uid": "1234500000000000",
                    "moderation_callback_credential": {"name":"aliyun-video", "revision":"r2"},
                    "moderation_previous_callback_credentials": [{"name":"aliyun-video", "revision":"r1"}],
                    "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 4096
                },
                {
                    "name": "aliyun-other-account", "revision": "callbacks-v1", "scope": SCOPE,
                    "signature_validator": "moderation-aliyun", "webhook_profile": "process-everything",
                    "process_webhook_job": JOB, "webhook_model": "database",
                    "moderation_callback_account_uid": "9999900000000000",
                    "moderation_callback_credential": {"name":"aliyun-other", "revision":"r1"},
                    "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 4096
                }
            ]
        }));
    }
    if env.get_utf8("MODERATION_CALLBACK_FIXTURE_TENCENT")? == Some("1") {
        return Ok(json!({
            "configs": [
                {
                    "name": "tencent-video", "revision": "callbacks-v1", "scope": SCOPE,
                    "signature_validator": "moderation-tencent", "webhook_profile": "process-everything",
                    "process_webhook_job": JOB, "webhook_model": "database",
                    "moderation_callback_credential": {"name":"tencent-video", "revision":"r2"},
                    "moderation_previous_callback_credentials": [{"name":"tencent-video", "revision":"r1"}],
                    "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 4096
                },
                {
                    "name": "tencent-other-account", "revision": "callbacks-v1", "scope": SCOPE,
                    "signature_validator": "moderation-tencent", "webhook_profile": "process-everything",
                    "process_webhook_job": JOB, "webhook_model": "database",
                    "moderation_callback_credential": {"name":"tencent-other", "revision":"r1"},
                    "atomic_database_queue": true, "deduplicate_body_seconds": 60, "max_body_bytes": 4096
                }
            ]
        }));
    }
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
    Route::webhooks(
        "/drivers/moderation/tencent-callback",
        WebhookRouteOptions::new().name("tencent-video"),
    )?;
    Route::webhooks(
        "/drivers/moderation/tencent-callback-other",
        WebhookRouteOptions::new().name("tencent-other-account"),
    )?;
    Route::webhooks(
        "/drivers/moderation/aliyun-callback",
        WebhookRouteOptions::new().name("aliyun-video"),
    )?;
    Route::webhooks(
        "/drivers/moderation/aliyun-callback-other",
        WebhookRouteOptions::new().name("aliyun-other-account"),
    )?;
    Route::webhooks(
        "/drivers/moderation/hive-video-hint",
        WebhookRouteOptions::new().name(HIVE_ROUTE),
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
                        .on_queue(JOB)
                        .with_options(JobOptions {
                            timeout: Some(Duration::from_secs(5)),
                            ..JobOptions::default()
                        }),
                ))
            })?;
            let connection = app.make::<DatabaseManager>().await?.connection(None)?;
            let store = Arc::new(DatabaseWebhookStore::new(connection.clone(), SCOPE)?);
            let queue = app.make::<QueueManager>().await?;
            if std::env::var_os("MODERATION_CALLBACK_FIXTURE_HIVE").is_some() {
                let manager = app.make::<ModerationManager>().await?;
                let profile = manager.pin_profile("hive-videos", "original")?;
                let reviews = ModerationStore::new(connection.clone(), "app", "tenant")?;
                let hint = Arc::new(HiveLookupHint::new(
                    reviews,
                    &manager,
                    profile,
                    SCOPE,
                    HIVE_ROUTE,
                    "callbacks-v1",
                )?);
                QueuedWebhookHint::new(
                    connection.clone(),
                    SCOPE,
                    HIVE_ROUTE,
                    HIVE_HINT,
                    "database",
                    HIVE_HINT,
                )?
                .register(&queue, hint.clone())?;
                WebhookRuntime::register_unverified_hint_receiver(
                    app.container(),
                    HIVE_HINT,
                    move |config, container| {
                        let database = container.make::<DatabaseManager>()?.connection(None)?;
                        let limiter = container.make::<RateLimiter>()?;
                        Ok(Arc::new(
                            QueuedWebhookHint::new(
                                database,
                                SCOPE,
                                config.name(),
                                HIVE_HINT,
                                "database",
                                HIVE_HINT,
                            )?
                            .receiver(limiter, 15, hint.clone())?,
                        ))
                    },
                )?;
            }
            if std::env::var_os("MODERATION_CALLBACK_FIXTURE_ASSOCIATE").is_some() {
                let reviews = ModerationStore::new(connection, "app", "tenant")?;
                let processor: Arc<dyn ProcessWebhookJob> =
                    if std::env::var_os("MODERATION_CALLBACK_FIXTURE_TENCENT").is_some() {
                        Arc::new(TencentCallbackProcessor::new(
                            reviews,
                            "original-account",
                            "video_profile",
                            SCOPE,
                            "tencent-video",
                            "callbacks-v1",
                        )?)
                    } else if std::env::var_os("MODERATION_CALLBACK_FIXTURE_ALIYUN").is_some() {
                        Arc::new(AliyunCallbackProcessor::new(
                            reviews,
                            "original-account",
                            SCOPE,
                            "aliyun-video",
                            "callbacks-v1",
                        )?)
                    } else {
                        Arc::new(SightengineCallbackProcessor::new(
                            reviews,
                            "original-account",
                            SCOPE,
                            "sightengine-video",
                            "callbacks-v1",
                        )?)
                    };
                QueuedWebhookJob::register_with_database_deferred_processing(
                    &queue,
                    JOB,
                    store,
                    processor,
                    20,
                    DeferredProcessingPolicy {
                        max_deferrals: 4,
                        max_age_seconds: 90,
                    },
                )?;
            } else {
                QueuedWebhookJob::register(&queue, JOB, store, Arc::new(CallbackFixture))?;
            }
            Ok(())
        })
    }
}

pub(super) fn commands(artisan: &mut Artisan) -> std::result::Result<(), ConsoleError> {
    artisan.command_with(
        "drivers:moderation-wake-callback {id}",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let id = input
                    .argument("id")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or_else(|| ConsoleError::new("缺少验收回执 ID"))?;
                let queue = app
                    .make::<QueueManager>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                let connection = app
                    .make::<DatabaseManager>()
                    .await
                    .map_err(ConsoleError::from_source)?
                    .connection(None)
                    .map_err(ConsoleError::from_source)?;
                let store = DatabaseWebhookStore::new(connection, SCOPE)
                    .map_err(ConsoleError::from_source)?;
                let woken = QueuedWebhookJob::wake_deferred(&store, queue, &id)
                    .await
                    .map_err(ConsoleError::from_source)?;
                output
                    .write_raw(&format!("{woken}\n"))
                    .map_err(ConsoleError::from_source)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
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
