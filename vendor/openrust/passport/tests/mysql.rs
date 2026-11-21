//! Requires the isolated MySQL 8.4 instance supplied by tests/mysql_acceptance.py.
use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::foundation::Application as _,
    database::{
        DatabaseManager,
        migrations::{Migration, MigrationEntry},
    },
    testing::TestApp,
};
use openrust_passport::{PassportServiceProvider, migrations};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

static ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new(
        "database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs",
        &migrations::oauth_auth_codes::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs",
        &migrations::oauth_access_tokens::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs",
        &migrations::oauth_refresh_tokens::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2016_06_01_000004_create_oauth_clients_table.rs",
        &migrations::oauth_clients::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/2024_06_01_000001_create_oauth_device_codes_table.rs",
        &migrations::oauth_device_codes::Definition,
    ),
];
const TABLES: &[&str] = &[
    "oauth_auth_codes",
    "oauth_access_tokens",
    "oauth_refresh_tokens",
    "oauth_clients",
    "oauth_device_codes",
];
fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let prefix = format!(
        "p{}_{}_",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        (
            "database".into(),
            json!({"default":"main","connections":{
            "main":{"url":url,"prefix":format!("{prefix}main_"),"charset":"utf8mb4","collation":"utf8mb4_unicode_ci"},
            "oauth":{"url":url,"prefix":format!("{prefix}oauth_"),"charset":"utf8mb4","collation":"utf8mb4_unicode_ci"}},"migrations":{"table":"history"}}),
        ),
        ("passport".into(), json!({"connection":"oauth"})),
    ]))
}
fn application(registered: bool) -> TestApp {
    let app = TestApp::create(|builder| {
        let builder = builder
            .withConfig(config)
            .withProviders(vec![Box::new(PassportServiceProvider)]);
        if registered {
            builder.withMigrations(ENTRIES)
        } else {
            builder
        }
    })
    .unwrap();
    let directory = app.root().join("lang/en");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("schema.json"),
        include_str!("../../../../lang/en/schema.json"),
    )
    .unwrap();
    app
}
#[tokio::test]
async fn mysql_published_schema_matches_fixed_php_and_migration_connections_are_isolated() {
    let Ok(_) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "required MySQL URL missing"
        );
        eprintln!("SKIP MySQL 8.4: OPENRUST_TEST_MYSQL_URL not supplied");
        return;
    };
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/schema.json")).unwrap();
    assert_eq!(
        corpus["passport"],
        "f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7"
    );
    assert_eq!(corpus["php"], "8.4.14");
    assert!(
        corpus["illuminate"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "v13.40.0")
    );
    let mut unregistered = application(false);
    unregistered.artisan(["migrate"]).await.assertSuccessful();
    let manager = unregistered.app().make::<DatabaseManager>().await.unwrap();
    let main = manager.connection(None).unwrap();
    let oauth = manager.connection(Some("oauth")).unwrap();
    let version = main
        .select("SELECT VERSION() AS version", &[])
        .await
        .unwrap();
    assert!(format!("{version:?}").contains("8.4."));
    for table in TABLES {
        assert!(!oauth.getSchemaBuilder().hasTable(table).await.unwrap());
    }
    main.getSchemaBuilder().drop("history").await.unwrap();
    let mut first = application(true);
    let mut second = application(true);
    let (a, b) = tokio::join!(first.artisan(["migrate"]), second.artisan(["migrate"]));
    a.assertSuccessful();
    b.assertSuccessful();
    let repeated = first.artisan(["migrate"]).await;
    repeated.assertSuccessful();
    assert!(repeated.stdout.contains("Nothing to migrate."));
    let a = first.app().make::<DatabaseManager>().await.unwrap();
    let b = second.app().make::<DatabaseManager>().await.unwrap();
    let main = a.connection(None).unwrap();
    let oauth = a.connection(Some("oauth")).unwrap();
    let other = b.connection(Some("oauth")).unwrap();
    for table in TABLES {
        assert!(!main.getSchemaBuilder().hasTable(table).await.unwrap());
        assert!(oauth.getSchemaBuilder().hasTable(table).await.unwrap());
        assert!(other.getSchemaBuilder().hasTable(table).await.unwrap());
    }
    let prefix = first
        .app()
        .config()
        .unwrap()
        .get("database.connections.main.prefix")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    for statements in corpus["migrations"].as_object().unwrap().values() {
        for sql in statements.as_array().unwrap() {
            let mut sql = sql.as_str().unwrap().to_owned();
            for table in TABLES {
                sql = sql.replace(
                    &format!("`{table}`"),
                    &format!("`{prefix}reference_{table}`"),
                );
            }
            main.statement(&sql, &[]).await.unwrap();
        }
    }
    for table in TABLES {
        let expected = main
            .getSchemaBuilder()
            .getColumns(&format!("reference_{table}"))
            .await
            .unwrap();
        let mut actual = oauth.getSchemaBuilder().getColumns(table).await.unwrap();
        // 项目明确增加了本地化元数据注释；单独验证后再对照上游的结构契约。
        let labels: serde_json::Value =
            serde_json::from_str(include_str!("../../../../lang/en/schema.json")).unwrap();
        for column in &mut actual {
            let expected_label = labels["columns"][&column.name].as_str().unwrap();
            assert_eq!(
                column.comment.as_deref(),
                Some(expected_label),
                "{table}.{} 的注释",
                column.name
            );
            let reference = expected
                .iter()
                .find(|item| item.name == column.name)
                .unwrap();
            assert!(reference.comment.as_deref().is_none_or(str::is_empty));
            column.comment.clone_from(&reference.comment);
        }
        assert_eq!(actual, expected, "{table}");
        let mut expected = main
            .getSchemaBuilder()
            .getIndexes(&format!("reference_{table}"))
            .await
            .unwrap();
        let mut actual = oauth.getSchemaBuilder().getIndexes(table).await.unwrap();
        for index in expected.iter_mut().chain(actual.iter_mut()) {
            index.name.clear();
        }
        expected.sort_by_key(|v| format!("{v:?}"));
        actual.sort_by_key(|v| format!("{v:?}"));
        assert_eq!(actual, expected, "{table}");
        assert!(
            oauth
                .getSchemaBuilder()
                .getForeignKeys(table)
                .await
                .unwrap()
                .is_empty()
        );
    }
    // Configuration-derived names are owned results and never ambient cross-app state.
    let config_a = first.app().config().unwrap();
    let config_b = second.app().config().unwrap();
    config_a.set("passport.connection", json!([true])).unwrap();
    assert!(
        migrations::oauth_clients::Definition
            .getConnection(config_a.as_ref())
            .is_err()
    );
    assert_eq!(
        migrations::oauth_clients::Definition
            .getConnection(config_b.as_ref())
            .unwrap()
            .as_deref(),
        Some("oauth")
    );
    config_a.set("passport.connection", json!("oauth")).unwrap();
    first.artisan(["migrate:rollback"]).await.assertSuccessful();
    for table in TABLES {
        assert!(!oauth.getSchemaBuilder().hasTable(table).await.unwrap());
        assert!(other.getSchemaBuilder().hasTable(table).await.unwrap());
    }
    first.artisan(["migrate"]).await.assertSuccessful();
    first.artisan(["migrate:status"]).await.assertSuccessful();
    first.artisan(["migrate:rollback"]).await.assertSuccessful();
    second
        .artisan(["migrate:rollback"])
        .await
        .assertSuccessful();
    for table in TABLES {
        main.getSchemaBuilder()
            .drop(&format!("reference_{table}"))
            .await
            .unwrap();
    }
    main.getSchemaBuilder().drop("history").await.unwrap();
    b.connection(None)
        .unwrap()
        .getSchemaBuilder()
        .drop("history")
        .await
        .unwrap();
}
