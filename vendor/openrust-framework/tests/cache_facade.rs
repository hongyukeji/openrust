use openrust::{
    cache::{ArrayStore, CacheError, CacheMacro, CacheResult, Ttl, Value},
    contracts::{
        cache::{Repository as _, Store},
        container::Container as _,
        events::{Event, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, Request, StatusCode},
    support::facades::{App, Cache, Route},
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

async fn manager() -> CacheResult<Json> {
    assert_eq!(Cache::get_default_driver()?, "first");
    let first = Cache::store(None)?;
    Cache::set_default_driver("second")?;
    assert_eq!(Cache::get_default_driver()?, "second");
    assert_eq!(Cache::get_name()?.as_deref(), Some("second"));
    Cache::forever("selected", &"second".into()).await?;
    assert_eq!(first.get("selected").await?, None);
    assert!(matches!(
        Cache::resolve("missing"),
        Err(CacheError::UnknownStore { .. })
    ));
    assert!(Cache::set_default_driver("").is_err());
    assert_eq!(Cache::get_default_driver()?, "second");
    let same = Cache::driver(None)?;
    assert!(Arc::ptr_eq(&same, &Cache::store(Some("second"))?));
    Cache::forget_drivers(&["first", "second"])?;
    assert!(!Arc::ptr_eq(&same, &Cache::store(Some("second"))?));
    assert_eq!(same.get("selected").await?, Some("second".into()));
    assert_eq!(Cache::get("selected").await?, None);
    let fresh = Cache::resolve("second")?;
    fresh.forever("fresh", &"fresh".into()).await?;
    assert_eq!(Cache::get("fresh").await?, None);
    let manual = Cache::repository(
        Arc::new(ArrayStore::new()),
        &json!({"store":"custom","events":false}),
    )?;
    assert_eq!(manual.get_name(), Some("custom"));
    assert!(manual.get_event_dispatcher().is_none());
    Cache::purge(None)?;
    Cache::set_default_driver("first")?;
    Ok(json!({"manager":"ok"}))
}

async fn setters() -> CacheResult<Json> {
    let shared = Cache::store(None)?;
    let cloned = shared.as_ref().clone();
    let original = Cache::get_store()?;
    Cache::forever("old", &"old".into()).await?;
    let configured = Cache::set_default_cache_time(Some(0))?;
    assert!(Arc::ptr_eq(&shared, &configured));
    assert_eq!(shared.get_default_cache_time(), Some(0));
    assert_eq!(cloned.get_default_cache_time(), Some(3600));
    Cache::put_with_default("old", &"new".into()).await?;
    assert!(Cache::missing("old").await?);
    Cache::set_default_cache_time(None)?;
    Cache::put_with_default("old", &"retained".into()).await?;
    let next: Arc<dyn Store> = Arc::new(ArrayStore::new());
    let changed = Cache::set_store(next.clone())?;
    assert!(Arc::ptr_eq(&changed, &shared));
    assert!(Arc::ptr_eq(&Cache::get_store()?, &next));
    assert_eq!(Cache::get("old").await?, None);
    assert_eq!(original.get("old").await?, Some("retained".into()));
    assert_eq!(cloned.get("old").await?, Some("retained".into()));
    Cache::forever("new", &"next".into()).await?;
    assert_eq!(shared.get("new").await?, Some("next".into()));
    let mut lock =
        Cache::lock_with_owner("owner", Some(Duration::from_secs(5)), Some("facade-owner"))?;
    assert_eq!(lock.owner(), "facade-owner");
    assert!(lock.get().await?);
    assert!(
        Cache::restore_lock("owner", "facade-owner")?
            .release()
            .await?
    );
    Ok(json!({"setters":"ok"}))
}

async fn typed() -> CacheResult<Json> {
    Cache::forever("numeric", &" +12 ".into()).await?;
    Cache::forever("false", &json!(false).into()).await?;
    Cache::forever("zero", &0_i64.into()).await?;
    Cache::forever("null", &json!(null).into()).await?;
    assert_eq!(Cache::string("numeric").await?, " +12 ");
    assert_eq!(Cache::integer("numeric").await?, 12);
    assert!(!Cache::boolean("false").await?);
    assert_eq!(Cache::float_or("missing-float", 1.25).await?, 1.25);
    assert_eq!(Cache::integer_or("missing", 7).await?, 7);
    assert_eq!(Cache::string_or("null", "fallback").await?, "fallback");
    assert_eq!(Cache::array_or("array", json!([1])).await?, json!([1]));
    assert!(matches!(
        Cache::string_or("zero", "fallback").await,
        Err(CacheError::InvalidType { .. })
    ));
    assert!(matches!(
        Cache::integer("missing").await,
        Err(CacheError::InvalidType { .. })
    ));
    assert_eq!(
        Cache::integer_or_else("numeric", || async {
            panic!("hit must not evaluate default")
        })
        .await?,
        12
    );
    assert_eq!(
        Cache::get_or("null", "fallback".into()).await?,
        "fallback".into()
    );
    assert_eq!(
        Cache::get_or("false", "fallback".into()).await?,
        json!(false).into()
    );
    assert_eq!(Cache::float_or("numeric", f64::NAN).await?, 12.0);
    assert!(matches!(
        Cache::float_or("bad-default", f64::NAN).await,
        Err(CacheError::InvalidType { .. })
    ));
    let values = Cache::get_multiple(&["numeric", "null", "absent"]).await?;
    assert_eq!(values["null"], None);
    let values = Cache::get_multiple_or(&["numeric", "null", "absent"], "fallback".into()).await?;
    assert_eq!(values["absent"], "fallback".into());
    Ok(json!({"typed":"ok"}))
}

async fn macros() -> CacheResult<Json> {
    let other = Cache::store(Some("second"))?;
    Cache::r#macro("name", |repository, _| {
        Box::pin(async move {
            repository.r#macro("nested", |_, _| Box::pin(async { Ok("nested".into()) }));
            Ok(repository.get_name().unwrap().into())
        })
    })?;
    Cache::mixin(
        [(
            "name",
            CacheMacro::new(|_, _| Box::pin(async { Ok("replaced".into()) })),
        )],
        false,
    )?;
    assert_eq!(Cache::macro_call("name", vec![]).await?, "first".into());
    assert_eq!(other.macro_call("name", vec![]).await?, "second".into());
    assert_eq!(
        Cache::resolve("second")?
            .macro_call("nested", vec![])
            .await?,
        "nested".into()
    );
    assert!(matches!(
        Cache::macro_call("absent", vec![]).await,
        Err(CacheError::UnknownMacro { .. })
    ));
    Cache::mixin(
        [(
            "name",
            CacheMacro::new(|_, _| Box::pin(async { Ok("replaced".into()) })),
        )],
        true,
    )?;
    assert_eq!(Cache::macro_call("name", vec![]).await?, "replaced".into());
    Cache::flush_macros()?;
    assert!(!other.has_macro("name"));
    Cache::r#macro("isolated", |_, _| {
        Box::pin(async { Ok("only this app".into()) })
    })?;
    Ok(json!({"macros":"ok"}))
}

