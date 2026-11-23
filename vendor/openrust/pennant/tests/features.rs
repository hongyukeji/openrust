use openrust::contracts::foundation::Application as _;
use openrust::database::{
    Connection, DatabaseManager,
    migrations::{MigrationRegistry, Migrator, RunOptions},
};
use openrust_pennant::{
    Driver, FeatureManager, PennantServiceProvider, Scope,
    drivers::{ArrayDriver, Decorator},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn fixture_config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    let driver = std::env::var("PENNANT_TEST_DRIVER").unwrap_or("sqlite".into());
    let db = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":root.join("test.sqlite"),"busy_timeout":10000,"max_connections":4})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":4})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":4})
        }
        _ => panic!("unsupported isolated driver"),
    };
    Ok(BTreeMap::from([(
        "database".into(),
        json!({"default":"testing","connections":{"testing":db}}),
    )]))
}

struct Fixture {
    app: openrust::foundation::Application,
    connection: Connection,
    migrator: Migrator,
    _root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("test.sqlite"), []).unwrap();
        let mut app = openrust::foundation::Application::configure(root.path().into())
            .with_config(fixture_config)
            .with_providers(vec![Box::new(PennantServiceProvider)])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let db = app.make::<DatabaseManager>().await.unwrap();
        let connection = db.connection(None).unwrap();
        let migrator = Migrator::new(
            db,
            Arc::new(MigrationRegistry(PennantServiceProvider::migrations())),
            "pennant_test_migrations".into(),
            root.path().into(),
        );
        migrator.install(None).await.unwrap();
        assert_eq!(
            migrator
                .run(None, &[], RunOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
        Self {
            app,
            connection,
            migrator,
            _root: root,
        }
    }
    async fn close(self) {
        assert_eq!(
            self.migrator.reset(None, &[], false).await.unwrap().len(),
            1
        );
        assert!(
            !self
                .connection
                .get_schema_builder()
                .has_table("features")
                .await
                .unwrap()
        );
        self.connection
            .get_schema_builder()
            .drop_if_exists("pennant_test_migrations")
            .await
            .unwrap();
        self.connection.disconnect();
    }
}

