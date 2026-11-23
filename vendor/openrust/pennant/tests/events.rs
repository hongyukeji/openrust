use openrust::testing::TestApp;
use openrust_pennant::{
    FeatureDefinition, FeatureFuture, FeatureManager, PennantServiceProvider, Scope, events::*,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn app() -> TestApp {
    TestApp::create(|builder| builder.with_config(|_, _| Ok(std::collections::BTreeMap::from([
        ("pennant".into(), json!({"default":"array", "stores":{"array":{"driver":"array"},"custom":{"driver":"custom"}}}))
    ]))).with_providers(vec![Box::new(PennantServiceProvider)])).unwrap()
}
struct Campaign {
    override_value: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl FeatureDefinition for Campaign {
    fn name() -> &'static str {
        "campaign"
    }
    fn acceptsNullScope() -> bool {
        false
    }
    fn before<'a>(&'a self, scope: &'a Scope) -> FeatureFuture<'a, Option<Value>> {
        Box::pin(async move {
            Ok(
                (self.override_value.load(Ordering::SeqCst) && scope != &Scope::Null)
                    .then(|| json!("preview")),
            )
        })
    }
    fn resolve<'a>(&'a self, _: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!("normal"))
        })
    }
}
#[tokio::test]
async fn feature_classes_override_stored_values_without_persisting_and_handle_null() {
    let mut app = app();
    let fake = app.fake_events(&[]).await.unwrap();
    let manager = app.app().make::<FeatureManager>().await.unwrap();
    let store = manager.store(None).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let factories = Arc::new(AtomicUsize::new(0));
    let override_value = Arc::new(AtomicBool::new(false));
    let definition_calls = calls.clone();
    let overrides = override_value.clone();
    let instances = factories.clone();
    store
        .defineClass(move || {
            instances.fetch_add(1, Ordering::SeqCst);
            let feature = Arc::new(Campaign {
                override_value: overrides.clone(),
                calls: definition_calls.clone(),
            });
            Box::pin(async move { Ok(feature) })
        })
        .unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    let class = std::any::type_name::<Campaign>();
    assert_eq!(store.name(class).unwrap(), "campaign");
    assert!(
        matches!(store.nameMap().unwrap()["campaign"], openrust_pennant::FeatureImplementation::Class(name) if name == class)
    );
    assert_eq!(
        store.r#for("alice").value(class).await.unwrap(),
        json!("normal")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fake.assertDispatchedTimes(FeatureResolved::NAME, 1);
    override_value.store(true, Ordering::SeqCst);
    // Cached values take precedence over the hook, exactly as the pinned Decorator.
    assert_eq!(
        store.r#for("alice").value(class).await.unwrap(),
        json!("normal")
    );
    store.flushCache().unwrap();
    store
        .getDriver()
        .set("campaign", &Scope::from("alice"), json!("normal"))
        .await
        .unwrap();
    assert_eq!(
        store.r#for("alice").value(class).await.unwrap(),
        json!("preview")
    );
    assert_eq!(
        store
            .getDriver()
            .get("campaign", &Scope::from("alice"))
            .await
            .unwrap(),
        json!("normal")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fake.assertDispatchedTimes(FeatureResolved::NAME, 1);
    // The bulk hook overrides stored values for all scopes without driver writes.
    let loaded = store
        .r#for("alice")
        .r#for("bob")
        .load(&[class])
        .await
        .unwrap();
    assert_eq!(loaded["campaign"], vec![json!("preview"), json!("preview")]);
    assert_eq!(
        store.r#for(Scope::Null).value(class).await.unwrap(),
        json!(false)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fake.assertDispatchedTimes(UnexpectedNullScopeEncountered::NAME, 1);
    fake.assertDispatchedTimes(FeatureResolved::NAME, 2);
    assert!(Arc::ptr_eq(
        &store.instance::<Campaign>().await.unwrap().calls,
        &calls
    ));
    // Redefining a class name using a literal removes its former before hook.
    store.defineValue("campaign", json!(true)).unwrap();
    store.r#for("alice").forget(&[class]).await.unwrap();
    assert_eq!(
        store.r#for("alice").value(class).await.unwrap(),
        json!(true)
    );
}
#[tokio::test]
async fn native_events_follow_cache_and_short_circuit_semantics() {
    let mut app = app();
    let fake = app.fake_events(&[]).await.unwrap();
    let manager = app.app().make::<FeatureManager>().await.unwrap();
    let store = manager.store(None).unwrap();
    store.defineValue("a", json!(false)).unwrap();
    store.defineValue("b", json!(true)).unwrap();
    let scopes = store.r#for("alice").r#for("bob");
    assert!(!scopes.allAreActive(&["a", "b"]).await.unwrap());
    fake.assertDispatchedTimes(FeatureResolved::NAME, 4);
    fake.assertDispatchedTimes(FeatureRetrieved::NAME, 1);
    assert!(scopes.someAreActive(&["b", "a"]).await.unwrap());
    fake.assertDispatchedTimes(FeatureRetrieved::NAME, 3);
    scopes.activate("a").await.unwrap();
    fake.assertDispatchedTimes(FeatureUpdated::NAME, 2);
    scopes.forget(&["a"]).await.unwrap();
    fake.assertDispatchedTimes(FeatureDeleted::NAME, 2);
    store.deactivateForEveryone("b").await.unwrap();
    fake.assertDispatchedTimes(FeatureUpdatedForAllScopes::NAME, 1);
    assert_eq!(
        store.r#for("guest").value("unknown").await.unwrap(),
        json!(false)
    );
    assert_eq!(
        store.r#for("guest").value("unknown").await.unwrap(),
        json!(false)
    );
    fake.assertDispatchedTimes(UnknownFeatureResolved::NAME, 1);
    store.purge(Some(&["a".into()])).await.unwrap();
    store.purge(None).await.unwrap();
    fake.assertDispatchedTimes(FeaturesPurged::NAME, 1);
    fake.assertDispatchedTimes(AllFeaturesPurged::NAME, 1);
    manager
        .extend("custom", |_, _| {
            Ok(Arc::new(openrust_pennant::drivers::ArrayDriver::new()))
        })
        .unwrap();
    let custom = manager.store(Some("custom")).unwrap();
    custom.defineValue("flag", json!(0)).unwrap();
    assert!(custom.r#for("alice").active("flag").await.unwrap());
}

struct Fail;
impl openrust::contracts::events::Listener for Fail {
    fn handle<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async {
            Err(openrust::events::EventError::Listener {
                source: Box::new(std::io::Error::other("listener failed")),
            })
        })
    }
}
#[tokio::test]
async fn listener_failures_keep_native_resolution_and_mutation_boundaries() {
    use openrust::contracts::events::Dispatcher as _;
    let mut app = app();
    app.boot().await.unwrap();
    let dispatcher = app
        .app()
        .make::<openrust::events::Dispatcher>()
        .await
        .unwrap();
    let store = app
        .app()
        .make::<FeatureManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    store.defineValue("flag", json!(true)).unwrap();
    dispatcher
        .listen(
            FeatureResolved::NAME,
            Arc::new(Fail),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    assert!(matches!(
        store.r#for("alice").value("flag").await,
        Err(openrust_pennant::Error::Event(_))
    ));
    assert!(store.stored().await.unwrap().is_empty());
    dispatcher.forget(FeatureResolved::NAME);
    dispatcher
        .listen(
            FeatureUpdated::NAME,
            Arc::new(Fail),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    assert!(matches!(
        store.r#for("alice").activate("flag").await,
        Err(openrust_pennant::Error::Event(_))
    ));
    assert_eq!(
        store.r#for("alice").value("flag").await.unwrap(),
        json!(true)
    );
}
struct Sequence(Arc<std::sync::Mutex<Vec<String>>>);
impl openrust::contracts::events::Listener for Sequence {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            let feature = event.downcast_ref::<FeatureResolved>().unwrap();
            self.0.lock().unwrap().push(feature.feature.clone());
            Ok(None)
        })
    }
}
#[tokio::test]
async fn definitions_bulk_values_and_events_preserve_requested_order() {
    use openrust::contracts::events::Dispatcher as _;
    let mut app = app();
    app.boot().await.unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    app.app()
        .make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            FeatureResolved::NAME,
            Arc::new(Sequence(seen.clone())),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    let store = app
        .app()
        .make::<FeatureManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    store.defineValue("z", json!(true)).unwrap();
    store.defineValue("a", json!(false)).unwrap();
    assert_eq!(store.defined().unwrap(), ["z", "a"]);
    let values = store.r#for("alice").values(&["z", "a"]).await.unwrap();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(*seen.lock().unwrap(), ["z", "a"]);
    store
        .defineLottery("always", openrust::support::Lottery::odds(1, 1).unwrap())
        .unwrap();
    store
        .defineLottery("never", openrust::support::Lottery::odds(0, 1).unwrap())
        .unwrap();
    assert!(store.r#for("alice").active("always").await.unwrap());
    assert!(store.r#for("alice").inactive("never").await.unwrap());
    assert!(
        matches!(store.implementation("z").unwrap(), openrust_pennant::FeatureImplementation::Value(value) if value == json!(true))
    );
    assert!(matches!(
        store.implementation("never").unwrap(),
        openrust_pennant::FeatureImplementation::Lottery(_)
    ));
}

#[tokio::test]
async fn all_filters_incompatible_scope_definitions_without_resolving_them() {
    let mut app = app();
    let fake = app.fake_events(&[]).await.unwrap();
    let store = app
        .app()
        .make::<FeatureManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    store.defineValue("public", json!(true)).unwrap();
    store
        .defineWithScope(
            "private",
            Arc::new(|_| Box::pin(async { Ok(json!(true)) })),
            false,
        )
        .unwrap();
    assert_eq!(
        store
            .r#for(Scope::Null)
            .all()
            .await
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["public"]
    );
    fake.assertNotDispatched(UnexpectedNullScopeEncountered::NAME);
    assert_eq!(store.r#for("alice").all().await.unwrap().len(), 2);
    assert!(!store.r#for(Scope::Null).active("private").await.unwrap());
    fake.assertDispatchedTimes(UnexpectedNullScopeEncountered::NAME, 1);
    assert!(
        store
            .r#for(Scope::Null)
            .load_missing(&["private"])
            .await
            .unwrap()
            .is_empty()
    );
}

struct CoreDriver(openrust_pennant::drivers::ArrayDriver);
impl openrust_pennant::Driver for CoreDriver {
    fn define(
        &self,
        name: &str,
        resolver: openrust_pennant::Resolver,
    ) -> openrust_pennant::Result<()> {
        self.0.define(name, resolver)
    }
    fn defined(&self) -> openrust_pennant::Result<Vec<String>> {
        self.0.defined()
    }
    fn get<'a>(&'a self, name: &'a str, scope: &'a Scope) -> FeatureFuture<'a, Value> {
        self.0.get(name, scope)
    }
    fn getAll<'a>(
        &'a self,
        features: &'a openrust_pennant::FeatureScopes,
    ) -> FeatureFuture<'a, openrust_pennant::FeatureValues> {
        self.0.getAll(features)
    }
    fn set<'a>(&'a self, name: &'a str, scope: &'a Scope, value: Value) -> FeatureFuture<'a, ()> {
        self.0.set(name, scope, value)
    }
    fn setForAllScopes<'a>(&'a self, name: &'a str, value: Value) -> FeatureFuture<'a, ()> {
        self.0.setForAllScopes(name, value)
    }
    fn delete<'a>(&'a self, name: &'a str, scope: &'a Scope) -> FeatureFuture<'a, ()> {
        self.0.delete(name, scope)
    }
    fn purge<'a>(&'a self, features: Option<&'a [String]>) -> FeatureFuture<'a, ()> {
        self.0.purge(features)
    }
}
#[tokio::test]
async fn optional_driver_capabilities_preserve_per_item_event_failure_boundaries() {
    use openrust::contracts::events::Dispatcher as _;
    let mut app = app();
    app.boot().await.unwrap();
    let manager = app.app().make::<FeatureManager>().await.unwrap();
    manager
        .extend("custom", |_, _| {
            Ok(Arc::new(CoreDriver(
                openrust_pennant::drivers::ArrayDriver::new(),
            )))
        })
        .unwrap();
    let store = manager.store(Some("custom")).unwrap();
    store.defineValue("first", json!(false)).unwrap();
    store.defineValue("second", json!(false)).unwrap();
    assert!(matches!(
        store.stored().await,
        Err(openrust_pennant::Error::Unsupported(_))
    ));
    app.app()
        .make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            FeatureUpdated::NAME,
            Arc::new(Fail),
            openrust::events::Delivery::Sync,
        )
        .unwrap();
    assert!(
        store
            .r#for("alice")
            .activateWithValue(&["first", "second"], json!(true))
            .await
            .is_err()
    );
    assert_eq!(
        store.r#for("alice").value("first").await.unwrap(),
        json!(true)
    );
    assert_eq!(
        store.r#for("alice").value("second").await.unwrap(),
        json!(false)
    );
}
