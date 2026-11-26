use openrust::{
    contracts::foundation::Application as _,
    database::{Connection, connectors::ConnectionFactory},
    support::facades::{App, Config, Db, Facade, ParallelTesting, Schema},
    testing::{ParallelTesting as Service, TestApp},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

async fn fixture(url: Option<String>) -> TestApp {
    let mut test = TestApp::create(|b| b.with_migrations(MIGRATIONS))
        .unwrap()
        .with_parallel_database();
    test.boot().await.unwrap();
    let connection = match url {
        Some(url) => {
            let mut config = json!({"url":url, "read":[{"url":url}], "write":{"url":url}});
            if url.starts_with("postgres") {
                config["direct"] = json!({"url":url});
            }
            config
        }
        None => {
            std::fs::write(test.root().join("original.sqlite"), []).unwrap();
            json!({"driver":"sqlite", "database":"original.sqlite"})
        }
    };
    test.app()
        .config()
        .unwrap()
        .set(
            "database",
            json!({"default":"test.connection", "connections":{"test.connection":connection}}),
        )
        .unwrap();
    test
}
fn service() -> Service {
    Service::new(BTreeMap::from([
        ("LARAVEL_PARALLEL_TESTING".into(), "1".into()),
        ("OPENRUST_PARALLEL_TEST_CASE_PROCESS".into(), "1".into()),
        ("TEST_TOKEN".into(), "parallel_contract".into()),
    ]))
    .with_default_isolation()
}
async fn database_exists(connection: &Connection, name: &str) -> bool {
    let bindings: [openrust::database::Value; 1] = [name.into()];
    match connection.get_driver_name() {
        "sqlite" => std::path::Path::new(name).exists(),
        "mysql" | "mariadb" => !connection
            .select(
                "select schema_name from information_schema.schemata where schema_name = ?",
                &bindings,
            )
            .await
            .unwrap()
            .is_empty(),
        "pgsql" => !connection
            .select(
                "select datname from pg_database where datname = ?",
                &bindings,
            )
            .await
            .unwrap()
            .is_empty(),
        _ => unreachable!(),
    }
}
async fn reusable_database_contract(url: Option<String>) {
    let mut test = fixture(url).await;
    let app =
        App::set_facade_application(Some(test.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&app), async {
        let configuration =
            app.config().unwrap().get("database.connections").unwrap()["test.connection"].clone();
        let original = ConnectionFactory
            .make("original", configuration, test.root())
            .unwrap();
        let name = original.get_database_name().to_string();
        let target = format!("{name}_test_parallel_contract");
        assert!(!database_exists(&original, &target).await);
        ParallelTesting::swap(Arc::new(service())).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let before_events = events.clone();
        ParallelTesting::set_up_test_database_before_migrating(move |_, db, _| {
            let events = before_events.clone();
            Box::pin(async move {
                assert_eq!(
                    Config::get("cache.prefix")?,
                    Some(json!("test_parallel_contract_"))
                );
                events.lock().unwrap().push((true, db));
                Ok(())
            })
        })
        .unwrap();
        let after_events = events.clone();
        ParallelTesting::set_up_test_database(move |_, db, _| {
            let events = after_events.clone();
            Box::pin(async move {
                events.lock().unwrap().push((false, db));
                Ok(())
            })
        })
        .unwrap();
        test.run_parallel_test(|case| {
            Box::pin(async move {
                case.artisan(["migrate", "--force"])
                    .await
                    .assert_successful();
                assert!(Schema::has_table("parallel_migrated").await?);
                case.get("/missing").await?.assert_not_found();
                assert!(
                    Db::connection(None)?
                        .get_database_name()
                        .ends_with("_test_parallel_contract")
                );
                Db::unprepared("create table parallel_marker (id integer)").await?;
                Db::unprepared("insert into parallel_marker values (7)").await?;
                let mut roles = vec!["::read", "::write"];
                if Db::connection(None)?.get_driver_name() == "pgsql" {
                    roles.push("::direct");
                }
                for role in roles {
                    let connection = Db::connection(Some(&format!("test.connection{role}")))?;
                    assert!(
                        connection
                            .get_database_name()
                            .ends_with("_test_parallel_contract")
                    );
                    assert!(
                        connection
                            .get_schema_builder()
                            .has_table("parallel_marker")
                            .await?
                    );
                }
                Ok(())
            })
        })
        .await
        .unwrap();
        assert!(database_exists(&original, &target).await);
        test.run_parallel_test(|case| {
            Box::pin(async move {
                case.artisan(["migrate", "--force"])
                    .await
                    .assert_successful();
                assert_eq!(
                    Db::connection(None)?
                        .table("parallel_marker")
                        .count("*")
                        .await?,
                    1
                );
                Ok(())
            })
        })
        .await
        .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![(true, target.clone()), (false, target.clone())]
        );
        // 显式重建会丢弃旧测试库；默认的根数据库始终保留。
        ParallelTesting::resolve_options_using(Some(Arc::new(|key| {
            Ok(json!(matches!(
                key,
                "recreate_databases" | "drop_databases"
            )))
        })))
        .unwrap();
        test.run_parallel_test(|_| {
            Box::pin(async {
                assert!(!Schema::has_table("parallel_marker").await?);
                Db::unprepared("create table rebuilt_marker (id integer)").await?;
                Ok(())
            })
        })
        .await
        .unwrap();
        assert!(!database_exists(&original, &target).await);
        assert!(database_exists(&original, &name).await);
        assert!(
            !original
                .get_schema_builder()
                .has_table("parallel_migrated")
                .await
                .unwrap()
        );
        assert_eq!(events.lock().unwrap().len(), 4);
        ParallelTesting::resolve_options_using(Some(Arc::new(|_| Ok(json!(true))))).unwrap();
        test.run_parallel_test(|_| {
            Box::pin(async {
                assert!(!Db::connection(None)?.get_database_name().is_empty());
                Ok(())
            })
        })
        .await
        .unwrap();
        assert!(!database_exists(&original, &target).await);
        assert_eq!(events.lock().unwrap().len(), 4);
        ParallelTesting::resolve_options_using(Some(Arc::new(|key| {
            Ok(json!(matches!(
                key,
                "recreate_databases" | "drop_databases"
            )))
        })))
        .unwrap();
        let result = test
            .run_parallel_test::<()>(|_| {
                Box::pin(async { Err(std::io::Error::other("预期的测试错误").into()) })
            })
            .await;
        assert_eq!(result.unwrap_err().to_string(), "预期的测试错误");
        assert!(!database_exists(&original, &target).await);
        use futures_util::FutureExt as _;
        let panic = std::panic::AssertUnwindSafe(
            test.run_parallel_test::<()>(|_| Box::pin(async { panic!("预期的测试panic") })),
        )
        .catch_unwind()
        .await;
        assert!(panic.is_err());
        assert!(!database_exists(&original, &target).await);
    })
    .await;
}
#[tokio::test]
async fn sqlite_reuses_recreates_and_drops_only_token_database() {
    reusable_database_contract(None).await;
}
#[tokio::test]
async fn mysql_reuses_recreates_and_drops_only_token_database() {
    if std::env::var("OPENRUST_TEST_MYSQL_URL").is_err() {
        return;
    }
    reusable_database_contract(Some(
        std::env::var("OPENRUST_TEST_MYSQL_URL").expect("需要独立MySQL验收实例"),
    ))
    .await;
}
#[tokio::test]
async fn postgres_reuses_recreates_and_drops_only_token_database() {
    if std::env::var("OPENRUST_TEST_POSTGRES_URL").is_err() {
        return;
    }
    reusable_database_contract(Some(
        std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("需要独立PostgreSQL验收实例"),
    ))
    .await;
}

