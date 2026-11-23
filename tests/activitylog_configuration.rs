#[cfg(feature = "activitylog")]
#[tokio::test]
async fn native_artisan_discovers_activitylog_publishes_migrates_and_caches_configuration() {
    use openrust::{
        config::Repository,
        database::{DatabaseManager, Value},
    };
    use serde_json::json;
    use std::{collections::BTreeMap, process::Command, sync::Arc};
    let path = std::env::temp_dir().join(format!(
        "openrust-activitylog-runtime-{}",
        std::process::id()
    ));
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
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "vendor/openrust/framework/src/translation/lang/{lang}/schema.json"
            )),
            path.join(format!("lang/{lang}/schema.json")),
        )
        .unwrap();
    }
    let driver = std::env::var("ACTIVITYLOG_TEST_DRIVER").unwrap_or("sqlite".into());
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
            .env(
                "APP_DEBUG",
                if stale_environment {
                    "invalid-after-cache"
                } else {
                    "false"
                },
            );
        c.env(
            "ACTIVITYLOG_ENABLED",
            if stale_environment {
                "invalid-after-cache"
            } else {
                "true"
            },
        )
        .env(
            "ACTIVITYLOG_BUFFER_ENABLED",
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
        &[
            "vendor:publish",
            "--tag=activitylog-config",
            "--no-interaction",
        ],
        false,
    );
    let published = path.join("config/activitylog.rs");
    let template = include_str!("../vendor/openrust/activitylog/config/activitylog.rs");
    assert_eq!(std::fs::read_to_string(&published).unwrap(), template);
    std::fs::write(
        &published,
        format!("{template}\n// application customization\n"),
    )
    .unwrap();
    run(
        &[
            "vendor:publish",
            "--tag=activitylog-config",
            "--no-interaction",
        ],
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
            "--tag=activitylog-config",
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
            .has_table("activity_log")
            .await
            .unwrap()
    );
    let columns = connection
        .get_schema_builder()
        .get_columns("activity_log")
        .await
        .unwrap();
    for name in ["subject_id", "causer_id"] {
        let column = columns.iter().find(|column| column.name == name).unwrap();
        assert!(
            column.type_name.contains("int"),
            "{driver} {name}: {}",
            column.type_name
        );
        assert!(column.nullable);
    }
    for name in ["properties", "attribute_changes"] {
        let column = columns.iter().find(|column| column.name == name).unwrap();
        assert!(
            matches!(column.type_name.as_str(), "text" | "json"),
            "{driver} {name}: {}",
            column.type_name
        );
        assert!(column.nullable);
    }
    for name in ["created_at", "updated_at"] {
        let column = columns.iter().find(|column| column.name == name).unwrap();
        assert!(
            column.type_name.contains("timestamp") || column.type_name.contains("datetime"),
            "{driver} {name}: {}",
            column.type_name
        );
    }
    let logger = openrust_activitylog::ActivityRepository::new(connection.clone());
    let record = logger
        .log(
            openrust_activitylog::Activity::new("native.probe")
                .performedOn(openrust_activitylog::Subject::new("account", "1").unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        logger.query().get().await.unwrap()[0]
            .subject
            .as_ref()
            .unwrap()
            .id,
        "1"
    );
    run(&["migrate", "--force"], false);
    let persisted_id = connection.table("activity_log").value("id").await.unwrap();
    let persisted_id = match persisted_id {
        Value::Integer(id) => id,
        Value::Unsigned(id) => i64::try_from(id).unwrap(),
        value => panic!("unexpected auto-increment ID: {value:?}"),
    };
    assert_eq!(persisted_id, record.id.unwrap());
    let status = run(&["migrate:status"], false);
    assert!(String::from_utf8_lossy(&status.stdout).contains("create_activity_log_table"));
    run(&["config:cache"], false);
    let cached = run(&["config:show", "activitylog"], true);
    assert!(String::from_utf8_lossy(&cached.stdout).contains("clean_after_days"));
    run(&["activitylog:clean", "--days=365"], true);
    assert_eq!(
        connection.table("activity_log").count("*").await.unwrap(),
        1
    );
    run(&["migrate:status"], true);
    run(&["config:clear"], true);
    run(&["migrate:rollback", "--force"], false);
    assert!(
        !connection
            .get_schema_builder()
            .has_table("activity_log")
            .await
            .unwrap()
    );
    manager.disconnect(None).unwrap();
    println!(
        "{driver}: native Artisan discovery, publication, migration, data retention, cached configuration, cleanup and rollback passed"
    );
}
