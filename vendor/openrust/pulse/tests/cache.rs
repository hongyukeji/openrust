use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::{Facade, Route},
    testing::TestApp,
};
use openrust_pulse::{
    PulseServiceProvider,
    events::SharedBeat,
    recorders::{Memory, Servers},
    storage::DatabaseStorage,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
fn config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    std::fs::write(root.join("test.sqlite"), []).unwrap();
    let driver = std::env::var("PULSE_TEST_DRIVER").unwrap_or("sqlite".into());
    let connection = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":root.join("test.sqlite"),"max_connections":1})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()})
        }
        _ => panic!("unsupported isolated driver"),
    };
    let cache = std::env::var("PULSE_TEST_CACHE").unwrap_or("file".into());
    let prefix = format!("pulse-cache-{:032x}:", fastrand::u128(..));
    let mut database = json!({"default":"test","connections":{"test":connection}});
    if cache == "redis" {
        database["redis"] = json!({"client":"redis","options":{"prefix":prefix},"default":{"url":std::env::var("OPENRUST_TEST_REDIS_URL").expect("isolated Redis URL required")}});
    }
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"local","debug":true})),
        ("database".into(), database),
        ("session".into(), json!({"driver":"array"})),
        (
            "cache".into(),
            json!({"default":cache,"prefix":prefix,"stores":{"file":{"driver":"file","path":root.join("cache/data"),"lock_path":root.join("cache/locks")},"database":{"driver":"database","connection":"test","table":"cache","lock_table":"cache_locks"},"redis":{"driver":"redis","connection":"default","lock_connection":"default"}}}),
        ),
        (
            "pulse".into(),
            json!({"enabled":true,"cache":cache,"storage":{"driver":"database","database":{"chunk":1000}},"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}},"recorders":{"Servers":{"server_name":"matrix","directories":[]},"CacheInteractions":false,"SlowQueries":false}}),
        ),
    ]))
}

async fn create_cache_table(connection: &openrust::database::Connection) {
    connection
        .get_schema_builder()
        .create("cache", |table| {
            table.string("key", None).primary();
            table.medium_text("value");
            table.big_integer("expiration");
        })
        .await
        .unwrap();
}

