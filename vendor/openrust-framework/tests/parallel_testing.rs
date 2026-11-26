use futures_util::FutureExt as _;
use openrust::{
    support::facades::{App, Config, Facade, ParallelTesting},
    testing::{ParallelTestError, ParallelTesting as Service, TestApp},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
fn server(flag: &str, token: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::from([
        ("LARAVEL_PARALLEL_TESTING".into(), flag.into()),
        (
            "LARAVEL_PARALLEL_TESTING_RECREATE_DATABASES".into(),
            "1".into(),
        ),
    ]);
    if let Some(token) = token {
        out.insert("TEST_TOKEN".into(), token.into());
    }
    out
}
async fn fixture() -> TestApp {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn options_tokens_and_gate_keep_false_and_string_zero_distinct() {
    let app = fixture().await;
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/parallel-testing/lifecycle-contract.json"
    ))
    .unwrap();
    for row in expected["gates"].as_array().unwrap() {
        let flag = row["flag"].as_str().unwrap();
        let token = row["token"].as_str();
        let active = row["calls"].as_u64().unwrap();
        let service = Service::new(server(flag, token));
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        service.set_up_process(move |_, _| {
            called.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        });
        assert_eq!(service.option("recreate_databases").unwrap(), json!("1"));
        assert_eq!(service.option("missing").unwrap(), json!(false));
        assert_eq!(
            service.token().unwrap(),
            token.map_or(json!(false), |v| json!(v))
        );
        service
            .call_set_up_process_callbacks(app.app())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), active as usize);
    }
    App::set_facade_application(Some(app.app()), async {
        ParallelTesting::swap(Arc::new(Service::new(server("1", Some("default"))))).unwrap();
        ParallelTesting::resolve_options_using(Some(Arc::new(|key| Ok(json!({"key":key})))))
            .unwrap();
        assert_eq!(
            ParallelTesting::option("raw").unwrap(),
            json!({"key":"raw"})
        );
        ParallelTesting::resolve_options_using(None).unwrap();
        assert_eq!(ParallelTesting::option("missing").unwrap(), json!(false));
        ParallelTesting::resolve_token_using(Some(Arc::new(|| Ok(json!("custom"))))).unwrap();
        assert_eq!(ParallelTesting::token().unwrap(), json!("custom"));
        ParallelTesting::resolve_token_using(None).unwrap();
        assert_eq!(ParallelTesting::token().unwrap(), json!("default"));
    })
    .await;
}
#[tokio::test]
async fn public_process_and_real_case_lifecycles_run_in_order_and_cleanup_failures() {
    let mut app = fixture().await;
    let application =
        App::set_facade_application(Some(app.app()), async { App::get_facade_root().unwrap() })
            .await;
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    App::set_facade_application(Some(&application), async {
        ParallelTesting::swap(Arc::new(Service::new(server("1", Some("2"))))).unwrap();
        let log = events.clone();
        ParallelTesting::set_up_process(move |_, token| {
            let log = log.clone();
            Box::pin(async move {
                Config::set("marker", json!("process"))?;
                log.lock().unwrap().push(format!("process+:{token}"));
                Ok(())
            })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::tear_down_process(move |_, token| {
            let log = log.clone();
            Box::pin(async move {
                log.lock().unwrap().push(format!("process-:{token}"));
                Ok(())
            })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::set_up_test_case(move |case, token| {
            let log = log.clone();
            Box::pin(async move {
                assert!(case.root().exists());
                assert_eq!(Config::string("marker")?, "process");
                log.lock().unwrap().push(format!("case+:{token}"));
                Ok(())
            })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::tear_down_test_case(move |case, token| {
            let log = log.clone();
            Box::pin(async move {
                assert!(case.root().exists());
                log.lock().unwrap().push(format!("case-:{token}"));
                Ok(())
            })
        })
        .unwrap();
        let result = ParallelTesting::run_process(async {
            app.run_parallel_test(|case| {
                Box::pin(async move {
                    case.get("/missing").await?.assert_not_found();
                    Err::<(), ParallelTestError>(std::io::Error::other("test failure").into())
                })
            })
            .await
        })
        .await;
        assert_eq!(result.unwrap_err().to_string(), "test failure");
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "process+:\"2\"",
                "case+:\"2\"",
                "case-:\"2\"",
                "process-:\"2\""
            ]
        );
        events.lock().unwrap().clear();
        let result = AssertUnwindSafe(ParallelTesting::run_process(async {
            app.run_parallel_test(|_| {
                Box::pin(async {
                    assert!(std::hint::black_box(false), "test panic");
                    Ok::<(), ParallelTestError>(())
                })
            })
            .await
        }))
        .catch_unwind()
        .await;
        assert!(result.is_err());
        assert_eq!(events.lock().unwrap().len(), 4);
    })
    .await;
}
#[tokio::test]
async fn callbacks_can_register_more_callbacks_and_resolvers_can_reenter() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        ParallelTesting::swap(Arc::new(Service::new(server("1", Some("1"))))).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        ParallelTesting::set_up_process(move |_, _| {
            let count = count.clone();
            Box::pin(async move {
                let count = count.clone();
                ParallelTesting::set_up_process(move |_, _| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async { Ok(()) })
                })?;
                Ok(())
            })
        })
        .unwrap();
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        ParallelTesting::resolve_token_using(Some(Arc::new(|| {
            ParallelTesting::resolve_token_using(None)?;
            Ok(json!("once"))
        })))
        .unwrap();
        assert_eq!(ParallelTesting::token().unwrap(), json!("once"));
        assert_eq!(ParallelTesting::token().unwrap(), json!("1"));
    })
    .await;
}
#[tokio::test]
async fn applications_have_independent_callbacks_and_resolvers() {
    let first = fixture().await;
    let second = fixture().await;
    App::set_facade_application(Some(first.app()), async {
        ParallelTesting::resolve_token_using(Some(Arc::new(|| Ok(json!("first"))))).unwrap();
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        ParallelTesting::resolve_token_using(Some(Arc::new(|| Ok(json!("second"))))).unwrap();
    })
    .await;
    let (a, b) = tokio::join!(
        App::set_facade_application(Some(first.app()), async {
            ParallelTesting::token().unwrap()
        }),
        App::set_facade_application(Some(second.app()), async {
            ParallelTesting::token().unwrap()
        })
    );
    assert_eq!((a, b), (json!("first"), json!("second")));
}

