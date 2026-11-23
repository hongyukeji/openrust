#![recursion_limit = "256"]
use openrust::{
    contracts::foundation::Application as _,
    database::{
        Connection, DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response},
    support::{ProviderResult, ServiceProvider, facades::Route},
    testing::TestApp,
};
use openrust_scout::{
    Builder, EngineManager, ScoutFuture, ScoutServiceProvider, Searchable,
    engines::{CollectionEngine, Engine, SearchResults},
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
struct Article(ModelState);
impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("scout_articles")
            .fillable(&["id", "title", "status", "category", "deleted_at"])
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        &META
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
impl Searchable for Article {
    fn search_index_should_be_updated(&self) -> bool {
        self.get_attribute("title") != Some(&Value::Text("skip-index".into()))
    }
    fn should_be_searchable(&self) -> bool {
        self.get_attribute("status") == Some(&Value::Text("published".into()))
    }
    fn to_searchable_array(&self) -> openrust_scout::Result<serde_json::Map<String, Json>> {
        Ok(json!({"title":match self.get_attribute("title"){Some(Value::Text(v))=>v.clone(),_=>String::new()},"nested":{"locale":"中文"}}).as_object().unwrap().clone())
    }
}
fn config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Json>, openrust::config::ConfigError> {
    let driver = std::env::var("SCOUT_TEST_DRIVER").unwrap_or("sqlite".into());
    let db = match driver.as_str() {
        "sqlite" => {
            std::fs::write(root.join("scout.sqlite"), []).unwrap();
            json!({"driver":"sqlite","database":root.join("scout.sqlite"),"max_connections":4,"busy_timeout":10000})
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
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing","debug":true})),
        (
            "database".into(),
            json!({"default":"test","connections":{"test":db}}),
        ),
        (
            "scout".into(),
            json!({"driver":"collection","after_commit":true,"soft_delete":true,"chunk":{"searchable":2}}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":root.join("cache"),"lock_path":root.join("locks")}}}),
        ),
        (
            "queue".into(),
            json!({"default":"database","connections":{"database":{"driver":"database","table":"jobs","queue":"scout","retry_after":90}},"failed":{"driver":"null"}}),
        ),
    ]))
}
fn immediate_config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, Json>, openrust::config::ConfigError> {
    let mut config = config(root, env)?;
    config.get_mut("scout").unwrap()["after_commit"] = false.into();
    Ok(config)
}
struct Register;
impl ServiceProvider for Register {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<EngineManager>()
                .await?
                .register::<Article>(app, "Article")
                .await?;
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/search", search)?;
    Route::get("/sync", manual_sync)?;
    Ok(())
}
fn search(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let results = Article::search("RUST")
            .map_err(internal)?
            .order_by("id", "asc")
            .paginate(1, 1)
            .await
            .map_err(internal)?;
        Response::json(
            openrust::http::StatusCode::OK,
            json!({"total":results.total(),"title":results.items()[0].get_attribute("title").and_then(|v|if let Value::Text(v)=v{Some(v)}else{None})}),
        )
    })
}
fn manual_sync(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        use openrust_scout::SearchableCollection;
        let result = async {
            let manager = Article::scout_manager()?;
            let nested = Article::without_syncing_to_search(async {
                let mut model = Article::find(1_i64).await.unwrap().unwrap();
                model.set_attribute("title", "suppressed");
                model.save().await.unwrap();
                Article::without_syncing_to_search(async {
                    model.set_attribute("title", "nested");
                    model.save().await.unwrap();
                })
                .await
                .unwrap();
            })
            .await;
            nested?;
            let cancelled = tokio::time::timeout(
                std::time::Duration::from_millis(1),
                Article::without_syncing_to_search(std::future::pending::<()>()),
            )
            .await;
            assert!(cancelled.is_err());
            assert!(!manager.syncing_disabled_for::<Article>());
            let mut model = Article::find(1_i64).await?.unwrap();
            model.set_attribute("title", "restored");
            model.save().await?;
            model.searchable_sync().await?;
            let models = Article::query()?.get().await?;
            models.unsearchable_sync().await?;
            assert_eq!(model.searchable_as()?, "scout_articles");
            Ok::<_, openrust_scout::Error>(())
        }
        .await;
        result.map_err(internal)?;
        Ok(Response::from("sync accepted"))
    })
}
fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
struct Fixture {
    app: TestApp,
    db: Connection,
    manager: Arc<EngineManager>,
}
impl Fixture {
    async fn new(after: bool) -> Self {
        let mut app = TestApp::create(move |builder| {
            builder
                .with_config(if after { config } else { immediate_config })
                .with_providers(vec![Box::new(ScoutServiceProvider), Box::new(Register)])
                .with_routing(Routing::default().api(routes))
        })
        .unwrap();
        app.boot().await.unwrap();
        let db = app
            .app()
            .make::<DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        for name in ["scout_articles", "jobs"] {
            db.get_schema_builder().drop_if_exists(name).await.unwrap();
        }
        db.get_schema_builder()
            .create("scout_articles", |table| {
                table.id();
                table.string("title", 255);
                table.string("status", 32);
                table.integer("category");
                table.timestamp("deleted_at", None).nullable();
            })
            .await
            .unwrap();
        db.get_schema_builder()
            .create("jobs", |table| {
                table.id();
                table.string("queue", 255).index();
                table.longText("payload");
                table.unsigned_integer("attempts");
                table.unsigned_big_integer("reserved_at").nullable();
                table.unsigned_big_integer("available_at");
                table.unsigned_big_integer("created_at");
            })
            .await
            .unwrap();
        let manager = app.app().make::<EngineManager>().await.unwrap();
        Self { app, db, manager }
    }
    fn article(&self, id: i64, title: &str, status: &str) -> Article {
        Article::new_instance_on(
            &self.db,
            [
                ("id".into(), id.into()),
                ("title".into(), title.into()),
                ("status".into(), status.into()),
                ("category".into(), (id % 2).into()),
            ]
            .into(),
        )
        .unwrap()
    }
    fn search(&self, q: &str) -> Builder<Article> {
        Builder::new(
            Article::new_instance_on(&self.db, Default::default()).unwrap(),
            q,
            self.manager.clone(),
        )
    }
    fn recording(&self) -> Arc<Recording> {
        let engine = Arc::new(Recording::default());
        self.manager.extend::<Article>("recording", engine.clone());
        self.app
            .app()
            .config()
            .unwrap()
            .set("scout.driver", json!("recording"))
            .unwrap();
        engine
    }
}
fn number(v: &Value) -> i64 {
    match v {
        Value::Integer(v) => *v,
        Value::Unsigned(v) => i64::try_from(*v).unwrap(),
        _ => panic!("integer id: {v:?}"),
    }
}
fn ids(models: impl IntoIterator<Item = Article>) -> Vec<i64> {
    models
        .into_iter()
        .map(|m| number(&m.get_scout_key().unwrap()))
        .collect()
}
#[derive(Clone, Debug, PartialEq)]
struct Action {
    kind: &'static str,
    ids: Vec<i64>,
    trashed: Vec<bool>,
    titles: Vec<String>,
}
#[derive(Default)]
struct Recording {
    actions: Mutex<Vec<Action>>,
    fail_next: std::sync::atomic::AtomicBool,
}
impl Recording {
    fn take(&self) -> Vec<Action> {
        std::mem::take(&mut *self.actions.lock().unwrap())
    }
    fn record(&self, kind: &'static str, models: &[Article]) {
        self.actions.lock().unwrap().push(Action {
            kind,
            ids: models
                .iter()
                .map(|m| number(&m.get_scout_key().unwrap()))
                .collect(),
            trashed: models.iter().map(Model::trashed).collect(),
            titles: models
                .iter()
                .map(|m| match m.get_attribute("title") {
                    Some(Value::Text(s)) => s.clone(),
                    _ => String::new(),
                })
                .collect(),
        });
    }
}
impl Engine<Article> for Recording {
    fn update<'a>(&'a self, models: &'a [Article]) -> ScoutFuture<'a, ()> {
        Box::pin(async move {
            if self
                .fail_next
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(openrust_scout::Error::NotSupported(
                    "temporary engine failure".into(),
                ));
            }
            self.record("update", models);
            Ok(())
        })
    }
    fn delete<'a>(&'a self, models: &'a [Article]) -> ScoutFuture<'a, ()> {
        Box::pin(async move {
            self.record("delete", models);
            Ok(())
        })
    }
    fn search<'a>(&'a self, b: &'a Builder<Article>) -> ScoutFuture<'a, SearchResults> {
        CollectionEngine.search(b)
    }
    fn paginate<'a>(
        &'a self,
        b: &'a Builder<Article>,
        size: u64,
        page: u64,
    ) -> ScoutFuture<'a, SearchResults> {
        CollectionEngine.paginate(b, size, page)
    }
    fn flush<'a>(&'a self, m: &'a Article) -> ScoutFuture<'a, ()> {
        Box::pin(async move {
            let _ = m;
            self.record("flush", &[]);
            Ok(())
        })
    }
    fn create_index<'a>(&'a self, _: &'a str, _: &'a Json) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn delete_index<'a>(&'a self, _: &'a str) -> ScoutFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test]