fn cache_interactions(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::support::facades::{Cache, Config};
    let discard = request.uri().path().ends_with("/sampled-out");
    Box::pin(async move {
        if discard {
            assert!(Cache::get("sampled-out").await?.is_none());
            Config::get_facade_root()?
                .set("pulse.recorders.CacheInteractions.sample_rate", json!(0))?;
            return Ok(Response::from("sampled out"));
        }
        let start = jiff::Timestamp::now().as_second();
        assert!(Cache::get("feature:123").await?.is_none());
        let payload = openrust::cache::Value::from("value");
        Cache::put("feature:123", &payload, None).await?;
        assert_eq!(Cache::get("feature:123").await?, Some(payload));
        assert!(Cache::get("secret:1").await?.is_none());
        let end = jiff::Timestamp::now().as_second();
        // 让缓存事件与 HTTP 终止时的写入落在不同秒。
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while jiff::Timestamp::now().as_second() <= end {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Response::json(openrust::http::StatusCode::OK, json!([start, end]))
    })
}

#[tokio::test]
async fn cache_interactions_preserve_source_time_and_count_on_native_driver() {
    use openrust::database::Value;
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = config(root, env)?;
                values.get_mut("pulse").unwrap()["recorders"] = json!({
                    "Servers": false,
                    "SlowQueries": false,
                    "CacheInteractions": {
                        "sample_rate": 1,
                        "ignore": [r"/\Asecret:/"],
                        "groups": [[r"/\Afeature:\d+\z/", "feature:*"], ["/feature/", "second"]]
                    }
                });
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/cache-interactions", cache_interactions)?;
                Route::get("/cache-interactions/sampled-out", cache_interactions)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    if std::env::var("PULSE_TEST_CACHE").as_deref() == Ok("database") {
        create_cache_table(storage.connection()).await;
    }

    let response = app.get("/api/cache-interactions").await.unwrap();
    response.assert_ok();
    let [start, end] = serde_json::from_slice::<[i64; 2]>(response.body()).unwrap();
    app.get("/api/cache-interactions/sampled-out")
        .await
        .unwrap()
        .assert_ok();
    let types = ["cache_miss".to_owned(), "cache_hit".to_owned()];
    let rows = storage
        .connection()
        .table("pulse_entries")
        .where_in("type", types.iter().cloned().map(Value::Text).collect())
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (row, kind) in rows.iter().zip(&types) {
        assert_eq!(row.get("type"), Some(&Value::Text(kind.clone())));
        assert_eq!(row.get("key"), Some(&Value::Text("feature:*".into())));
        assert_eq!(row.get("value"), Some(&Value::Null));
        let timestamp = match row.get("timestamp") {
            Some(Value::Integer(timestamp)) => *timestamp,
            Some(Value::Unsigned(timestamp)) => i64::try_from(*timestamp).unwrap(),
            other => panic!("无效的缓存事件时间戳：{other:?}"),
        };
        assert!((start..=end).contains(&timestamp), "{row:?}");
    }
    let counts = storage
        .aggregate_total(&types, "count", 3600)
        .await
        .unwrap();
    for kind in types {
        assert_eq!(counts[&kind], Some(1.0));
    }
}

struct Sampler;
impl Facade for Sampler {
    type Service = Servers;
    fn get_facade_accessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<Servers>()
    }
}
fn beats(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let pulse = openrust_pulse::facades::Pulse::manager()?;
        let servers = Sampler::get_facade_root()?;
        for (time, instance) in [(100, "one"), (114, "one"), (115, "one"), (114, "two")] {
            servers
                .record(
                    &SharedBeat {
                        time,
                        instance: instance.into(),
                    },
                    &pulse,
                )
                .await?;
        }
        Ok(Response::from("recorded"))
    })
}
#[tokio::test]
async fn native_cache_driver_throttles_by_instance_and_shares_restart_signal() {
    use openrust::contracts::cache::{LockProvider as _, Repository as _};
    let mut app = TestApp::create(|b| {
        b.with_config(config)
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/beats", beats)?;
                Ok(())
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    let db = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    create_cache_table(&db).await;
    db.get_schema_builder()
        .create("cache_locks", |t| {
            t.string("key", None).primary();
            t.string("owner", None);
            t.big_integer("expiration");
        })
        .await
        .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let servers = app.app().make::<Servers>().await.unwrap();
    servers.detect_cpu_using(Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        17
    })));
    servers.detect_memory_using(Some(Arc::new(|| Memory {
        total: 4096,
        used: 1024,
    })));
    let response = app.get("/api/beats").await.unwrap();
    assert_eq!(
        response.status().as_u16(),
        200,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    assert_eq!(storage.values("system", None).await.unwrap().len(), 1);
    app.artisan(["pulse:restart"]).await.assert_successful();
    let cache = app
        .app()
        .make::<openrust::cache::CacheManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    assert!(cache.get("openrust:pulse:restart").await.unwrap().is_some());
    let mut first = cache
        .lock(
            "openrust:pulse:check",
            Some(std::time::Duration::from_secs(1)),
            None,
        )
        .unwrap();
    let mut second = cache
        .lock(
            "openrust:pulse:check",
            Some(std::time::Duration::from_secs(1)),
            None,
        )
        .unwrap();
    assert!(first.get().await.unwrap());
    assert!(!second.get().await.unwrap());
    first.release().await.unwrap();
    assert!(second.get().await.unwrap());
    second.release().await.unwrap();
    cache.forget("openrust:pulse:restart").await.unwrap();
    for instance in ["one", "two"] {
        cache
            .forget(&format!("openrust:pulse:throttle:{instance}:Servers"))
            .await
            .unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn pulse_work_and_check_use_their_own_iteration_schedules() {
    use openrust::{
        contracts::{
            cache::Repository as _,
            container::Container as _,
            events::{Dispatcher as _, Event, Listener},
            foundation::Application as _,
        },
        events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
        support::facades::Cache,
    };
    use openrust_pulse::{
        Item, PulseFuture,
        contracts::{Ingest, Storage},
    };
    use std::{sync::Mutex, time::Duration};
    use tokio::time::{Instant, sleep};

    struct CadenceProbe {
        starts: Mutex<Vec<Instant>>,
    }

    impl CadenceProbe {
        async fn record(
            &self,
            delay: Duration,
            restart_value: i64,
        ) -> Result<(), openrust::cache::CacheError> {
            let pass = {
                let mut starts = self.starts.lock().unwrap();
                starts.push(Instant::now());
                starts.len()
            };
            sleep(delay).await;
            if pass == 2 {
                Cache::store(Some("array"))?
                    .forever(
                        "openrust:pulse:restart",
                        &openrust::cache::Value::from(restart_value),
                    )
                    .await?;
            }
            Ok(())
        }

        fn assert_interval(&self, expected: Duration) {
            let starts = self.starts.lock().unwrap();
            assert_eq!(starts.len(), 2);
            let interval = starts[1] - starts[0];
            assert!((expected..expected + Duration::from_millis(10)).contains(&interval));
        }
    }

    impl Ingest for CadenceProbe {
        fn ingest<'a>(&'a self, _: &'a [Item]) -> PulseFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn trim(&self) -> PulseFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn digest<'a>(&'a self, _: &'a dyn Storage) -> PulseFuture<'a, usize> {
            Box::pin(async {
                self.record(Duration::from_secs(2), 1).await?;
                Ok(0)
            })
        }
    }

    impl Listener for CadenceProbe {
        fn handle<'a>(
            &'a self,
            _: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async {
                self.record(Duration::from_millis(400), 2).await.unwrap();
                Ok(None)
            })
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut settings = config(root, env)?;
                let cache = settings.get_mut("cache").unwrap();
                cache["default"] = json!("array");
                cache["stores"]["array"] = json!({"driver":"array"});
                let pulse = settings.get_mut("pulse").unwrap();
                pulse["cache"] = json!("array");
                pulse["recorders"]["Servers"] = json!(false);
                Ok(settings)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
    })
    .unwrap();
    let ingest = Arc::new(CadenceProbe {
        starts: Mutex::default(),
    });
    app.app()
        .container()
        .instance::<dyn Ingest>(ingest.clone())
        .unwrap();
    app.boot().await.unwrap();
    tokio::time::timeout(Duration::from_secs(12), app.artisan(["pulse:work"]))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "pulse:work 未退出，已读次数为 {}",
                ingest.starts.lock().unwrap().len()
            )
        })
        .assert_successful();
    ingest.assert_interval(Duration::from_secs(3));

    let listener = Arc::new(CadenceProbe {
        starts: Mutex::default(),
    });
    let dispatcher = app
        .app()
        .make::<openrust::events::Dispatcher>()
        .await
        .unwrap();
    let beat = SharedBeat {
        time: 0,
        instance: String::new(),
    };
    dispatcher
        .listen(beat.name(), listener.clone(), Delivery::Sync)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(12), app.artisan(["pulse:check"]))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "pulse:check 未退出，已检查次数为 {}",
                listener.starts.lock().unwrap().len()
            )
        })
        .assert_successful();
    listener.assert_interval(Duration::from_secs(1));
}

