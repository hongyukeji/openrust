use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use openrust::{
    cache::{ArrayStore, CacheError, CacheManager, CacheResult, Repository, events::CacheHit},
    contracts::{cache::Repository as _, container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::ApplicationBuilder},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Cache},
    testing::TestApp,
};
use serde_json::{Value as Config, json};

fn configure(builder: ApplicationBuilder) -> ApplicationBuilder {
    builder.withConfig(|_, _| {
        Ok([(
            "cache".into(),
            json!({
                "default": "reports",
                "stores": {
                    "reports": {"driver": "project", "label": "reports"},
                    "mirror": {"driver": "project", "label": "mirror"},
                    "builtin": {"driver": "array"},
                    "left": {"driver": "left"},
                    "right": {"driver": "right"},
                    "recover": {"driver": "recover"}
                }
            }),
        )]
        .into())
    })
}

struct ProjectCache;

impl ServiceProvider for ProjectCache {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Cache::extend("project", |manager, config| {
                manager.repository(Arc::new(ArrayStore::new()), config)
            })?;
            Ok(())
        })
    }
}

async fn manager() -> (TestApp, Arc<CacheManager>) {
    let mut app = TestApp::create(configure).unwrap();
    app.boot().await.unwrap();
    let manager = app.app().container().make::<CacheManager>().unwrap();
    (app, manager)
}

#[tokio::test]
async fn custom_driver_lifetime_dispatch_and_named_configuration() {
    let (app, manager) = manager().await;
    let (_, second) = self::manager().await;
    let owner = manager.getContainer().unwrap();
    assert!(std::ptr::eq(owner.as_ref(), app.app().container()));
    let container = Arc::downgrade(&owner);
    let settings = Arc::new(std::sync::Mutex::new(Vec::new()));
    let resolved = settings.clone();

    manager
        .extend("project", move |manager, config| {
            assert!(Arc::ptr_eq(
                &manager.getContainer().unwrap(),
                &container.upgrade().unwrap()
            ));
            let _ = manager.store(Some("builtin"))?;
            resolved.lock().unwrap().push(config.clone());
            manager.repository(Arc::new(ArrayStore::new()), config)
        })
        .unwrap();
    assert!(settings.lock().unwrap().is_empty());
    assert!(matches!(
        second.store(None),
        Err(CacheError::UnsupportedDriver)
    ));

    let first = manager.driver(None).unwrap();
    let mirror = manager.store(Some("mirror")).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &manager.store(Some("reports")).unwrap()
    ));
    assert!(first.forever("status", &"ready".into()).await.unwrap());
    assert_eq!(mirror.get("status").await.unwrap(), None);
    assert_eq!(settings.lock().unwrap()[0]["store"], "reports");
    assert_eq!(settings.lock().unwrap()[1]["label"], "mirror");
    let on_demand = manager
        .build(json!({"driver":"project", "name": "scratch"}))
        .unwrap();
    assert_eq!(on_demand.get("status").await.unwrap(), None);
    assert_eq!(settings.lock().unwrap()[2]["store"], "scratch");
    assert!(matches!(
        manager.store(Some("scratch")),
        Err(CacheError::UnknownStore { .. })
    ));
    let ad_hoc = manager.resolve("reports").unwrap();
    assert_eq!(ad_hoc.get("status").await.unwrap(), None);
    assert_eq!(settings.lock().unwrap().len(), 4);

    manager
        .extend("project", |_, _| Err(CacheError::InvalidValue))
        .unwrap();
    assert_eq!(
        manager.store(None).unwrap().get("status").await.unwrap(),
        Some("ready".into())
    );
    manager.purge(None).unwrap();
    assert!(matches!(manager.store(None), Err(CacheError::InvalidValue)));
    manager
        .extend("project", |manager, config| {
            manager.repository(Arc::new(ArrayStore::new()), config)
        })
        .unwrap();
    assert_eq!(
        manager.store(None).unwrap().get("status").await.unwrap(),
        None
    );
    assert!(Arc::ptr_eq(
        &mirror,
        &manager.store(Some("mirror")).unwrap()
    ));
    manager.setDefaultDriver("mirror").unwrap();
    assert!(Arc::ptr_eq(&mirror, &manager.store(None).unwrap()));
}

