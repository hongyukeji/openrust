use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{MaintenanceModeManager, configuration::Routing, maintenance::MaintenanceError},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Cache, Config, MaintenanceMode, Route},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn controls(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        exercise().await.unwrap();
        Response::json(StatusCode::OK, json!({"ok":true}))
    })
}

async fn exercise() -> Result<(), MaintenanceError> {
    assert_eq!(MaintenanceMode::get_default_driver()?, "array");
    assert!(MaintenanceMode::get_drivers()?.contains_key("array"));
    assert!(Arc::ptr_eq(
        &MaintenanceMode::get_container()?,
        &openrust::support::facades::App::get_container()?
    ));
    let initial = MaintenanceMode::driver(None)?;
    assert!(Arc::ptr_eq(&initial, &MaintenanceMode::driver(Some(""))?));
    assert!(!MaintenanceMode::active().await?);
    assert_eq!(MaintenanceMode::data().await?, None);
    let payload = json!({"status":503,"retry":3}).as_object().unwrap().clone();
    MaintenanceMode::activate(&payload).await?;
    assert!(initial.active().await?);
    assert_eq!(MaintenanceMode::data().await?, Some(payload));
    MaintenanceMode::deactivate().await?;
    assert!(!initial.active().await?);
    let manager = MaintenanceMode::extend("alias", |container| {
        container
            .make::<MaintenanceModeManager>()?
            .driver(Some("array"))
    })?;
    assert!(Arc::ptr_eq(
        &initial,
        &MaintenanceMode::driver(Some("alias"))?
    ));
    assert_eq!(MaintenanceMode::get_drivers()?.len(), 2);
    MaintenanceMode::extend("loop", |container| {
        container
            .make::<MaintenanceModeManager>()?
            .driver(Some("loop"))
    })?;
    assert!(matches!(
        MaintenanceMode::driver(Some("loop")),
        Err(MaintenanceError::DriverResolutionCycle)
    ));
    assert!(matches!(
        MaintenanceMode::driver(Some("unsupported")),
        Err(MaintenanceError::InvalidDriver)
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    MaintenanceMode::extend("concurrent", move |container| {
        count.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        container
            .make::<MaintenanceModeManager>()?
            .driver(Some("array"))
    })?;
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let manager = manager.clone();
            std::thread::spawn(move || manager.driver(Some("concurrent")).unwrap())
        })
        .collect();
    for thread in threads {
        assert!(Arc::ptr_eq(&initial, &thread.join().unwrap()));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    MaintenanceMode::forget_drivers()?;
    assert!(MaintenanceMode::get_drivers()?.is_empty());
    assert!(!Arc::ptr_eq(&initial, &MaintenanceMode::driver(None)?));
    assert!(MaintenanceMode::driver(Some("alias")).is_ok());
    let container = MaintenanceMode::get_container()?;
    assert!(Arc::ptr_eq(
        &manager,
        &MaintenanceMode::set_container(&container)?
    ));
    Config::set("app.maintenance.driver", json!(null)).unwrap();
    assert!(matches!(
        MaintenanceMode::get_default_driver(),
        Err(MaintenanceError::InvalidDriver)
    ));
    Config::set("app.maintenance.driver", json!("array")).unwrap();
    Ok(())
}

fn normal(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::new(StatusCode::OK, Body::from("normal"))) })
}

fn configure(
    builder: openrust::foundation::configuration::ApplicationBuilder,
) -> openrust::foundation::configuration::ApplicationBuilder {
    builder
        .with_config(|_, _| {
            Ok(BTreeMap::from([
                (
                    "app".into(),
                    json!({"locale":"en","supported_locales":[],"maintenance":{"driver":"array"}}),
                ),
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
            ]))
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/control/check", controls)?;
            Route::get("/normal", normal)?;
            Ok(())
        }))
        .with_middleware(|middleware| {
            middleware.prevent_requests_during_maintenance(["control/*"]);
        })
}

#[tokio::test]
async fn maintenance_facade_uses_real_manager_custom_creators_and_concurrent_resolution() {
    let mut app = TestApp::create(configure).unwrap();
    assert_eq!(app.get("/control/check").await.unwrap().status(), 200);
    assert_eq!(app.artisan(["down"]).await.status, 0);
    assert_eq!(app.get("/normal").await.unwrap().status(), 503);
    assert_eq!(app.artisan(["up"]).await.status, 0);
    assert_eq!(app.get("/normal").await.unwrap().status(), 200);
}

#[tokio::test]
async fn maintenance_driver_changes_are_visible_to_application_and_keep_captured_config() {
    let mut app = TestApp::create(configure).unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<MaintenanceModeManager>().await.unwrap();
    assert!(Arc::ptr_eq(
        &app.app().maintenance_mode().unwrap(),
        &manager.driver(None).unwrap()
    ));
    let array = manager.driver(None).unwrap();
    let manual = MaintenanceModeManager::new(&manager.get_container().unwrap()).unwrap();
    assert!(!Arc::ptr_eq(&array, &manual.driver(None).unwrap()));
    app.app()
        .config()
        .unwrap()
        .set("app.maintenance.driver", json!("file"))
        .unwrap();
    manager
        .driver(None)
        .unwrap()
        .activate(&json!({"status":503}).as_object().unwrap().clone())
        .await
        .unwrap();
    assert!(app.app().is_down_for_maintenance().await.unwrap());
    assert!(!array.active().await.unwrap());
    let mut other = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"maintenance":{"driver":"array"}}),
            )]))
        })
    })
    .unwrap();
    other.boot().await.unwrap();
    let container = other
        .app()
        .make::<MaintenanceModeManager>()
        .await
        .unwrap()
        .get_container()
        .unwrap();
    manager.set_container(&container).forget_drivers();
    assert!(Arc::ptr_eq(&manager.get_container().unwrap(), &container));
    // setContainer 保留构造时配置，未来 file 驱动使用新容器中的 Paths。
    assert_eq!(manager.get_default_driver().unwrap(), "file");
    assert!(!manager.driver(None).unwrap().active().await.unwrap());
    manager
        .driver(None)
        .unwrap()
        .activate(&json!({}).as_object().unwrap().clone())
        .await
        .unwrap();
    assert!(other.root().join("storage/framework/down").is_file());
    manager.driver(None).unwrap().deactivate().await.unwrap();
    drop(container);
    drop(other);
    assert!(manager.get_container().is_err());
}