async fn backend() -> CacheResult<Json> {
    let prefix = Cache::get_prefix()?;
    let driver = std::env::var("CACHE_FACADE_DRIVER").unwrap_or_else(|_| "file".into());
    if matches!(driver.as_str(), "database" | "redis") {
        assert!(prefix.starts_with("facade:"));
    } else {
        assert!(prefix.is_empty());
    }
    assert!(
        Cache::put(
            "payload",
            &Value::Bytes(vec![0, 255, 42]),
            Some(Ttl::Seconds(30))
        )
        .await?
    );
    assert_eq!(
        Cache::get("payload").await?,
        Some(Value::Bytes(vec![0, 255, 42]))
    );
    Cache::put(
        "past",
        &"past".into(),
        Some(Ttl::Until(SystemTime::UNIX_EPOCH)),
    )
    .await?;
    assert_eq!(Cache::get("past").await?, None);
    let repository = Cache::store(None)?;
    Cache::set_default_cache_time(Some(17))?;
    let memo = Cache::memo(None)?;
    assert_eq!(memo.get_default_cache_time(), Some(3600));
    assert_eq!(memo.get_name(), None);
    Cache::forever("memo", &"one".into()).await?;
    assert_eq!(memo.get("memo").await?, Some("one".into()));
    Cache::forever("memo", &"two".into()).await?;
    assert_eq!(memo.get("memo").await?, Some("one".into()));
    repository.set_store(Arc::new(ArrayStore::new()));
    assert_eq!(memo.get_store().get_prefix(), "");
    assert!(memo.get_store().lock_provider().is_some());
    memo.put("different", &"new backend".into(), None).await?;
    assert_eq!(Cache::get("different").await?, Some("new backend".into()));
    Ok(json!({"backend":"ok"}))
}