struct Migration;
impl openrust::database::migrations::Migration for Migration {
    fn up<'a>(
        &'a self,
        context: &'a mut openrust::database::migrations::MigrationContext,
    ) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<()>> {
        Box::pin(async {
            context
                .get_schema_builder()
                .create("parallel_fixture", |t| {
                    t.id();
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut openrust::database::migrations::MigrationContext,
    ) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<()>> {
        Box::pin(async { context.get_schema_builder().drop("parallel_fixture").await })
    }
}
static MIGRATION: Migration = Migration;
static MIGRATIONS: &[openrust::database::migrations::MigrationEntry] =
    &[openrust::database::migrations::MigrationEntry::new(
        "database/migrations/0001_parallel_fixture.rs",
        &MIGRATION,
    )];
#[tokio::test]
async fn real_mysql_preparation_runs_database_callbacks_before_and_after_first_migration_only() {
    let url =
        std::env::var("OPENRUST_TEST_MYSQL_URL").expect("必须通过独立MySQL验收夹具运行本测试");
    for mode in ["refresh", "migrations"] {
        let mut app = TestApp::mysql(&url, |b| b.with_migrations(MIGRATIONS))
            .await
            .unwrap();
        app.boot().await.unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        App::set_facade_application(Some(app.app()), async {
            ParallelTesting::swap(Arc::new(Service::new(server("1", Some("db-worker"))))).unwrap();
            let log = events.clone();
            ParallelTesting::set_up_test_database_before_migrating(move |app, database, token| {
                let log = log.clone();
                Box::pin(async move {
                    let manager = app.make::<openrust::database::DatabaseManager>().await?;
                    let connection = manager.connection(None)?;
                    assert_eq!(connection.get_database_name(), database);
                    assert!(
                        !connection
                            .get_schema_builder()
                            .has_table("parallel_fixture")
                            .await?
                    );
                    assert_eq!(token, json!("db-worker"));
                    log.lock().unwrap().push("before");
                    Ok(())
                })
            })
            .unwrap();
            let log = events.clone();
            ParallelTesting::set_up_test_database(move |app, database, token| {
                let log = log.clone();
                Box::pin(async move {
                    let manager = app.make::<openrust::database::DatabaseManager>().await?;
                    let connection = manager.connection(None)?;
                    assert_eq!(connection.get_database_name(), database);
                    assert!(
                        connection
                            .get_schema_builder()
                            .has_table("parallel_fixture")
                            .await?
                    );
                    assert_eq!(token, json!("db-worker"));
                    log.lock().unwrap().push("after");
                    Ok(())
                })
            })
            .unwrap();
        })
        .await;
        for _ in 0..2 {
            if mode == "refresh" {
                app.refresh_database(Default::default(), |case| {
                    Box::pin(async {
                        case.assert_database_count("parallel_fixture", 0).await?;
                        Ok(())
                    })
                })
                .await
                .unwrap();
            } else {
                app.run_database_migrations(Default::default(), |case| {
                    Box::pin(async {
                        case.assert_database_count("parallel_fixture", 0).await?;
                        Ok(())
                    })
                })
                .await
                .unwrap();
            }
        }
        assert_eq!(*events.lock().unwrap(), vec!["before", "after"]);
        app.close().await.unwrap();
    }
}

#[tokio::test]
async fn every_public_callback_invoker_matches_original_order_and_arguments() {
    let mut case = fixture().await;
    let app =
        App::set_facade_application(Some(case.app()), async { App::get_facade_root().unwrap() })
            .await;
    let events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    App::set_facade_application(Some(&app), async {
        ParallelTesting::swap(Arc::new(Service::new(server("1", Some("trace"))))).unwrap();
        let log = events.clone();
        ParallelTesting::set_up_process(move |_, token| {
            log.lock().unwrap().push(json!(["process+", token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::tear_down_process(move |_, token| {
            log.lock().unwrap().push(json!(["process-", token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::set_up_test_case(move |_, token| {
            log.lock().unwrap().push(json!(["case+", token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::tear_down_test_case(move |_, token| {
            log.lock().unwrap().push(json!(["case-", token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::set_up_test_database_before_migrating(move |_, database, token| {
            log.lock()
                .unwrap()
                .push(json!(["database-before", database, token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        let log = events.clone();
        ParallelTesting::set_up_test_database(move |_, database, token| {
            log.lock()
                .unwrap()
                .push(json!(["database+", database, token]));
            Box::pin(async { Ok(()) })
        })
        .unwrap();
        ParallelTesting::call_set_up_process_callbacks()
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_case_callbacks(&mut case)
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_database_before_migrating_callbacks("isolated")
            .await
            .unwrap();
        ParallelTesting::call_set_up_test_database_callbacks("isolated")
            .await
            .unwrap();
        ParallelTesting::call_tear_down_test_case_callbacks(&mut case)
            .await
            .unwrap();
        ParallelTesting::call_tear_down_process_callbacks()
            .await
            .unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/parallel-testing/lifecycle-contract.json"
        ))
        .unwrap();
        assert_eq!(json!(*events.lock().unwrap()), expected["callbacks"]);
        assert_eq!(Config::get("cache.prefix").unwrap(), None);
        assert_eq!(Config::get("view.compiled").unwrap(), None);
    })
    .await;
}
#[tokio::test]
async fn real_mysql_failed_before_callback_does_not_reclassify_existing_database_as_new() {
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").expect("必须提供独立MySQL验收连接");
    let mut case = TestApp::mysql(&url, |b| b.with_migrations(MIGRATIONS))
        .await
        .unwrap();
    case.boot().await.unwrap();
    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));
    App::set_facade_application(Some(case.app()), async {
        ParallelTesting::swap(Arc::new(Service::new(server("1", Some("failure"))))).unwrap();
        let count = before.clone();
        ParallelTesting::set_up_test_database_before_migrating(move |_, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(std::io::Error::other("before failure").into()) })
        })
        .unwrap();
        let count = after.clone();
        ParallelTesting::set_up_test_database(move |_, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        })
        .unwrap();
    })
    .await;
    let result = case
        .run_database_migrations(Default::default(), |_| Box::pin(async { Ok(()) }))
        .await;
    assert!(result.is_err());
    case.run_database_migrations(Default::default(), |case| {
        Box::pin(async {
            case.assert_database_count("parallel_fixture", 0).await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(before.load(Ordering::SeqCst), 1);
    assert_eq!(after.load(Ordering::SeqCst), 0);
    case.close().await.unwrap();
}