async fn behavior(store: Arc<Decorator>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    store
        .define(
            "counter",
            Arc::new(move |_| {
                let value = counter.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { Ok(json!(value)) })
            }),
        )
        .unwrap();
    let user = store.r#for("user-1");
    assert_eq!(user.value("counter").await.unwrap(), 0);
    assert!(
        user.active("counter").await.unwrap(),
        "zero is active in Pennant"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    user.forget(&["counter"]).await.unwrap();
    assert_eq!(user.value("counter").await.unwrap(), 1);
    assert_eq!(store.r#for("user-2").value("counter").await.unwrap(), 2);
    for value in [
        Value::Null,
        json!(0),
        json!(""),
        json!([]),
        json!({}),
        json!(true),
    ] {
        user.activateWithValue(&["rich"], value.clone())
            .await
            .unwrap();
        assert!(user.active("rich").await.unwrap(), "{value}");
        assert_eq!(user.value("rich").await.unwrap(), value);
    }
    user.deactivate("rich").await.unwrap();
    assert!(user.inactive("rich").await.unwrap());
    assert!(user.inactive("unknown").await.unwrap());
    assert!(!store.stored().await.unwrap().contains(&"unknown".into()));
    store.defineValue("unknown", json!(true)).unwrap();
    // Unknown false is request cached; registering a resolver does not invalidate it.
    assert!(user.inactive("unknown").await.unwrap());
    store.flushCache().unwrap();
    assert!(user.active("unknown").await.unwrap());
    let both = store.r#for("user-1").r#for("user-2");
    assert!(both.value("counter").await.is_err());
    both.activate("multi").await.unwrap();
    assert!(both.allAreActive(&["multi"]).await.unwrap());
    store.r#for("user-2").deactivate("multi").await.unwrap();
    assert!(!both.active("multi").await.unwrap());
    assert!(!both.inactive("multi").await.unwrap());
    both.activate("other").await.unwrap();
    assert!(both.someAreActive(&["multi", "other"]).await.unwrap());
    assert!(!both.someAreInactive(&["multi", "other"]).await.unwrap());
    assert!(both.allAreActive(&[]).await.unwrap());
    assert!(!both.someAreActive(&[]).await.unwrap());
    store
        .activateForEveryone("multi", json!("enabled"))
        .await
        .unwrap();
    assert_eq!(
        store.r#for("user-2").value("multi").await.unwrap(),
        "enabled"
    );
    assert!(
        store.r#for("new-user").inactive("multi").await.unwrap(),
        "for everyone only updates stored scopes"
    );
    store.globally().activate("global").await.unwrap();
    assert!(store.r#for(Scope::Null).inactive("global").await.unwrap());
    assert!(store.globally().active("global").await.unwrap());
    store.purge(Some(&["multi".into()])).await.unwrap();
    assert!(!store.stored().await.unwrap().contains(&"multi".into()));
    store.purge(None).await.unwrap();
    assert!(store.stored().await.unwrap().is_empty());
}

#[tokio::test]
async fn array_driver_and_scoped_interactions_preserve_pennant_semantics() {
    behavior(Arc::new(Decorator::new(
        "array",
        Arc::new(ArrayDriver::new()),
    )))
    .await;
}

#[tokio::test]
async fn database_driver_bulk_races_persistence_and_native_migration() {
    let fixture = Fixture::new().await;
    let manager = fixture.app.make::<FeatureManager>().await.unwrap();
    let store = manager.store(None).unwrap();
    fixture
        .app
        .config()
        .unwrap()
        .set(
            "pennant.stores.alternate",
            json!({"driver":"database","table":null}),
        )
        .unwrap();
    manager
        .store(Some("alternate"))
        .unwrap()
        .r#for("alternate")
        .activate("fallback-table")
        .await
        .unwrap();
    assert_eq!(
        fixture
            .connection
            .table("features")
            .r#where("name", "=", "fallback-table")
            .count("*")
            .await
            .unwrap(),
        1
    );
    behavior(store.clone()).await;
    store.defineValue("race", json!("winner")).unwrap();
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let driver = store.getDriver();
        tasks.push(tokio::spawn(async move {
            driver.get("race", &Scope::from("same")).await.unwrap()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), "winner");
    }
    assert_eq!(
        fixture
            .connection
            .table("features")
            .r#where("name", "=", "race")
            .count("*")
            .await
            .unwrap(),
        1
    );
    let features = openrust_pennant::FeatureScopes::from([
        ("race".into(), vec![Scope::from("same"), Scope::from("new")]),
        ("missing".into(), vec![Scope::Null]),
    ]);
    let values = store.getAll(&features).await.unwrap();
    assert_eq!(values["race"], vec![json!("winner"), json!("winner")]);
    assert_eq!(values["missing"], vec![json!(false)]);
    manager.forget_driver(None).unwrap();
    assert_eq!(
        manager
            .store(None)
            .unwrap()
            .r#for("same")
            .value("race")
            .await
            .unwrap(),
        "winner"
    );
    fixture.close().await;
}

struct Preview;
impl openrust_pennant::FeatureDefinition for Preview {
    fn name() -> &'static str {
        "preview"
    }
    fn before<'a>(
        &'a self,
        scope: &'a Scope,
    ) -> openrust_pennant::FeatureFuture<'a, Option<Value>> {
        Box::pin(async move { Ok((scope == &Scope::from("alice")).then(|| json!("temporary"))) })
    }
    fn resolve<'a>(&'a self, _: &'a Scope) -> openrust_pennant::FeatureFuture<'a, Value> {
        Box::pin(async { Ok(json!("resolved")) })
    }
}
#[tokio::test]
async fn database_class_hooks_override_rows_without_persisting_bulk_results() {
    let fixture = Fixture::new().await;
    let store = fixture
        .app
        .make::<FeatureManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    store
        .defineClass(|| Box::pin(async { Ok(Arc::new(Preview)) }))
        .unwrap();
    store
        .getDriver()
        .set("preview", &Scope::from("alice"), json!("stored"))
        .await
        .unwrap();
    let values = store
        .r#for("alice")
        .r#for("bob")
        .load(&[std::any::type_name::<Preview>()])
        .await
        .unwrap();
    assert_eq!(
        values["preview"],
        vec![json!("temporary"), json!("resolved")]
    );
    assert_eq!(
        store
            .getDriver()
            .get("preview", &Scope::from("alice"))
            .await
            .unwrap(),
        json!("stored")
    );
    assert_eq!(
        store
            .getDriver()
            .get("preview", &Scope::from("bob"))
            .await
            .unwrap(),
        json!("resolved")
    );
    store.flushCache().unwrap();
    assert_eq!(
        store.r#for("alice").value("preview").await.unwrap(),
        json!("temporary")
    );
    fixture.close().await;
}

