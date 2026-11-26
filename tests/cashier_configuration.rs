#[cfg(feature = "cashier")]
#[tokio::test]
async fn native_artisan_registers_billing_migration_without_api_keys_and_preserves_rows() {
    use openrust::{
        config::Repository,
        database::{DatabaseManager, Value},
    };
    use serde_json::json;
    use std::{collections::BTreeMap, process::Command, sync::Arc};
    let path =
        std::env::temp_dir().join(format!("openrust-cashier-runtime-{}", std::process::id()));
    std::fs::create_dir_all(path.join("database")).unwrap();
    std::fs::File::create(path.join("database/database.sqlite")).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    for lang in ["en", "zh-Hans", "zh-Hant"] {
        std::fs::create_dir_all(path.join(format!("lang/{lang}"))).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("lang/{lang}/schema.json")),
            path.join(format!("lang/{lang}/schema.json")),
        )
        .unwrap();
    }
    let driver = std::env::var("CASHIER_TEST_DRIVER").unwrap_or("sqlite".into());
    let url = match driver.as_str() {
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            Some(std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap())
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            Some(std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap())
        }
        "sqlite" => None,
        _ => panic!("unsupported driver"),
    };
    let db_config = if let Some(url) = &url {
        json!({"driver":driver,"url":url})
    } else {
        json!({"driver":"sqlite","database":path.join("database/database.sqlite")})
    };
    let manager = DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":db_config}}),
        )]))),
        &path,
    );
    let connection = manager.connection(None).unwrap();
    let run = |args: &[&str], stale_environment: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_main"));
        c.current_dir(&path)
            .env("APP_BASE_PATH", &path)
            .env("APP_ENV", "testing")
            .env("APP_LOCALE", "en")
            .env("APP_FALLBACK_LOCALE", "en")
            .env("DB_CONNECTION", &driver)
            .env("DB_DATABASE", path.join("database/database.sqlite"))
            .env("LOG_CHANNEL", "null")
            .env("STRIPE_SECRET", "")
            .env("PADDLE_API_KEY", "")
            .env(
                "CASHIER_LIVE",
                if stale_environment {
                    "invalid-after-cache"
                } else {
                    "false"
                },
            );
        if let Some(url) = &url {
            c.env("DB_URL", url);
        } else {
            c.env_remove("DB_URL");
        }
        let out = c.arg("artisan").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    run(
        &["vendor:publish", "--tag=cashier-config", "--no-interaction"],
        false,
    );
    let published = path.join("config/cashier.rs");
    let template = include_str!("../vendor/openrust-cashier/config/cashier.rs");
    assert_eq!(std::fs::read_to_string(&published).unwrap(), template);
    std::fs::write(
        &published,
        format!("{template}\n// application customization\n"),
    )
    .unwrap();
    run(
        &["vendor:publish", "--tag=cashier-config", "--no-interaction"],
        false,
    );
    assert!(
        std::fs::read_to_string(&published)
            .unwrap()
            .contains("application customization")
    );
    run(
        &[
            "vendor:publish",
            "--tag=cashier-config",
            "--force",
            "--no-interaction",
        ],
        false,
    );
    assert_eq!(std::fs::read_to_string(&published).unwrap(), template);
    run(&["migrate", "--force"], false);
    assert!(
        connection
            .get_schema_builder()
            .has_table("cashier_events")
            .await
            .unwrap()
    );
    connection
        .table("cashier_events")
        .insert(&[[
            ("id".into(), "a".repeat(64).into()),
            ("fingerprint".into(), "b".repeat(64).into()),
            ("status".into(), "done".into()),
        ]
        .into_iter()
        .collect()])
        .await
        .unwrap();
    run(&["migrate", "--force"], false);
    assert_eq!(
        connection
            .table("cashier_events")
            .value("status")
            .await
            .unwrap(),
        Value::from("done")
    );
    let status = run(&["migrate:status"], false);
    assert!(String::from_utf8_lossy(&status.stdout).contains("create_cashier_tables"));
    run(&["config:cache"], false);
    run(&["migrate:status"], true);
    run(&["config:clear"], true);
    run(&["migrate:rollback", "--force"], false);
    assert!(
        !connection
            .get_schema_builder()
            .has_table("cashier_events")
            .await
            .unwrap()
    );
    manager.disconnect(None).unwrap();
    println!(
        "{driver}: native Artisan feature registration, migrate, data retention, status, rollback and lazy key loading passed"
    );
}