#[tokio::test]
async fn custom_driver_facade_runs_before_artisan() {
    let mut app =
        TestApp::create(|builder| configure(builder).withProviders(vec![Box::new(ProjectCache)]))
            .unwrap();
    let events = app.fakeEvents(&[CacheHit::NAME]).await.unwrap();
    let store = app
        .app()
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();
    assert!(store.forever("status", &"ready".into()).await.unwrap());
    assert_eq!(store.get("status").await.unwrap(), Some("ready".into()));
    assert_eq!(
        events.dispatched(CacheHit::NAME)[0]
            .downcast_ref::<CacheHit>()
            .unwrap()
            .storeName
            .as_deref(),
        Some("reports")
    );
    let command = app.artisan(["cache:clear", "reports"]).await;
    assert_eq!(command.status, 0, "{}", command.stderr);
    assert_eq!(store.get("status").await.unwrap(), None);
}

#[tokio::test]
async fn concurrent_resolvers_share_the_completed_repository() {
    let (_app, manager) = manager().await;
    let calls = Arc::new(AtomicUsize::default());
    let counter = calls.clone();
    manager
        .extend("project", move |manager, config| {
            counter.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            manager.repository(Arc::new(ArrayStore::new()), config)
        })
        .unwrap();
    let ready = Arc::new(Barrier::new(6));
    let tasks: Vec<_> = (0..6)
        .map(|_| {
            let (manager, ready) = (manager.clone(), ready.clone());
            std::thread::spawn(move || {
                ready.wait();
                manager.store(None).unwrap()
            })
        })
        .collect();
    let stores: Vec<_> = tasks.into_iter().map(|task| task.join().unwrap()).collect();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(stores.iter().all(|store| Arc::ptr_eq(store, &stores[0])));
}

fn load_with_dependency(
    manager: &CacheManager,
    config: &Config,
    dependency: &str,
    first: &AtomicUsize,
    ready: &Barrier,
) -> CacheResult<Repository> {
    if first.fetch_add(1, Ordering::SeqCst) == 0 {
        ready.wait();
    }
    manager.store(Some(dependency))?;
    manager.repository(Arc::new(ArrayStore::new()), config)
}

#[tokio::test]
async fn dependency_cycles_and_factory_failures_allow_resolution_to_retry() {
    let (_app, manager) = manager().await;
    let ready = Arc::new(Barrier::new(2));
    for (driver, dependency) in [("left", "right"), ("right", "left")] {
        let ready = ready.clone();
        let first = AtomicUsize::default();
        manager
            .extend(driver, move |manager, config| {
                load_with_dependency(manager, config, dependency, &first, &ready)
            })
            .unwrap();
    }
    let (sender, results) = std::sync::mpsc::channel();
    for name in ["left", "right"] {
        let (manager, sender) = (manager.clone(), sender.clone());
        std::thread::spawn(move || {
            sender.send(manager.store(Some(name))).unwrap();
        });
    }
    for _ in 0..2 {
        assert!(matches!(
            results.recv_timeout(Duration::from_secs(5)).unwrap(),
            Err(CacheError::StoreResolutionCycle)
        ));
    }

    let attempts = AtomicUsize::default();
    manager
        .extend("recover", move |manager, config| {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(CacheError::InvalidValue);
            }
            manager.repository(Arc::new(ArrayStore::new()), config)
        })
        .unwrap();
    assert!(matches!(
        manager.store(Some("recover")),
        Err(CacheError::InvalidValue)
    ));
    assert_eq!(
        manager
            .store(Some("recover"))
            .unwrap()
            .get("empty")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn purging_during_creation_does_not_publish_a_stale_repository() {
    let (_app, manager) = manager().await;
    let (entered, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let (before, after) = (entered.clone(), release.clone());
    let calls = AtomicUsize::default();
    manager
        .extend("project", move |manager, config| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                before.wait();
                after.wait();
            }
            manager.repository(Arc::new(ArrayStore::new()), config)
        })
        .unwrap();
    let pending = manager.clone();
    let first = std::thread::spawn(move || pending.store(None).unwrap());
    entered.wait();
    manager.forgetDriver(None).unwrap();
    let current = manager.store(None).unwrap();
    release.wait();
    let stale = first.join().unwrap();
    assert!(!Arc::ptr_eq(&stale, &current));
    assert!(Arc::ptr_eq(&current, &manager.store(None).unwrap()));
}