#[tokio::test]
async fn maintenance_cache_uses_repository_default_lifetime_and_follows_purge() {
    async fn cache_control() -> Result<Value, MaintenanceError> {
        Config::set("app.maintenance.driver", json!("cache")).unwrap();
        Cache::set_default_cache_time(Some(0)).unwrap();
        assert!(matches!(
            MaintenanceMode::activate(&json!({}).as_object().unwrap().clone()).await,
            Err(MaintenanceError::WriteRejected)
        ));
        Cache::set_default_cache_time(None).unwrap();
        MaintenanceMode::activate(&json!({"status":503}).as_object().unwrap().clone()).await?;
        assert!(MaintenanceMode::active().await?);
        let old = Cache::store(None).unwrap();
        Cache::purge(None).unwrap();
        assert!(!MaintenanceMode::active().await?);
        assert!(old.has("illuminate:foundation:down").await.unwrap());
        Ok(json!({"ok":true}))
    }
    fn cache_endpoint(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Response::json(StatusCode::OK, cache_control().await.unwrap()) })
    }
    let mut app = TestApp::create(|builder| {
        configure(builder).with_routing(Routing::default().web(|| {
            Route::get("/control/cache", cache_endpoint)?;
            Ok(())
        }))
    })
    .unwrap();
    assert_eq!(app.get("/control/cache").await.unwrap().status(), 200);
}

#[tokio::test]
async fn maintenance_http_resolves_explicit_contract_replacement_after_kernel_start() {
    let mut app = TestApp::create(configure).unwrap();
    let mut other = TestApp::create(configure).unwrap();
    assert_eq!(app.get("/normal").await.unwrap().status(), 200);
    other.boot().await.unwrap();
    let replacement = other.app().maintenance_mode().unwrap();
    replacement
        .activate(&json!({"status":503}).as_object().unwrap().clone())
        .await
        .unwrap();
    app.instance::<dyn openrust::contracts::foundation::MaintenanceMode>(replacement.clone())
        .await
        .unwrap();
    assert_eq!(app.get("/normal").await.unwrap().status(), 503);
    assert_eq!(app.artisan(["up"]).await.status, 0);
    assert_eq!(app.get("/normal").await.unwrap().status(), 200);
    assert!(!replacement.active().await.unwrap());
}

struct MaintenanceJob(Arc<AtomicUsize>);
impl openrust::contracts::queue::JobHandler for MaintenanceJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        payload: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            let active = MaintenanceMode::active().await.map_err(|source| {
                openrust::queue::QueueError::Execution {
                    source: Box::new(source),
                }
            })?;
            assert_eq!(active, payload.data()["active"].as_bool().unwrap());
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn artisan_worker_uses_maintenance_manager_and_force_with_real_redis_queue() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        return;
    };
    let mut app = TestApp::create(configure).unwrap();
    app.boot().await.unwrap();
    let config = app.app().config().unwrap();
    config
        .set(
            "database.redis.default",
            json!({"url":format!("redis://127.0.0.1:{port}/")}),
        )
        .unwrap();
    config.set("queue.default", json!("maintenance")).unwrap();
    config
        .set(
            "queue.connections.maintenance",
            json!({"driver":"redis","queue":"maintenance-facade","retry_after":30}),
        )
        .unwrap();
    config.set("queue.failed.driver", json!("null")).unwrap();
    let queue = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    queue
        .register("maintenance.probe", Arc::new(MaintenanceJob(count.clone())))
        .unwrap();
    let push = |active| {
        queue.dispatch(
            openrust::queue::Payload::new("maintenance.probe", json!({"active":active})).unwrap(),
            openrust::queue::DispatchOptions::default(),
            None,
        )
    };
    push(true).await.unwrap();
    assert_eq!(app.artisan(["down"]).await.status, 0);
    let blocked = app
        .artisan(["queue:work", "--once", "--sleep=0", "--timeout=0"])
        .await;
    assert_eq!(blocked.status, 0, "{}", blocked.stderr);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let forced = app
        .artisan([
            "queue:work",
            "--once",
            "--sleep=0",
            "--timeout=0",
            "--force",
        ])
        .await;
    assert_eq!(forced.status, 0, "{}", forced.stderr);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    push(false).await.unwrap();
    assert_eq!(app.artisan(["up"]).await.status, 0);
    let resumed = app
        .artisan(["queue:work", "--once", "--sleep=0", "--timeout=0"])
        .await;
    assert_eq!(resumed.status, 0, "{}", resumed.stderr);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}
