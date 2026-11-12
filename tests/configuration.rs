use std::{fs, path::Path};

use openrust::{
    config::{ConfigError, Value},
    foundation::bootstrap::LoadEnvironmentVariables,
};
use serde_json::json;

#[path = "../config/mod.rs"]
mod config;

#[test]
fn application_configuration_loads_all_sections_and_typed_overrides() {
    let root = std::env::temp_dir().join(format!("openrust-config-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup<'a>(&'a Path);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0);
        }
    }
    let _cleanup = Cleanup(&root);

    let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
    let defaults = config::load(&root, &environment).unwrap();
    assert_eq!(
        defaults.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "app",
            "auth",
            "broadcasting",
            "cache",
            "concurrency",
            "cors",
            "database",
            "filesystems",
            "grpc",
            "hashing",
            "images",
            "logging",
            "mail",
            "queue",
            "reverb",
            "runtime",
            "services",
            "session",
            "view",
        ]
    );
    assert_eq!(defaults["auth"]["defaults"]["guard"], "web");
    assert_eq!(
        defaults["cache"]["stores"]["file"]["path"],
        json!(root.join("storage/framework/cache/data"))
    );
    assert_eq!(
        defaults["database"]["connections"]["sqlite"]["database"],
        json!(root.join("database/database.sqlite"))
    );
    assert_eq!(
        defaults["queue"]["batching"]["database"],
        defaults["database"]["default"]
    );
    assert_eq!(defaults["filesystems"]["default"], "local");
    assert_eq!(
        defaults["view"]["paths"],
        json!([root.join("resources/views")])
    );
    assert_eq!(defaults["reverb"]["enabled"], false);
    assert_eq!(
        defaults["runtime"],
        json!({"enabled":true,"http":true,"queue":true,"schedule":true})
    );
    assert_eq!(defaults["grpc"]["clients"], json!({}));

    for (contents, default_db, cache_db) in [
        ("", 0, 0),
        ("REDIS_DB=3\n", 3, 3),
        ("REDIS_CACHE_DB=1\n", 0, 1),
        ("REDIS_DB=3\nREDIS_CACHE_DB=0\n", 3, 0),
    ] {
        fs::write(root.join(".env"), contents).unwrap();
        let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
        let configured = config::load(&root, &environment).unwrap();
        assert_eq!(
            configured["database"]["redis"]["default"]["database"],
            default_db
        );
        assert_eq!(
            configured["database"]["redis"]["cache"]["database"],
            cache_db
        );
    }

    fs::write(
        root.join(".env"),
        "APP_NAME=ExampleStore\nAPP_PORT=9081\nAPP_DEBUG=true\nDB_CONNECTION=pgsql\nDB_PREFIX=app_\nSESSION_LIFETIME=90\nSESSION_SECURE_COOKIE=false\nSESSION_SAME_SITE=null\nLOG_STACK=single,stderr\nREVERB_SCHEME=http\nREDIS_PASSWORD=null\nAPP_PUBLIC_PATH=site\nPUBLIC_STORAGE_ROOT=uploads\nRUNTIME_ENABLED=false\nRUNTIME_HTTP=false\nRUNTIME_QUEUE=false\nRUNTIME_SCHEDULE=false\n",
    )
    .unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
    let configured = config::load(&root, &environment).unwrap();
    for connection in ["sqlite", "mysql", "mariadb", "pgsql", "sqlsrv"] {
        assert_eq!(
            defaults["database"]["connections"][connection]["prefix"],
            ""
        );
        assert_eq!(
            configured["database"]["connections"][connection]["prefix"],
            "app_"
        );
    }
    assert_eq!(configured["app"]["debug"], true);
    assert_eq!(
        configured["runtime"],
        json!({"enabled":false,"http":false,"queue":false,"schedule":false})
    );
    assert_eq!(configured["app"]["url"], "http://localhost:9081");
    assert_eq!(configured["cache"]["prefix"], "examplestore-cache-");
    assert_eq!(
        configured["filesystems"]["disks"]["public"]["url"],
        "http://localhost:9081/storage"
    );
    assert_eq!(
        configured["filesystems"]["links"][root.join("site").join("storage").to_str().unwrap()],
        json!(root.join("uploads"))
    );
    assert_eq!(
        configured["mail"]["mailers"]["smtp"]["local_domain"],
        "localhost"
    );
    assert_eq!(configured["session"]["lifetime"], 90);
    assert_eq!(configured["session"]["secure"], false);
    assert_eq!(configured["session"]["same_site"], Value::Null);
    assert_eq!(configured["session"]["cookie"], "example_store_session");
    assert_eq!(
        configured["logging"]["channels"]["stack"]["channels"],
        json!(["single", "stderr"])
    );
    assert_eq!(
        configured["database"]["redis"]["default"]["password"],
        Value::Null
    );
    assert_eq!(configured["queue"]["failed"]["database"], "pgsql");
    assert_eq!(
        configured["broadcasting"]["connections"]["reverb"]["options"]["useTLS"],
        false
    );
    assert_eq!(
        configured["broadcasting"]["connections"]["reverb"]["options"],
        configured["reverb"]["apps"]["apps"][0]["options"]
    );

    fs::write(root.join(".env"), "SESSION_LIFETIME=invalid\n").unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
    assert!(matches!(
        config::load(&root, &environment),
        Err(ConfigError::InvalidType { key, .. }) if key == "SESSION_LIFETIME"
    ));
}
