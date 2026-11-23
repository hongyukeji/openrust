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
            t.mediumText("value");
            t.bigInteger("expiration");
        })
        .await
        .unwrap();
    schema
        .create("cache_locks", |t| {
            t.string("key", None).primary();
            t.string("owner", None);
            t.bigInteger("expiration");
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
