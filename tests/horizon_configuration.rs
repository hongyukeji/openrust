#[cfg(feature = "horizon")]
#[test]
fn native_artisan_discovers_horizon_and_preserves_published_and_cached_configuration() {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!(
        "openrust-horizon-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    for language in ["en", "zh-Hans", "zh-Hant"] {
        let path = root.as_path().join(format!("lang/{language}"));
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
        let output = Command::new(env!("CARGO_BIN_EXE_main"))
            .current_dir(root.as_path())
            .env("APP_BASE_PATH", root.as_path())
            .env("APP_ENV", "testing")
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
            .env("LOG_CHANNEL", "null")
            .env(
                "HORIZON_PREFIX",
                if cached {
                    "changed:"
                } else {
                    "cached_horizon:"
                },
            )
            .env(
                "HORIZON_PATH",
                if cached {
                    "changed-path"
                } else {
                    "operations/queue"
                },
            )
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
    run(&["help", "horizon"], false);
    run(&["help", "horizon:retry"], false);
    run(
        &["vendor:publish", "--tag=horizon-config", "--no-interaction"],
        false,
    );
    let path = root.as_path().join("config/horizon.rs");
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("balanceCooldown")
    );
    std::fs::write(&path, "// application customization").unwrap();
    run(
        &["vendor:publish", "--tag=horizon-config", "--no-interaction"],
        false,
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "// application customization"
    );
    run(
        &[
            "vendor:publish",
            "--tag=horizon-config",
            "--force",
            "--no-interaction",
        ],
        false,
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("balanceCooldown")
    );
    run(&["config:cache"], false);
    run(&["help", "horizon"], true);
    let cached = run(&["config:show", "horizon"], true);
    let cached = String::from_utf8(cached.stdout).unwrap();
    assert!(cached.contains("cached_horizon:"), "{cached}");
    assert!(cached.contains("operations/queue"), "{cached}");
    assert!(!cached.contains("changed-path"), "{cached}");
    run(&["config:clear"], true);
}