struct Count(Arc<AtomicUsize>);
impl Listener for Count {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }
}
fn events() -> (Arc<Dispatcher>, Arc<AtomicUsize>) {
    let dispatcher = Arc::new(Dispatcher::new());
    let count = Arc::new(AtomicUsize::new(0));
    dispatcher
        .register(|registry| {
            registry.listen(
                openrust::cache::events::CacheHit::NAME,
                Arc::new(Count(count.clone())),
                Delivery::Sync,
            )
        })
        .unwrap();
    (dispatcher, count)
}
async fn dispatchers() -> CacheResult<Json> {
    let repo = Cache::store(None)?;
    let (first, first_count) = events();
    let (second, second_count) = events();
    Cache::set_event_dispatcher(first.clone())?;
    Cache::forever("events", &"stored".into()).await?;
    Cache::get("events").await?;
    assert_eq!(first_count.load(Ordering::SeqCst), 1);
    let owner = App::get_container()?;
    owner.instance::<dyn openrust::contracts::events::Dispatcher>(second.clone())?;
    Cache::get("events").await?;
    assert_eq!(first_count.load(Ordering::SeqCst), 2);
    assert_eq!(second_count.load(Ordering::SeqCst), 0);
    Cache::refresh_event_dispatcher()?;
    assert!(Arc::ptr_eq(
        &repo.get_event_dispatcher().unwrap(),
        &(second.clone() as Arc<dyn openrust::contracts::events::Dispatcher>)
    ));
    Cache::get("events").await?;
    assert_eq!(second_count.load(Ordering::SeqCst), 1);
    Ok(json!({"dispatchers":"ok"}))
}

struct NextApplication(Arc<Application>);

async fn application_change() -> CacheResult<Json> {
    let next = App::make::<NextApplication>().await?;
    let previous = Cache::store(None)?;
    Cache::forever("previous", &"from A".into()).await?;
    Cache::r#macro("domain", |_, _| Box::pin(async { Ok("A".into()) }))?;
    let target = next.0.clone();
    Cache::extend("switch-during-build", move |manager, config| {
        Cache::set_application(&target)?;
        // 构造开始后应用被切换，本次构造仍使用起始应用的配置和宏。
        manager.repository(Arc::new(ArrayStore::new()), config)
    })?;
    let built =
        Cache::build(json!({"driver":"switch-during-build","store":"inflight","events":false}))?;
    assert_eq!(built.macro_call("domain", vec![]).await?, "A".into());
    assert_eq!(Cache::get_default_driver()?, "second");
    assert!(Arc::ptr_eq(&previous, &Cache::store(Some("first"))?));
    assert_eq!(previous.get("previous").await?, Some("from A".into()));
    let fresh = Cache::resolve("first")?;
    assert_eq!(fresh.get("previous").await?, None);
    assert!(!fresh.has_macro("domain"));
    assert_eq!(Cache::get_name()?.as_deref(), Some("second"));
    Ok(json!({"application":"ok"}))
}

fn http_error(error: CacheError) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/application", || async {
        application_change().await.map_err(http_error)
    })?;
    Route::get("/manager", || async { manager().await.map_err(http_error) })?;
    Route::get("/setters", || async { setters().await.map_err(http_error) })?;
    Route::get("/typed", || async { typed().await.map_err(http_error) })?;
    Route::get("/macros", || async { macros().await.map_err(http_error) })?;
    Route::get("/backend", || async { backend().await.map_err(http_error) })?;
    Route::get("/dispatchers", || async {
        dispatchers().await.map_err(http_error)
    })?;
    Route::get("/macro-isolated", || async {
        Ok::<_, HttpError>(json!(Cache::has_macro("isolated").map_err(http_error)?))
    })?;
    Ok(())
}