#[test]
fn serialization_distinguishes_global_null_models_and_custom_values() {
    assert_eq!(Scope::Null.serialize(false), "__laravel_null");
    assert_eq!(Scope::Global.serialize(false), "__laravel_global");
    assert_eq!(Scope::from(123_i64).serialize(false), "123");
    assert_eq!(Scope::from("123").serialize(false), "123");
    let model = Scope::Model {
        class: "app::models::User".into(),
        key: "42".into(),
        morph: Some("user".into()),
    };
    assert_eq!(model.serialize(false), "app::models::User|42");
    assert_eq!(model.serialize(true), "user|42");
}

#[tokio::test]
async fn multi_scope_aggregation_matches_unmodified_pennant_php() {
    let reference: Value = serde_json::from_str(include_str!("fixtures/aggregation.json")).unwrap();
    assert_eq!(
        reference["commit"],
        "9046466feb0146b6223db8b8e9c589aeba23f7f0"
    );
    assert_eq!(reference["cases"].as_array().unwrap().len(), 2401);
    for case in reference["cases"].as_array().unwrap() {
        let driver = Arc::new(ArrayDriver::new());
        for (index, (feature, scope)) in [
            ("one", "left"),
            ("two", "left"),
            ("one", "right"),
            ("two", "right"),
        ]
        .into_iter()
        .enumerate()
        {
            driver
                .set(feature, &Scope::from(scope), case["values"][index].clone())
                .await
                .unwrap();
        }
        let store = Arc::new(Decorator::new("array", driver));
        let pending = store.r#for("left").r#for("right");
        let actual = json!([
            pending.allAreActive(&["one", "two"]).await.unwrap(),
            pending.someAreActive(&["one", "two"]).await.unwrap(),
            pending.allAreInactive(&["one", "two"]).await.unwrap(),
            pending.someAreInactive(&["one", "two"]).await.unwrap(),
        ]);
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn scalar_scope_serialization_matches_unmodified_pennant_php() {
    let reference: Value = serde_json::from_str(include_str!("fixtures/scopes.json")).unwrap();
    assert_eq!(
        reference["commit"],
        "9046466feb0146b6223db8b8e9c589aeba23f7f0"
    );
    assert_eq!(reference["global"], Scope::Global.serialize(false));
    for case in reference["cases"].as_array().unwrap() {
        let scope = match &case["scope"] {
            Value::Null => Scope::Null,
            Value::String(value) => Scope::from(value.clone()),
            Value::Number(value) => Scope::Number(value.clone()),
            other => panic!("invalid fixture scope: {other}"),
        };
        assert_eq!(
            scope.serialize(false),
            case["serialized"].as_str().unwrap(),
            "{case}"
        );
    }
}