#[tokio::test]
async fn memory_and_without_databases_skip_all_database_side_effects() {
    for skip in [false, true] {
        let mut test = TestApp::create(|b| b).unwrap().with_parallel_database();
        test.boot().await.unwrap();
        test.app().config().unwrap().set("database", if skip {
            json!({"default":"absent", "connections":{}})
        } else {
            json!({"default":"sqlite", "connections":{"sqlite":{"driver":"sqlite", "database":":memory:"}}})
        }).unwrap();
        let app = App::set_facade_application(Some(test.app()), async {
            App::get_facade_root().unwrap()
        })
        .await;
        App::set_facade_application(Some(&app), async {
            let original = app.config().unwrap().get("database");
            ParallelTesting::swap(Arc::new(service())).unwrap();
            ParallelTesting::resolve_options_using(Some(Arc::new(move |key| {
                Ok(json!(key != "without_databases" || skip))
            })))
            .unwrap();
            ParallelTesting::set_up_test_database(|_, _, _| {
                Box::pin(async { panic!("跳过数据库时不能触发数据库钩子") })
            })
            .unwrap();
            test.run_parallel_test(|_| Box::pin(async { Ok(()) }))
                .await
                .unwrap();
            assert_eq!(app.config().unwrap().get("database"), original);
        })
        .await;
    }
}
#[tokio::test]
async fn invalid_token_fails_before_any_database_change() {
    let mut test = fixture(None).await;
    let app =
        App::set_facade_application(Some(test.app()), async { App::get_facade_root().unwrap() })
            .await;
    App::set_facade_application(Some(&app), async {
        let original = app.config().unwrap().get("database");
        ParallelTesting::swap(Arc::new(service())).unwrap();
        ParallelTesting::resolve_token_using(Some(Arc::new(|| Ok(json!("../escape"))))).unwrap();
        assert!(
            test.run_parallel_test::<()>(|_| Box::pin(async {
                panic!("非法token不应执行正文")
            }))
            .await
            .is_err()
        );
        assert_eq!(app.config().unwrap().get("database"), original);
        assert_eq!(
            std::fs::read_dir(test.root())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().contains("_test_"))
                .count(),
            0
        );
    })
    .await;
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
                .create("parallel_migrated", |table| {
                    table.id();
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut openrust::database::migrations::MigrationContext,
    ) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<()>> {
        Box::pin(async { context.get_schema_builder().drop("parallel_migrated").await })
    }
}
static MIGRATIONS: &[openrust::database::migrations::MigrationEntry] =
    &[openrust::database::migrations::MigrationEntry::new(
        "database/migrations/0001_parallel_migrated.rs",
        &Migration,
    )];