async fn kernel(label: &str) -> (tempfile::TempDir, Kernel) {
    let root = tempfile::tempdir().unwrap();
    let tag = root
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let driver = std::env::var("CACHE_FACADE_DRIVER").unwrap_or_else(|_| "file".into());
    std::fs::File::create(root.path().join("fixture.sqlite")).unwrap();
    let mut connection = json!({"driver":"sqlite","database":root.path().join("fixture.sqlite")});
    if let Ok(url) = std::env::var("CACHE_FACADE_DATABASE_URL") {
        connection = json!({"url":url,"use_upsert_alias":true});
    }
    let first = json!({"driver":driver,"prefix":format!("facade:{tag}:"),"path":"storage/framework/cache/data","lock_path":"storage/framework/cache/locks","table":format!("facade_cache_{tag}"),"lock_table":format!("facade_locks_{tag}")});
    let mut database = json!({"default":"fixture","connections":{"fixture":connection}});
    if driver == "redis" {
        let url = std::env::var("CACHE_FACADE_REDIS_URL").expect("Redis验收必须提供专属连接");
        database["redis"] = json!({"default":{"url":url}});
    }
    let config = BTreeMap::from([
        (
            "app".to_owned(),
            json!({"name":label,"env":"testing","locale":"en","supported_locales":[]}),
        ),
        (
            "cache".to_owned(),
            json!({"default":"first","stores":{"first":first,"second":{"driver":"array"}}}),
        ),
        ("database".to_owned(), database),
    ]);
    std::fs::write(
        root.path().join("fixture.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let app = Application::configure(root.path().to_path_buf())
        .with_config(|root, _| {
            Ok(serde_json::from_slice::<BTreeMap<String, Json>>(
                &std::fs::read(root.join("fixture.json")).unwrap(),
            )
            .unwrap())
        })
        .with_routing(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    if driver == "database" {
        let app = kernel.get_application();
        let database = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let mut schema = database.get_schema_builder();
        schema
            .create(&format!("facade_cache_{tag}"), |table| {
                table.string("key", None).primary();
                table.medium_text("value");
                table.big_integer("expiration");
            })
            .await
            .unwrap();
        schema
            .create(&format!("facade_locks_{tag}"), |table| {
                table.string("key", None).primary();
                table.string("owner", None);
                table.big_integer("expiration");
            })
            .await
            .unwrap();
    }
    (root, kernel)
}

async fn call(kernel: &Kernel, path: &str) -> Json {
    let mut request = Request::from(
        http::Request::builder()
            .uri(format!("/api/{path}"))
            .header("host", "example.test")
            .body(Body::empty())
            .unwrap(),
    );
    let response = kernel.handle(&mut request).await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_parts().1, 65536)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn facade_manager_selection_and_detached_repositories_are_real() {
    let (_root, k) = kernel("manager").await;
    assert_eq!(call(&k, "manager").await, json!({"manager":"ok"}));
}
#[tokio::test]
async fn facade_setters_update_existing_arcs_and_clones_keep_their_settings() {
    let (_root, k) = kernel("setters").await;
    assert_eq!(call(&k, "setters").await, json!({"setters":"ok"}));
}
#[tokio::test]
async fn facade_typed_reads_preserve_cache_conversions_and_null_fallbacks() {
    let (_root, k) = kernel("typed").await;
    assert_eq!(call(&k, "typed").await, json!({"typed":"ok"}));
}
#[tokio::test]
async fn facade_macros_bind_the_selected_repository_and_are_isolated() {
    let (_root, k) = kernel("macros").await;
    call(&k, "macros").await;
    assert_eq!(call(&k, "macro-isolated").await, json!(true));
    let (_other_root, other) = kernel("other").await;
    assert_eq!(call(&other, "macro-isolated").await, json!(false));
}
#[tokio::test]
async fn facade_reads_writes_locks_and_memo_follow_backend_changes() {
    let (_root, k) = kernel("backend").await;
    assert_eq!(call(&k, "backend").await, json!({"backend":"ok"}));
}
#[tokio::test]
async fn facade_dispatcher_refresh_preserves_old_selection_until_requested() {
    let (_root, k) = kernel("events").await;
    assert_eq!(call(&k, "dispatchers").await, json!({"dispatchers":"ok"}));
}

#[tokio::test]
async fn facade_application_replacement_preserves_cached_stores_and_pins_inflight_builds() {
    let (_first_root, first) = kernel("A").await;
    let (_second_root, second) = kernel("B").await;
    second
        .get_application()
        .config()
        .unwrap()
        .set("cache.default", json!("second"))
        .unwrap();
    let target = second
        .get_application()
        .make::<Application>()
        .await
        .unwrap();
    first
        .get_application()
        .container()
        .instance(Arc::new(NextApplication(target)))
        .unwrap();
    assert_eq!(
        call(&first, "application").await,
        json!({"application":"ok"})
    );
    assert_eq!(call(&second, "macro-isolated").await, json!(false));
}