#[tokio::test]
async fn pulse_check_uses_the_public_beat_instance_and_vapor_defaults() {
    use openrust::{
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    };
    use std::{sync::Mutex, time::Duration};

    #[derive(Default)]
    struct BeatInstances(Mutex<Vec<String>>);

    impl Listener for BeatInstances {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async {
                let beat = event.downcast_ref::<SharedBeat>().unwrap();
                self.0.lock().unwrap().push(beat.instance.clone());
                Ok(None)
            })
        }
    }

    for (value, vapor) in [
        ("0", false),
        ("FaLsE", false),
        ("(false)", false),
        ("(null)", false),
        ("EMPTY", false),
        (r#"'"0"'"#, false),
        (r#"'"false"'"#, true),
        ("/parameter/path", true),
    ] {
        let mut app = TestApp::create(|builder| {
            builder
                .with_config(|root, env| {
                    let mut settings = config(root, env)?;
                    settings.get_mut("pulse").unwrap()["recorders"]["Servers"] = json!(false);
                    Ok(settings)
                })
                .with_providers(vec![Box::new(PulseServiceProvider)])
        })
        .unwrap();
        std::fs::write(
            app.root().join(".env.testing"),
            format!("VAPOR_SSM_PATH={value}\n"),
        )
        .unwrap();
        app.boot().await.unwrap();
        let listener = Arc::new(BeatInstances::default());
        app.app()
            .make::<openrust::events::Dispatcher>()
            .await
            .unwrap()
            .listen(
                "OpenRust\\Pulse\\Events\\SharedBeat",
                listener.clone(),
                Delivery::Sync,
            )
            .unwrap();
        let args = if vapor {
            vec!["pulse:check"]
        } else {
            vec!["pulse:check", "--once"]
        };
        tokio::time::timeout(Duration::from_secs(10), app.artisan(args))
            .await
            .unwrap()
            .assert_successful();
        let instances = listener.0.lock().unwrap();
        assert_eq!(instances.len(), 1, "{value}");
        if vapor {
            assert_eq!(instances[0], "vapor", "{value}");
        } else {
            assert_eq!(instances[0].len(), 16, "{value}");
            assert!(
                instances[0]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric()),
                "{value}"
            );
        }
    }
}

