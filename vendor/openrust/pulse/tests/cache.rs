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
    let mut schema = db.get_schema_builder();
    schema
        .create("cache", |t| {
            t.string("key", None).primary();
            t.medium_text("value");
            t.big_integer("expiration");
        })
        .await
        .unwrap();
    schema
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
