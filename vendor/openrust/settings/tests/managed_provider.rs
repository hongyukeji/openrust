mod support;
use openrust::{
    cache::{CacheManager, Value},
    contracts::{cache::Repository as _, config::Repository as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_settings::{Settings, SettingsError, SettingsManager, SettingsServiceProvider};
use serde_json::json;
use support::{Fixture, Site};
struct Host {
    database: serde_json::Value,
    table: String,
}
impl ServiceProvider for Host {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app.config()?;
            let mut database = self.database.clone();
            if let Ok(url) = std::env::var("SETTINGS_TEST_REDIS_URL") {
                database["redis"] = json!({"default":{"url":url,"max_retries":0}});
                config.set("cache",json!({"default":"shared","stores":{"shared":{"driver":"redis","connection":"default","prefix":"settings-provider:"}}}))?;
            } else {
                config.set(
                    "cache",
                    json!({"default":"shared","stores":{"shared":{"driver":"array"}}}),
                )?;
            }
            config.set("database", database)?;
            config.set("settings.repositories.database.table", json!(self.table))?;
            config.set("settings.refresh_interval", json!(1))?;
            SettingsServiceProvider::register_settings::<Site>(app).await?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn default_provider_uses_one_configuration_and_public_settings_api_for_multiple_apps() {
    let f = Fixture::new().await;
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let mut apps = Vec::new();
    for root in &roots {
        let mut app = Application::configure(root.path().to_owned())
            .with_providers(vec![
                Box::new(SettingsServiceProvider),
                Box::new(Host {
                    database: f.config.get("database").unwrap(),
                    table: f.table.clone(),
                }),
            ])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        apps.push(app);
    }
    let a = apps[0].make::<Settings<Site>>().await.unwrap();
    let b = apps[1].make::<Settings<Site>>().await.unwrap();
    assert_eq!(b.get().await.name, "Default");
    let manager = apps[1].make::<SettingsManager>().await.unwrap();
    let cache = apps[1]
        .make::<CacheManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    let projection = format!("{}:rendered", f.table);
    cache
        .put(&projection, &Value::Json(json!("old projection")), None)
        .await
        .unwrap();
    let captured = cache.clone();
    let key = projection.clone();
    manager
        .on_change("site", move |_change| {
            let cache = captured.clone();
            let key = key.clone();
            Box::pin(async move {
                cache
                    .forget(&key)
                    .await
                    .map_err(|_| SettingsError::Refresh)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    a.fill(json!({"name":"provider update"})).await.unwrap();
    a.save().await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let state = manager.status("site").await.unwrap();
        if state.observed_version.is_some() && state.pending_handlers == 0 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(cache.get(&projection).await.unwrap(), None);
    assert_eq!(b.get().await.name, "Default");
    b.refresh().await.unwrap();
    assert_eq!(b.get().await.name, "provider update");
    for app in &apps {
        app.make::<SettingsManager>()
            .await
            .unwrap()
            .shutdown()
            .await;
    }
    drop((a, b, manager, apps));
    f.close().await;
}

#[tokio::test]
async fn unknown_cache_store_fails_configuration_and_disabled_cache_is_explicit() {
    let f = Fixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .with_providers(vec![
            Box::new(SettingsServiceProvider),
            Box::new(Host {
                database: f.config.get("database").unwrap(),
                table: f.table.clone(),
            }),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.config()
        .unwrap()
        .set("settings.cache.store", json!("missing-settings-cache"))
        .unwrap();
    assert!(app.make::<SettingsManager>().await.is_err());
    app.config()
        .unwrap()
        .set("settings.cache.enabled", json!(false))
        .unwrap();
    let manager = app.make::<SettingsManager>().await.unwrap();
    assert_eq!(
        app.make::<Settings<Site>>().await.unwrap().get().await.name,
        "Default"
    );
    manager.shutdown().await;
    drop(app);
    f.close().await;
}