async fn collection_search_filters_maps_callbacks_soft_deletes_pages_and_native_http() {
    use futures_util::TryStreamExt as _;
    let mut f = Fixture::new(true).await;
    for (id, title, status) in [
        (1, "Rust Guide", "published"),
        (2, "Trust in Rust", "published"),
        (3, "draft Rust", "draft"),
        (4, "Database 中文", "published"),
    ] {
        f.article(id, title, status).save().await.unwrap();
    }
    assert_eq!(ids(f.search("RUST").get().await.unwrap()), vec![2, 1]);
    assert_eq!(
        ids(f
            .search("rust")
            .r#where("category", "=", 1_i64)
            .get()
            .await
            .unwrap()),
        vec![1]
    );
    assert_eq!(
        ids(f
            .search("")
            .where_in("id", vec![1_i64.into(), 4_i64.into()])
            .where_not_in("id", vec![4_i64.into()])
            .get()
            .await
            .unwrap()),
        vec![1]
    );
    assert_eq!(ids(f.search("中文").get().await.unwrap()), vec![4]);
    assert_eq!(
        ids(f
            .search(r"\u4e2d")
            .order_by("id", "asc")
            .get()
            .await
            .unwrap()),
        vec![1, 2, 4]
    );
    assert_eq!(ids(f.search("").take(1).get().await.unwrap()), vec![4]);
    let page = f
        .search("rust")
        .order_by("id", "asc")
        .take(1)
        .paginate(1, 2)
        .await
        .unwrap();
    assert_eq!(page.total(), 2);
    assert_eq!(number(&page.items()[0].get_scout_key().unwrap()), 2);
    assert!(!page.has_more_pages());
    let page = f.search("rust").simple_paginate(1, 1).await.unwrap();
    assert!(page.has_more_pages());
    assert_eq!(f.search("absent").paginate(2, 1).await.unwrap().total(), 0);
    assert!(f.search("rust").paginate(0, 1).await.is_err());
    assert_eq!(
        ids(f
            .search("rust")
            .order_by("id", "asc")
            .cursor()
            .try_collect::<Vec<_>>()
            .await
            .unwrap()),
        vec![1, 2]
    );
    let filtered = f
        .search("rust")
        .query(|mut query| {
            query.r#where("id", "=", 1_i64);
            Ok(query)
        })
        .get()
        .await
        .unwrap();
    assert_eq!(ids(filtered), vec![1]);
    let callback = f
        .search("")
        .where_in("id", vec![1_i64.into()])
        .callback(|mut query| {
            query.r#where("id", "=", 4_i64);
            Ok(query)
        })
        .get()
        .await
        .unwrap();
    assert_eq!(ids(callback), vec![4]);
    let raw = f.search("rust").with_raw_results(|mut results| {
        results.ids.reverse();
        Ok(results)
    });
    assert_eq!(ids(raw.get().await.unwrap()), vec![1, 2]);
    assert_eq!(
        raw.keys()
            .await
            .unwrap()
            .iter()
            .map(number)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    let string_keys = f.search("rust").with_raw_results(|mut results| {
        results.ids = results
            .ids
            .into_iter()
            .map(|key| number(&key).to_string().into())
            .collect();
        Ok(results)
    });
    assert_eq!(ids(string_keys.get().await.unwrap()), vec![2, 1]);
    assert_eq!(
        ids(string_keys.cursor().try_collect::<Vec<_>>().await.unwrap()),
        vec![2, 1]
    );
    assert!(f.search("").semantic(None).is_err());
    assert!(
        f.search("rust")
            .semantic(None)
            .unwrap()
            .get()
            .await
            .is_err()
    );
    assert!(f.search("rust").hybrid(0.0, 1.0, None).is_err());
    assert!(
        f.search("rust")
            .hybrid(1.0, 1.0, None)
            .unwrap()
            .get()
            .await
            .is_err()
    );
    f.app.get("/api/search").await.unwrap().assert_ok();
    let mut article = Article::query_on(&f.db).find(1_i64).await.unwrap().unwrap();
    article.delete().await.unwrap();
    assert_eq!(ids(f.search("rust").get().await.unwrap()), vec![2]);
    assert_eq!(
        ids(f.search("rust").with_trashed().get().await.unwrap()),
        vec![2, 1]
    );
    assert_eq!(
        ids(f.search("rust").only_trashed().get().await.unwrap()),
        vec![1]
    );
    article.restore().await.unwrap();
    assert_eq!(ids(f.search("rust").get().await.unwrap()), vec![2, 1]);
    assert!(f.search("  ").get().await.unwrap().is_empty());
    for (operator, bound, expected) in [
        (">", 2_i64, vec![4]),
        ("<", 2, vec![1]),
        (">=", 2, vec![2, 4]),
        ("<=", 2, vec![1, 2]),
        ("!=", 2, vec![1, 4]),
    ] {
        assert_eq!(
            ids(f
                .search("")
                .r#where("id", operator, bound)
                .order_by("id", "asc")
                .get()
                .await
                .unwrap()),
            expected
        );
    }
    f.article(5, "Agent 0", "published").save().await.unwrap();
    f.article(6, "Agent 00", "published").save().await.unwrap();
    assert_eq!(ids(f.search("0").get().await.unwrap()), vec![6, 5]);
    let zero = f
        .search("0")
        .order_by("id", "asc")
        .paginate(1, 2)
        .await
        .unwrap();
    assert_eq!(zero.total(), 2);
    assert_eq!(number(&zero.items()[0].get_scout_key().unwrap()), 6);
    f.app
        .app()
        .config()
        .unwrap()
        .set("scout.driver", json!("null"))
        .unwrap();
    assert!(f.search("").get().await.unwrap().is_empty());
    f.app
        .app()
        .config()
        .unwrap()
        .set("scout.driver", json!("missing"))
        .unwrap();
    assert!(f.search("").get().await.is_err());
}
#[tokio::test]
async fn observer_waits_for_outer_commit_discards_rollback_and_tracks_soft_delete_lifecycle() {
    let f = Fixture::new(true).await;
    let engine = f.recording();
    let mut transaction = f.db.begin_transaction().await.unwrap();
    let mut article = f.article(1, "Rust", "published");
    article.save_in_transaction(&mut transaction).await.unwrap();
    assert!(engine.take().is_empty());
    transaction.roll_back(None).await.unwrap();
    assert!(engine.take().is_empty());
    let mut outer = f.db.begin_transaction().await.unwrap();
    let mut article = f.article(2, "Rust", "published");
    article.save_in_transaction(&mut outer).await.unwrap();
    assert!(engine.take().is_empty());
    outer.begin_transaction().await.unwrap();
    f.article(3, "rolled back inner", "published")
        .save_in_transaction(&mut outer)
        .await
        .unwrap();
    outer.roll_back(Some(1)).await.unwrap();
    outer.begin_transaction().await.unwrap();
    f.article(4, "committed inner", "published")
        .save_in_transaction(&mut outer)
        .await
        .unwrap();
    outer.commit().await.unwrap();
    assert!(engine.take().is_empty());
    outer.commit().await.unwrap();
    assert_eq!(
        engine
            .take()
            .iter()
            .flat_map(|a| a.ids.clone())
            .collect::<Vec<_>>(),
        vec![4, 2]
    );
    article.set_attribute("status", "draft");
    article.save().await.unwrap();
    assert_eq!(engine.take()[0].kind, "delete");
    article.set_attribute("status", "published");
    article.save().await.unwrap();
    engine.take();
    article.set_attribute("title", "skip-index");
    article.save().await.unwrap();
    assert!(engine.take().is_empty());
    article.delete().await.unwrap();
    let actions = engine.take();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, "update");
    assert_eq!(actions[0].trashed, vec![true]);
    article.restore().await.unwrap();
    assert!(engine.take().iter().all(|a| a.kind == "update"));
    article.force_delete().await.unwrap();
    let actions = engine.take();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, "delete");
    f.manager.disable_syncing_for::<Article>();
    f.article(3, "Rust", "published").save().await.unwrap();
    assert!(engine.take().is_empty());
    f.manager.enable_syncing_for::<Article>();
    assert!(
        f.manager
            .register::<Article>(f.app.app(), "DuplicateAlias")
            .await
            .is_err()
    );
}
#[tokio::test]
async fn native_queue_reloads_latest_models_deletes_tombstones_and_import_commands_use_chunks() {
    let mut f = Fixture::new(true).await;
    let engine = f.recording();
    f.app
        .app()
        .config()
        .unwrap()
        .set(
            "scout.queue",
            json!({"connection":"database","queue":"scout"}),
        )
        .unwrap();
    let mut transaction = f.db.begin_transaction().await.unwrap();
    let mut first = f.article(1, "Rust", "published");
    first.save_in_transaction(&mut transaction).await.unwrap();
    assert_eq!(f.db.table("jobs").count("*").await.unwrap(), 0);
    transaction.commit().await.unwrap();
    assert_eq!(f.db.table("jobs").count("*").await.unwrap(), 1);
    assert!(engine.take().is_empty());
    // Queue updates restore the current model instead of indexing stale serialized fields.
    f.db.table("scout_articles")
        .r#where("id", "=", 1_i64)
        .update([("title".into(), "Updated Rust".into())].into())
        .await
        .unwrap();
    engine
        .fail_next
        .store(true, std::sync::atomic::Ordering::SeqCst);
    f.app
        .artisan([
            "queue:work",
            "database",
            "--queue=scout",
            "--once",
            "--tries=2",
            "--backoff=0",
            "--sleep=0",
            "--memory=0",
        ])
        .await
        .assert_successful();
    assert!(engine.take().is_empty());
    assert_eq!(f.db.table("jobs").count("*").await.unwrap(), 1);
    f.db.table("scout_articles")
        .r#where("id", "=", 1_i64)
        .update([("title".into(), "Retried Rust".into())].into())
        .await
        .unwrap();
    f.app
        .artisan([
            "queue:work",
            "database",
            "--queue=scout",
            "--stop-when-empty",
            "--tries=2",
            "--sleep=0",
            "--memory=0",
        ])
        .await
        .assert_successful();
    let actions = engine.take();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].ids, vec![1]);
    assert_eq!(actions[0].titles, vec!["Retried Rust"]);
    assert_eq!(f.db.table("jobs").count("*").await.unwrap(), 0);
    assert_eq!(actions[0].kind, "update");
    first.force_delete().await.unwrap();
    assert_eq!(f.db.table("scout_articles").count("*").await.unwrap(), 0);
    f.app
        .artisan([
            "queue:work",
            "database",
            "--queue=scout",
            "--stop-when-empty",
            "--tries=2",
            "--sleep=0",
            "--memory=0",
        ])
        .await
        .assert_successful();
    assert_eq!(engine.take()[0].kind, "delete");
    f.app
        .app()
        .config()
        .unwrap()
        .set("scout.queue", json!(false))
        .unwrap();
    for id in 2..7 {
        f.article(id, "Rust", "published").save().await.unwrap();
    }
    engine.take();
    f.app
        .artisan(["scout:import", "Article", "--fresh", "-c", "2"])
        .await
        .assert_successful();
    let actions = engine.take();
    assert_eq!(
        actions
            .iter()
            .filter(|a| a.kind == "update")
            .map(|a| a.ids.len())
            .collect::<Vec<_>>(),
        vec![2, 2, 1]
    );
    assert_eq!(actions[0].kind, "flush");
    assert_eq!(f.db.table("scout_articles").count("*").await.unwrap(), 5);
    f.app
        .artisan(["scout:flush", "Article"])
        .await
        .assert_successful();
    assert_eq!(engine.take()[0].kind, "flush");
    assert_eq!(f.db.table("scout_articles").count("*").await.unwrap(), 5);
    f.app
        .artisan(["scout:import", "missing"])
        .await
        .assert_exit_code(1);
    f.app
        .artisan(["scout:import", "Article", "--chunk=0"])
        .await
        .assert_exit_code(1);
}
#[tokio::test]
async fn immediate_observer_indexes_before_transaction_completion_when_after_commit_is_disabled() {
    let f = Fixture::new(false).await;
    let engine = f.recording();
    let mut transaction = f.db.begin_transaction().await.unwrap();
    f.article(1, "Rust", "published")
        .save_in_transaction(&mut transaction)
        .await
        .unwrap();
    assert_eq!(engine.take()[0].kind, "update");
    transaction.roll_back(None).await.unwrap();
}

