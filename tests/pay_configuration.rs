#[cfg(feature = "pay")]
#[path = "../config/mod.rs"]
mod config;
#[cfg(feature = "pay")]
#[test]
fn payment_configuration_maps_merchant_credentials_without_reading_key_files() {
    let path = std::env::temp_dir().join(format!("openrust-pay-config-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let mut dotenv = String::from("PAY_DRIVER=alipay\n");
    for (name, value) in [
        ("WECHAT_APP_ID", "wx-app"),
        ("WECHAT_MCH_ID", "merchant"),
        ("WECHAT_SERIAL_NO", "serial"),
        ("WECHAT_PRIVATE_KEY_PATH", "/missing/merchant.pem"),
        ("WECHAT_API_V3_KEY", "secret"),
        ("WECHAT_PUBLIC_KEY_ID", "PUB_KEY_ID_TEST"),
        ("WECHAT_PUBLIC_KEY_PATH", "/missing/platform.pem"),
        ("ALIPAY_APP_ID", "ali-app"),
        ("ALIPAY_SELLER_ID", "seller"),
        ("ALIPAY_PRIVATE_KEY_PATH", "/missing/alipay.pem"),
        ("ALIPAY_PUBLIC_KEY_PATH", "/missing/ali-platform.pem"),
    ] {
        dotenv.push_str(&format!("PAY_{name}={value}\n"));
    }
    std::fs::write(path.join(".env"), dotenv).unwrap();
    let env =
        openrust::foundation::bootstrap::LoadEnvironmentVariables::bootstrap(&path, None).unwrap();
    let config = config::load(&path, &env).unwrap();
    let pay = &config["pay"];
    assert_eq!(pay["default"], "alipay");
    assert_eq!(pay["drivers"]["wechat"]["mch_id"], "merchant");
    assert_eq!(pay["drivers"]["wechat"]["app_id"], "wx-app");
    assert_eq!(pay["drivers"]["wechat"]["api_v3_key"], "secret");
    assert_eq!(pay["drivers"]["wechat"]["public_key_id"], "PUB_KEY_ID_TEST");
    assert_eq!(pay["drivers"]["alipay"]["seller_id"], "seller");
    assert_eq!(
        pay["drivers"]["alipay"]["private_key_path"],
        "/missing/alipay.pem"
    );
}

#[cfg(feature = "pay")]
#[tokio::test]
async fn native_artisan_registers_payment_migration_without_merchant_keys_and_preserves_rows() {
    use openrust::{
        config::Repository,
        database::{DatabaseManager, Value},
    };
    use serde_json::json;
    use std::{collections::BTreeMap, process::Command, sync::Arc};
    let path = std::env::temp_dir().join(format!("openrust-pay-runtime-{}", std::process::id()));
    std::fs::create_dir_all(path.join("database")).unwrap();
    std::fs::File::create(path.join("database/database.sqlite")).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    for lang in ["en", "zh_CN", "zh_TW"] {
        std::fs::create_dir_all(path.join(format!("lang/{lang}"))).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("lang/{lang}/schema.json")),
            path.join(format!("lang/{lang}/schema.json")),
        )
        .unwrap();
    }
    let driver = std::env::var("PAY_TEST_DRIVER").unwrap_or("sqlite".into());
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
    let run = |args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_main"));
        c.current_dir(&path)
            .env("APP_BASE_PATH", &path)
            .env("APP_ENV", "testing")
            .env("APP_LOCALE", "en")
            .env("APP_FALLBACK_LOCALE", "en")
            .env("DB_CONNECTION", &driver)
            .env("DB_DATABASE", path.join("database/database.sqlite"))
            .env("LOG_CHANNEL", "null")
            .env("PAY_WECHAT_PRIVATE_KEY_PATH", "/missing/not-read.pem")
            .env("PAY_ALIPAY_PRIVATE_KEY_PATH", "/missing/not-read.pem");
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
    run(&["migrate", "--force"]);
    assert!(
        connection
            .getSchemaBuilder()
            .hasTable("payment_notifications")
            .await
            .unwrap()
    );
    connection
        .table("payment_notifications")
        .insert(&[[
            ("id".into(), "a".repeat(64).into()),
            ("fingerprint".into(), "b".repeat(64).into()),
            ("status".into(), "done".into()),
        ]
        .into_iter()
        .collect()])
        .await
        .unwrap();
    run(&["migrate", "--force"]);
    assert_eq!(
        connection
            .table("payment_notifications")
            .value("status")
            .await
            .unwrap(),
        Value::from("done")
    );
    let status = run(&["migrate:status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("create_payment_notifications_table"));
    run(&["migrate:rollback", "--force"]);
    assert!(
        !connection
            .getSchemaBuilder()
            .hasTable("payment_notifications")
            .await
            .unwrap()
    );
    manager.disconnect(None).unwrap();
    println!(
        "{driver}: native Artisan feature registration, migrate, data retention, status, rollback and lazy key loading passed"
    );
}
