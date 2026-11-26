#[cfg(feature = "pulse")]
#[test]
fn native_artisan_discovers_pulse_and_preserves_published_and_cached_configuration() {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!(
        "openrust-pulse-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    std::fs::create_dir_all(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    std::fs::create_dir_all(root.join("database")).unwrap();
    std::fs::write(root.join("database/pulse.sqlite"), []).unwrap();
    let driver = std::env::var("PULSE_TEST_DRIVER").unwrap_or("sqlite".into());
    let url = match driver.as_str() {
        "sqlite" => None,
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            Some(std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap())
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            Some(std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap())
        }
        _ => panic!("unsupported isolated driver"),
    };
    for language in ["en", "zh-Hans", "zh-Hant"] {
        let path = root.join(format!("lang/{language}"));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "vendor/openrust-framework/src/localization/lang/{language}/schema.json"
            )),
            path.join("schema.json"),
        )
        .unwrap();
    }
    let run = |args: &[&str], cached: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command.env_remove("DB_URL");
        if let Some(url) = &url {
            command.env("DB_URL", url);
        }
        let output = command
            .current_dir(&root)
            .env("APP_BASE_PATH", &root)
            .env("APP_ENV", "testing")
            .env("DB_CONNECTION", &driver)
            .env("DB_DATABASE", root.join("database/pulse.sqlite"))
            .env("APP_LOCALE", "en")
            .env("APP_FALLBACK_LOCALE", "en")
            .env(
                "APP_DEBUG",
                if cached {
                    "invalid-after-cache"
                } else {
                    "false"
                },
            )
            .env(
                "PULSE_ENABLED",
                if cached {
                    "invalid-after-cache"
                } else {
                    "true"
                },
            )
            .env("LOG_CHANNEL", "null")
            .env("CACHE_STORE", "file")
            .arg("artisan")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run(&["help", "pulse:check"], false);
    run(&["migrate", "--force", "--no-interaction"], false);
    run(&["pulse:work", "--stop-when-empty"], false);
    run(&["pulse:clear", "--force"], false);
    run(
        &["vendor:publish", "--tag=pulse-config", "--no-interaction"],
        false,
    );
    let config = root.join("config/pulse.rs");
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("PULSE_ENABLED")
    );
    std::fs::write(&config, "// application customization").unwrap();
    run(
        &["vendor:publish", "--tag=pulse-config", "--no-interaction"],
        false,
    );
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "// application customization"
    );
    run(&["config:cache"], false);
    run(&["pulse:clear", "--force"], true);
    run(&["config:clear"], true);
    run(&["help", "pulse:purge"], false);
}

#[cfg(not(feature = "pulse"))]
#[test]
fn native_artisan_omits_pulse_when_the_feature_is_disabled() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_main"))
        .args(["artisan", "help", "pulse:purge"])
        .env("APP_ENV", "testing")
        .env("LOG_CHANNEL", "null")
        .output()
        .unwrap();
    assert!(!result.status.success());
}