#[tokio::test]
async fn native_scope_restores_nested_and_cancelled_sync_suppression_and_collection_methods() {
    let mut f = Fixture::new(false).await;
    let engine = f.recording();
    f.article(1, "Rust", "published").save().await.unwrap();
    engine.take();
    f.app.get("/api/sync").await.unwrap().assert_ok();
    let actions = engine.take();
    assert_eq!(
        actions.iter().map(|a| a.kind).collect::<Vec<_>>(),
        vec!["update", "update", "delete"]
    );
    assert_eq!(actions[0].titles, vec!["restored"]);
    let other = Fixture::new(false).await;
    assert!(!other.manager.syncing_disabled_for::<Article>());
}

#[derive(Clone)]
struct SearchValue(ModelState);
impl Model for SearchValue {
    fn metadata() -> &'static ModelMetadata {
        Article::metadata()
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
impl Searchable for SearchValue {
    fn to_searchable_array(&self) -> openrust_scout::Result<serde_json::Map<String, Json>> {
        let Some(Value::Text(text)) = self.get_attribute("title") else {
            panic!("fixture JSON")
        };
        Ok(serde_json::Map::from_iter([(
            "value".into(),
            serde_json::from_str(text).unwrap(),
        )]))
    }
}
#[tokio::test]
async fn collection_matches_fixed_php_scalar_and_json_reference() {
    let fixture = Fixture::new(true).await;
    let reference: Json =
        serde_json::from_str(include_str!("fixtures/collection-php.json")).unwrap();
    for (index, value) in reference["values"].as_array().unwrap().iter().enumerate() {
        fixture
            .article(index as i64 + 1, &value.to_string(), "published")
            .save()
            .await
            .unwrap();
    }
    for case in reference["cases"].as_array().unwrap() {
        let model = SearchValue::new_instance_on(&fixture.db, Default::default()).unwrap();
        let result = Builder::new(
            model,
            case["query"].as_str().unwrap(),
            fixture.manager.clone(),
        )
        .order_by("id", "asc")
        .keys()
        .await
        .unwrap();
        let actual: Vec<i64> = result.iter().map(number).collect();
        let expected: Vec<i64> = case["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(actual, expected, "query: {}", case["query"]);
    }
}

#[tokio::test]
async fn collection_matches_original_engine_unicode_filter_mapping_and_pagination() {
    let fixture = Fixture::new(true).await;
    let reference: Json =
        serde_json::from_str(include_str!("fixtures/collection-engine.json")).unwrap();
    for record in reference["records"].as_array().unwrap() {
        let mut article = fixture.article(
            record["id"].as_i64().unwrap(),
            record["title"].as_str().unwrap(),
            record["status"].as_str().unwrap(),
        );
        article.set_attribute("category", record["category"].as_i64().unwrap());
        article.save().await.unwrap();
    }
    for case in reference["cases"].as_array().unwrap() {
        let model = Article::new_instance_on(&fixture.db, Default::default()).unwrap();
        let mut builder = Builder::new(
            model,
            case["query"].as_str().unwrap(),
            fixture.manager.clone(),
        )
        .order_by("id", "asc");
        match case["mode"].as_str().unwrap() {
            "in" => builder = builder.where_in("category", vec![2_i64.into()]),
            "not_in" => builder = builder.where_not_in("category", vec![2_i64.into()]),
            "where" => builder = builder.r#where("id", ">", 2_i64),
            "limit" => builder = builder.take(2),
            _ => {}
        }
        let engine = CollectionEngine;
        let result = if case["mode"] == "page" {
            engine.paginate(&builder, 2, 2).await.unwrap()
        } else {
            engine.search(&builder).await.unwrap()
        };
        let ids: Vec<i64> = result.ids.iter().map(number).collect();
        assert_eq!(json!(ids), case["ids"], "{case}");
        assert_eq!(json!(result.total), case["total"], "{case}");
        let mapped = engine.map(&builder, &result).await.unwrap();
        let mapped: Vec<i64> = mapped
            .iter()
            .map(|model| number(model.get_key().unwrap()))
            .collect();
        assert_eq!(json!(mapped), case["mapped"], "{case}");
    }
}