#[tokio::test]
async fn sqlite_url_parameters_and_empty_endpoint_configuration_are_preserved() {
    let mut test = fixture(None).await;
    let original = test.root().join("original.sqlite");
    let app =
        App::set_facade_application(Some(test.app()), async { App::get_facade_root().unwrap() })
            .await;
    app.config()
        .unwrap()
        .set(
            "database.connections",
            json!({"test.connection":{
                "url":format!("sqlite://{}?mode=rw&cache=private",original.display()), "direct":{}
            }}),
        )
        .unwrap();
    App::set_facade_application(Some(&app), async {
        ParallelTesting::swap(Arc::new(service())).unwrap();
        ParallelTesting::resolve_options_using(Some(Arc::new(|key| {
            Ok(json!(key == "drop_databases"))
        })))
        .unwrap();
        test.run_parallel_test(|_| {
            Box::pin(async {
                assert!(
                    Db::connection(None)?
                        .get_config("sqlite_url")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .ends_with("_test_parallel_contract?mode=rw&cache=private")
                );
                assert_eq!(
                    Config::get("database.connections")?.unwrap()["test.connection"]["direct"],
                    json!({})
                );
                Db::unprepared("create table url_options (id integer)").await?;
                Ok(())
            })
        })
        .await
        .unwrap();
        assert!(original.is_file());
        assert!(
            !original
                .with_file_name("original.sqlite_test_parallel_contract")
                .exists()
        );
    })
    .await;
}
