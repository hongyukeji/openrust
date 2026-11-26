use super::{EventListener, NativeWatcher};
use openrust::{
    contracts::{events::Dispatcher as _, foundation::Application as _},
    events::Delivery,
    foundation::Application,
    http::HttpFuture,
    support::ProviderResult,
};
use serde_json::{Map, Value};
use std::sync::Arc;

/// A watcher owns its options and registers only its own native observers.
pub trait Watcher: Send + Sync {
    fn register<'a>(&'a self, app: &'a Application) -> HttpFuture<'a, ProviderResult>;
}

macro_rules! watchers {
    ($($name:ident => [$($event:expr),* $(,)?]),* $(,)?) => {
        $(
            #[derive(Clone, Default)]
            pub struct $name {
                pub options: Map<String, Value>,
            }
            impl $name {
                pub fn new(options: Map<String, Value>) -> Self { Self { options } }
            }
            impl Watcher for $name {
                fn register<'a>(&'a self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
                    Box::pin(register_watcher(app, stringify!($name), self.options.clone(), &[$($event),*]))
                }
            }
        )*
        pub(crate) async fn register_all(app: &Application) -> ProviderResult {
            let config = app.config()?;
            let dispatcher = app.make::<openrust::events::Dispatcher>().await?;
            let lifecycle = Arc::new(NativeWatcher { config: config.clone(), only: None });
            for name in [openrust::foundation::http::events::RequestReceived::NAME, openrust::console::events::CommandStarting::NAME] {
                dispatcher.listen(name, lifecycle.clone(), Delivery::Sync)?;
            }
            $(
                let options = match config.get(concat!("telescope.watchers.", stringify!($name))) {
                    Some(Value::Bool(true)) => Some(Map::new()),
                    Some(Value::Object(options)) if options.get("enabled").and_then(Value::as_bool).unwrap_or(true) => Some(options),
                    _ => None,
                };
                if let Some(options) = options { $name::new(options).register(app).await?; }
            )*
            Ok(())
        }
    };
}
watchers! {
    BatchWatcher => [openrust::bus::events::BatchDispatched::NAME, openrust::bus::events::BatchStarted::NAME, openrust::bus::events::BatchFinished::NAME, openrust::bus::events::BatchCanceled::NAME],
    CacheWatcher => [openrust::cache::events::CacheHit::NAME, openrust::cache::events::CacheMissed::NAME, openrust::cache::events::KeyWritten::NAME, openrust::cache::events::KeyForgotten::NAME],
    ClientRequestWatcher => ["OpenRust\\Http\\Client\\Events\\ResponseReceived", "OpenRust\\Http\\Client\\Events\\ConnectionFailed"],
    CommandWatcher => [openrust::console::events::CommandFinished::NAME],
    DumpWatcher => ["OpenRust\\Support\\Events\\Dumped"],
    EventWatcher => [],
    ExceptionWatcher => [],
    GateWatcher => ["GateEvaluated"],
    JobWatcher => [openrust::queue::events::JobQueueing::NAME, openrust::queue::events::JobProcessing::NAME, openrust::queue::events::JobProcessed::NAME, openrust::queue::events::JobFailed::NAME],
    LogWatcher => [],
    MailWatcher => ["MessageSent"],
    ModelWatcher => ["eloquent.*"],
    NotificationWatcher => ["NotificationSent"],
    QueryWatcher => [openrust::database::events::QueryExecuted::NAME],
    RedisWatcher => ["OpenRust\\Redis\\Events\\CommandExecuted"],
    RequestWatcher => ["OpenRust\\Foundation\\Http\\Events\\RequestHandled"],
    ScheduleWatcher => ["console.scheduled_task_completed"],
    ViewWatcher => [],
}
async fn register_watcher(
    app: &Application,
    name: &'static str,
    options: Map<String, Value>,
    events: &[&str],
) -> ProviderResult {
    if options.get("enabled").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }
    let watcher = NativeWatcher {
        config: app.config()?,
        only: Some((name, options)),
    };
    match name {
        "LogWatcher" | "ExceptionWatcher" => super::register_logging(
            app.make::<openrust::log::LogManager>().await?.as_ref(),
            watcher,
        )?,
        "ViewWatcher" => super::register_views(
            app.make::<openrust::view::Factory>().await?.as_ref(),
            watcher,
            app.base_path().to_owned(),
        ),
        "EventWatcher" => app.make::<openrust::events::Dispatcher>().await?.listen(
            "*",
            Arc::new(EventListener { watcher }),
            Delivery::Sync,
        )?,
        _ => {
            let dispatcher = app.make::<openrust::events::Dispatcher>().await?;
            let listener = Arc::new(watcher);
            for event in events {
                dispatcher.listen(event, listener.clone(), Delivery::Sync)?;
            }
        }
    }
    Ok(())
}
