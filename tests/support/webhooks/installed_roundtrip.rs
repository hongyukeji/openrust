use std::{net::SocketAddr, sync::Arc, time::Duration};

use openrust::{
    console::{Application as Artisan, AsyncCommandCallback, ConsoleError},
    contracts::foundation::Application as _,
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    queue::{JobContext, JobOptions, QueueManager},
    routing::RoutingError,
    support::{ProviderResult, ServiceProvider, facades::Route},
};
use openrust_webhook_client::{
    DatabaseWebhookStore, WebhookRouteOptions, WebhookRoutes, WebhookRuntime, WebhookStore,
    jobs::{ProcessWebhookJob, QueuedWebhookJob},
    models::WebhookCall as Receipt,
};
use openrust_webhook_server::{
    DeliveryContext, DeliveryPermission, DeliveryPolicy, DeliveryService, DeliveryStore,
    TargetPolicy, WebhookCall, register_delivery_policy,
};

const SCOPE: &str = "installed-webhooks";
const RECEIVE_QUEUE: &str = "installed-receive";
const SEND_QUEUE: &str = "installed-send";
const HANDLER: &str = "installed-handler";

fn credential(protocol: &str) -> Option<&'static str> {
    match protocol {
        "spatie" => Some("FIXTURE_INSTALLED_SPATIE_SECRET"),
        "standard" => Some("FIXTURE_INSTALLED_STANDARD_SECRET"),
        _ => None,
    }
}

fn destination(protocol: &str) -> openrust_webhook_server::Result<(String, TargetPolicy)> {
    let port = std::env::var("FIXTURE_INSTALLED_PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .ok_or(openrust_webhook_server::Error::TargetDenied)?;
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let url = format!("http://{address}/api/installed-webhooks/{protocol}");
    let policy = TargetPolicy::trusted_endpoint(&url, vec![address])?;
    Ok((url, policy))
}

struct InstalledPolicy;

impl DeliveryPolicy for InstalledPolicy {
    fn authorize<'a>(
        &'a self,
        context: DeliveryContext<'a>,
    ) -> HttpFuture<'a, openrust_webhook_server::Result<Option<DeliveryPermission>>> {
        Box::pin(async move {
            let Some(protocol) = context.credential else {
                return Ok(None);
            };
            let Some(reference) = credential(protocol) else {
                return Ok(None);
            };
            let (url, policy) = destination(protocol)?;
            if context.scope != SCOPE || context.url != url {
                return Ok(None);
            }
            Ok(std::env::var(reference)
                .ok()
                .map(|secret| DeliveryPermission::new(policy).with_secret(secret)))
        })
    }
}

struct WriteProcessedReceipt;

impl ProcessWebhookJob for WriteProcessedReceipt {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        receipt: &'a Receipt,
        _: &'a dyn WebhookStore,
    ) -> HttpFuture<'a, openrust_webhook_client::Result<()>> {
        Box::pin(async move {
            let write = || -> std::io::Result<()> {
                use std::io::Write as _;
                let path = std::env::var_os("FIXTURE_INSTALLED_RECEIPTS")
                    .ok_or_else(|| std::io::Error::other("缺少隔离验收记录路径"))?;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?;
                let record = serde_json::json!({
                    "id": receipt.id(), "name": receipt.name(), "body": receipt.raw_body(),
                });
                writeln!(file, "{record}")
            };
            write().map_err(|source| openrust_webhook_client::Error::Runtime {
                source: Box::new(source),
            })
        })
    }
}

pub(crate) struct InstalledWebhookProvider;

impl ServiceProvider for InstalledWebhookProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let manager = app.make::<DatabaseManager>().await?;
            let queue = app.make::<QueueManager>().await?;
            let connection = manager.connection(None)?;
            let store = Arc::new(DatabaseWebhookStore::new(connection.clone(), SCOPE)?);
            QueuedWebhookJob::register_with_database_lease(
                &queue,
                HANDLER,
                store,
                Arc::new(WriteProcessedReceipt),
                20,
            )?;
            let dispatcher = Arc::new(
                QueuedWebhookJob::new(HANDLER)?
                    .on_connection("database")
                    .on_queue(RECEIVE_QUEUE)
                    .with_options(JobOptions {
                        max_tries: Some(2),
                        timeout: Some(Duration::from_secs(8)),
                        ..Default::default()
                    }),
            );
            WebhookRuntime::register_job_dispatcher(app.container(), HANDLER, move |_, _| {
                Ok(dispatcher.clone())
            })?;
            let policy = Arc::new(InstalledPolicy);
            register_delivery_policy(app.container(), policy.clone())?;
            DeliveryService::new(DeliveryStore::new(connection, SCOPE)?, policy)
                .register_worker(&queue, "default")?;
            Ok(())
        })
    }
}

pub(crate) fn routes() -> Result<(), RoutingError> {
    for protocol in ["spatie", "standard"] {
        Route::webhooks(
            &format!("/installed-webhooks/{protocol}"),
            WebhookRouteOptions::new().name(protocol),
        )?;
    }
    Ok(())
}

pub(crate) fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.command_with(
        "webhooks:installed-send {protocol} {key}",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let protocol = input
                    .argument("protocol")
                    .ok_or_else(|| ConsoleError::new("需要验收协议"))?;
                let protocol = protocol
                    .as_str()
                    .ok_or_else(|| ConsoleError::new("验收协议须为字符串"))?;
                if credential(protocol).is_none() {
                    return Err(ConsoleError::new("未知验收协议"));
                }
                let key = input
                    .argument("key")
                    .ok_or_else(|| ConsoleError::new("需要验收标识"))?;
                let key = key
                    .as_str()
                    .ok_or_else(|| ConsoleError::new("验收标识须为字符串"))?;
                let (url, target) = destination(protocol).map_err(ConsoleError::from_source)?;
                let manager = app
                    .make::<DatabaseManager>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                let connection = manager
                    .connection(None)
                    .map_err(ConsoleError::from_source)?;
                let queue = app
                    .make::<QueueManager>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                let store =
                    DeliveryStore::new(connection, SCOPE).map_err(ConsoleError::from_source)?;
                let service = DeliveryService::new(store, Arc::new(InstalledPolicy));
                let body = serde_json::to_vec(&serde_json::json!({"key": key}))
                    .map_err(ConsoleError::from_source)?;
                let queued = WebhookCall::create()
                    .map_err(ConsoleError::from_source)?
                    .url(url)
                    .uuid(key)
                    .send_raw_body(body)
                    .sign_using(if protocol == "standard" {
                        "standard"
                    } else {
                        "default"
                    })
                    .use_secret_reference(protocol)
                    .on_connection(Some("database".into()))
                    .on_queue(Some(SEND_QUEUE.into()))
                    .dispatch(&service, &queue, &target, key, 120)
                    .await
                    .map_err(ConsoleError::from_source)?;
                output
                    .write_raw(&format!("{}\n", queued.delivery_id))
                    .map_err(ConsoleError::from_source)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