#[tokio::test]
async fn binary_cache_groups_follow_the_storage_driver_and_keep_distinct_metrics_rows() {
    use openrust::database::Value;
    use openrust::support::facades::Cache;
    use openrust_pulse::{Entry, Pulse};

    fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        if request.uri().path().ends_with("/graph") {
            return Box::pin(async {
                let graph = openrust_pulse::facades::Pulse::graph_bytes(
                    &["cache_miss".into()],
                    "count",
                    3600,
                )
                .await?;
                let summary = graph
                    .into_iter()
                    .map(|(key, series)| {
                        let counts = &series["cache_miss"];
                        (key, counts.len(), counts.values().flatten().sum::<f64>())
                    })
                    .collect::<Vec<_>>();
                Response::json(openrust::http::StatusCode::OK, summary)
            });
        }
        let key = match request.uri().path().rsplit('/').next() {
            Some("first") => "é",
            Some("second") => "ê",
            _ => "valid",
        };
        Box::pin(async move {
            assert!(Cache::get(key).await?.is_none());
            Ok(Response::from("ok"))
        })
    }

    let redis_ingest = std::env::var_os("PULSE_TEST_INGEST_REDIS").is_some();
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = config(root, env)?;
                values.get_mut("app").unwrap()["key"] =
                    "01234567890123456789012345678901".into();
                values.get_mut("app").unwrap()["cipher"] = "AES-256-CBC".into();
                values.get_mut("database").unwrap()["connections"]["test"]["max_connections"] =
                    json!(1);
                values.get_mut("pulse").unwrap()["recorders"]["CacheInteractions"] =
                    json!({"sample_rate":1,"ignore":[],"groups":[["/(?=)/","X"]]});
                if std::env::var_os("PULSE_TEST_INGEST_REDIS").is_some() {
                    values.get_mut("database").unwrap()["redis"] = json!({
                        "client": "redis",
                        "options": {"prefix": format!("pulse-ingest-binary-{:032x}:", fastrand::u128(..))},
                        "default": {"url": std::env::var("OPENRUST_TEST_REDIS_URL").expect("isolated Redis URL required")},
                    });
                    values.get_mut("pulse").unwrap()["ingest"]["driver"] = "redis".into();
                    values.get_mut("pulse").unwrap()["ingest"]["redis"] = json!({"chunk": 2});
                }
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Route::get("/binary-group/first", probe)?;
                Route::get("/binary-group/second", probe)?;
                Route::get("/binary-group/valid", probe)?;
                Route::get("/binary-group/graph", probe)?;
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let db = storage.connection();
    let driver = db.get_driver_name();
    let non_strict = driver == "mysql" && std::env::var_os("PULSE_TEST_MYSQL_NON_STRICT").is_some();
    if driver == "mysql" {
        let sql = if non_strict {
            "set session sql_mode=''"
        } else {
            "set session sql_mode='STRICT_TRANS_TABLES'"
        };
        db.statement(sql, &[]).await.unwrap();
    }

    let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = errors.clone();
    app.app()
        .make::<Pulse>()
        .await
        .unwrap()
        .handle_exceptions_using(move |error| {
            if let openrust_pulse::Error::Database(openrust::database::DatabaseError::Driver {
                source,
                ..
            }) = error
            {
                captured.lock().unwrap().push(
                    source
                        .as_database_error()
                        .unwrap()
                        .code()
                        .unwrap()
                        .into_owned(),
                );
            } else {
                panic!("分组键应进入数据库：{error}");
            }
        });
    for route in ["first", "second", "valid"] {
        app.get(&format!("/api/binary-group/{route}"))
            .await
            .unwrap()
            .assert_ok();
    }
    if redis_ingest {
        assert_eq!(
            app.app()
                .make::<Pulse>()
                .await
                .unwrap()
                .digest()
                .await
                .unwrap(),
            3
        );
    }
    let rows = db
        .table("pulse_entries")
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    let valid = Value::from("XvXaXlXiXdX");
    let response = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    assert_eq!(
        response.status().as_u16(),
        200,
        "{}",
        String::from_utf8_lossy(response.body())
    );
    let cards: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
    let warm = app.get("/pulse/api/metrics?period=1h").await.unwrap();
    warm.assert_ok();
    let warm: serde_json::Value = serde_json::from_slice(warm.body()).unwrap();
    assert_eq!(warm["cache"], cards["cache"]);
    if redis_ingest {
        let pulse = app.app().make::<Pulse>().await.unwrap();
        assert_eq!(pulse.digest().await.unwrap(), 0);
    }
    let cache = cards["cache"].as_array().unwrap();

    match driver {
        "sqlite" => {
            let first = vec![b'X', 0xc3, b'X', 0xa9, b'X'];
            let second = vec![b'X', 0xc3, b'X', 0xaa, b'X'];
            assert_eq!(rows.len(), 3);
            for (row, raw) in rows.iter().zip([first.clone(), second.clone()]) {
                let hash = format!("{:x}", md5::compute(&raw));
                assert_eq!(row.get("key"), Some(&Value::Bytes(raw)));
                assert_eq!(row.get("key_hash"), Some(&Value::from(hash)));
            }
            assert_eq!(rows[2].get("key"), Some(&valid));
            let storage_type = db
                .scalar(
                    "select typeof(key) as kind from pulse_entries order by id limit 1",
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(storage_type, Value::from("text"));
            let display = cache
                .iter()
                .filter(|row| row["key"] == "X�X�X")
                .collect::<Vec<_>>();
            assert_eq!(display.len(), 2);
            assert!(
                display
                    .iter()
                    .all(|row| row["cache_miss"].as_f64() == Some(1.0))
            );
            assert!(errors.lock().unwrap().is_empty());
            let response = app.get("/api/binary-group/graph").await.unwrap();
            response.assert_ok();
            let graph: Vec<(Vec<u8>, usize, f64)> =
                serde_json::from_slice(response.body()).unwrap();
            assert_eq!(
                graph,
                [
                    (b"XvXaXlXiXdX".to_vec(), 60, 1.0),
                    (first, 60, 1.0),
                    (second, 60, 1.0),
                ]
            );
        }
        "mysql" if non_strict => {
            assert_eq!(rows.len(), 3);
            assert!(
                rows[..2]
                    .iter()
                    .all(|row| row.get("key") == Some(&Value::from("X")))
            );
            let truncated = cache.iter().find(|row| row["key"] == "X").unwrap();
            let misses = &truncated["cache_miss"];
            assert_eq!(
                misses.as_f64().or_else(|| misses.as_str()?.parse().ok()),
                Some(2.0),
                "{truncated}"
            );
            assert!(errors.lock().unwrap().is_empty());
        }
        "pgsql" | "mysql" => {
            let state = if driver == "pgsql" { "22021" } else { "HY000" };
            assert_eq!(*errors.lock().unwrap(), [state, state]);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("key"), Some(&valid));

            let now = jiff::Timestamp::now().as_second();
            let storage = DatabaseStorage::new(db.clone()).with_chunk(1).unwrap();
            let batch = [
                Entry::new(now, "cache_miss", "batch-valid", None)
                    .count()
                    .into(),
                Entry::from_key_bytes(now, "cache_miss", vec![b'X', 0xc3, b'X'], None)
                    .count()
                    .into(),
            ];
            assert!(storage.store(&batch).await.is_err());
            assert_eq!(db.table("pulse_entries").count("*").await.unwrap(), 1);
            assert_eq!(db.table("pulse_aggregates").count("*").await.unwrap(), 4);
        }
        other => panic!("{other}"),
    }
}

