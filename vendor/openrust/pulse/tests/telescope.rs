#![cfg(feature = "telescope")]
use openrust::{
    contracts::{
        cache::Repository as _,
        events::{Dispatcher as _, Event, Listener},
    },
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
fn config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    std::fs::write(root.join("metrics.sqlite"), []).unwrap();
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing","debug":true})),
        (
            "database".into(),
            json!({"default":"test","connections":{"test":{"driver":"sqlite","database":root.join("metrics.sqlite"),"max_connections":4,"busy_timeout":5000}}}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":root.join("cache/data"),"lock_path":root.join("cache/locks")}}}),
        ),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
        ("pulse".into(), json!({"recorders":{"Servers":false}})),
    ]))
}
struct BeatLog;
impl Listener for BeatLog {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async {
            openrust::support::facades::Log::error("pulse loop probe", &Default::default())
                .map_err(|error| openrust::events::EventError::Listener {
                    source: Box::new(error),
                })?;
            Ok(None)
        })
    }
}
struct ProbeProvider;
impl ServiceProvider for ProbeProvider {
    fn boot<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<openrust::events::Dispatcher>().await?.listen(
                "OpenRust\\Pulse\\Events\\SharedBeat",
                Arc::new(BeatLog),
                Delivery::Sync,
            )?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn long_running_check_stores_telescope_each_iteration_and_exits_on_shared_restart() {
    let mut app = TestApp::create(|b| {
        b.withConfig(config).withProviders(vec![
            Box::new(openrust_pulse::PulseServiceProvider),
            Box::new(openrust_telescope::TelescopeServiceProvider),
            Box::new(ProbeProvider),
        ])
    })
    .unwrap();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let db = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let cache = app
        .app()
        .make::<openrust::cache::CacheManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    let task = tokio::spawn(async move {
        app.artisan(["pulse:check"]).await.assertSuccessful();
        app
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if db
                .table("telescope_entries")
                .r#where("type", "=", "log")
                .r#where("content", "like", "%pulse loop probe%")
                .count("*")
                .await
                .unwrap()
                > 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Telescope must persist before the long command exits");
    assert!(!task.is_finished());
    cache
        .forever("openrust:pulse:restart", &openrust::cache::Value::from(1))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
}