#[tokio::test]
async fn value_keys_round_trip_through_the_pulse_facade_and_storage() {
    use openrust::database::Value;
    use openrust_pulse::{ByteValue, Pulse, facades::Pulse as PulseFacade};

    const RAW: &[u8] = b"X\xc3X\xa9X";
    const OTHER: &[u8] = b"X\xc3X\xaaX";

    fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        let operation = request.uri().path().rsplit('/').next().unwrap().to_owned();
        Box::pin(async move {
            let now = jiff::Timestamp::now().as_second();
            match operation.as_str() {
                "write" => {
                    PulseFacade::set_bytes("probe", RAW.to_vec(), "first", Some(now)).await?;
                    let last = PulseFacade::set_bytes("probe", RAW.to_vec(), "last", Some(now - 1))
                        .await?;
                    assert_eq!(last.key, RAW);
                    assert_eq!(last.timestamp, now - 1);
                }
                "distinct" => {
                    PulseFacade::set_bytes("probe", OTHER.to_vec(), "other", Some(now)).await?;
                    PulseFacade::set("probe", "X�X�X", "display", Some(now)).await?;
                }
                "text" => {
                    PulseFacade::set("probe", "text", "first", Some(now)).await?;
                    PulseFacade::set_bytes("probe", b"text".to_vec(), "text-last", Some(now - 1))
                        .await?;
                }
                "read" => {
                    let keys = [RAW.to_vec(), b"text".to_vec()];
                    let rows = PulseFacade::values_bytes("probe", Some(&keys)).await?;
                    assert_eq!(rows.len(), 2);
                    let raw = rows
                        .iter()
                        .find(|row| row.get("key") == Some(&Value::Bytes(RAW.to_vec())));
                    assert_eq!(raw.unwrap().get("value"), Some(&Value::from("last")));
                    return Ok(Response::from("read"));
                }
                _ => unreachable!(),
            }
            Ok(Response::from("written"))
        })
    }

    let redis_ingest = std::env::var_os("PULSE_TEST_INGEST_REDIS").is_some();
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|root, env| {
                let mut values = config(root, env)?;
                values.get_mut("database").unwrap()["connections"]["test"]["max_connections"] =
                    json!(1);
                if std::env::var_os("PULSE_TEST_INGEST_REDIS").is_some() {
                    values.get_mut("database").unwrap()["redis"] = json!({
                        "client":"redis",
                        "options":{"prefix":format!("pulse-values-{:032x}:", fastrand::u128(..))},
                        "default":{"url":std::env::var("OPENRUST_TEST_REDIS_URL").unwrap()},
                    });
                    let ingest = &mut values.get_mut("pulse").unwrap()["ingest"];
                    ingest["driver"] = "redis".into();
                    ingest["redis"] = json!({"chunk":10});
                }
                Ok(values)
            })
            .with_providers(vec![Box::new(PulseServiceProvider)])
            .with_routing(Routing::default().api(|| {
                for operation in ["write", "distinct", "text", "read"] {
                    Route::get(&format!("/value-keys/{operation}"), probe)?;
                }
                Ok(())
            }))
    })
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();

    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let db = storage.connection();
    let driver = db.get_driver_name();
    let non_strict = driver == "mysql" && std::env::var_os("PULSE_TEST_MYSQL_NON_STRICT").is_some();
    if driver == "mysql" {
        let sql = if non_strict {
            "set session sql_mode=''"
        } else {
            "set session sql_mode='STRICT_TRANS_TABLES'"
        };
        db.statement(sql, &[]).await.unwrap();
    }

    app.get("/api/value-keys/text").await.unwrap().assert_ok();
    let pulse = app.app().make::<Pulse>().await.unwrap();
    if redis_ingest {
        assert_eq!(pulse.digest().await.unwrap(), 2);
    }
    let text = storage
        .values("probe", Some(&["text".into()]))
        .await
        .unwrap();
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].get("value"), Some(&Value::from("text-last")));
    assert_eq!(
        storage
            .values_bytes("probe", Some(&[b"text".to_vec()]))
            .await
            .unwrap(),
        text
    );
    if driver == "sqlite" || non_strict {
        app.get("/api/value-keys/write").await.unwrap().assert_ok();
        if driver == "sqlite" {
            app.get("/api/value-keys/distinct")
                .await
                .unwrap()
                .assert_ok();
        }
        if redis_ingest {
            assert_eq!(pulse.digest().await.unwrap(), 4);
            assert_eq!(pulse.digest().await.unwrap(), 0);
        }
        let expected_key = if driver == "sqlite" {
            Value::Bytes(RAW.to_vec())
        } else {
            Value::from("X")
        };
        let key = if driver == "sqlite" { RAW } else { b"X" };
        let rows = storage
            .values_bytes("probe", Some(&[key.to_vec()]))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("key"), Some(&expected_key));
        assert_eq!(rows[0].get("value"), Some(&Value::from("last")));
        if driver == "sqlite" {
            app.get("/api/value-keys/read").await.unwrap().assert_ok();
            assert_eq!(storage.values_bytes("probe", None).await.unwrap().len(), 4);
            let kind = db
                .scalar(
                    "select typeof(key) as kind from pulse_values where value = 'last'",
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(kind, Value::from("text"));
        }
    } else {
        let storage = DatabaseStorage::new(db.clone()).with_chunk(1).unwrap();
        let now = jiff::Timestamp::now().as_second();
        let batch = [
            ByteValue::from_key_bytes(now, "probe", RAW.to_vec(), "failed").into(),
            ByteValue::from_key_bytes(now, "probe", b"rollback".to_vec(), "failed").into(),
        ];
        let error = storage.store(&batch).await.unwrap_err();
        let openrust_pulse::Error::Database(openrust::database::DatabaseError::Driver {
            source,
            ..
        }) = error
        else {
            panic!("预期数据库驱动拒绝无效 UTF-8 键：{error}");
        };
        let code = source.as_database_error().unwrap().code().unwrap();
        assert_eq!(code, if driver == "pgsql" { "22021" } else { "HY000" });
        assert_eq!(storage.values_bytes("probe", None).await.unwrap(), text);
    }
}
