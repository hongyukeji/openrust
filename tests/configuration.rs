use std::{fs, path::Path};

use openrust::contracts::config::Repository as _;
use openrust::{
    config::{ConfigError, Value},
    foundation::bootstrap::{LoadConfiguration, LoadEnvironmentVariables},
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
    assert_eq!(defaults["app"]["name"], env!("CARGO_PKG_NAME"));
    assert_eq!(defaults["app"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        defaults
            .keys()
            .map(String::as_str)
            .filter(|key| !matches!(*key, "api" | "sanctum"))
            .collect::<Vec<_>>(),
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
            "services",
            "session",
            "sms",
            "view",
            "vonage",
        ]
    );
    assert_eq!(defaults["sms"]["default"], "log");
    assert_eq!(defaults["sms"]["drivers"]["log"]["driver"], "log");
    assert_eq!(defaults["sms"]["drivers"]["aliyun"]["market"], "auto");
    assert!(defaults["sms"]["drivers"]["aliyun"]["access_key_id"].is_null());
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
    assert_eq!(defaults["app"]["disabled_runners"], json!([]));
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
        "APP_NAME=ExampleStore\nAPP_PORT=9081\nAPP_DEBUG=true\nDB_CONNECTION=pgsql\nDB_PREFIX=app_\nSESSION_LIFETIME=90\nSESSION_SECURE_COOKIE=false\nSESSION_SAME_SITE=null\nLOG_STACK=single,stderr\nREVERB_SCHEME=http\nREDIS_PASSWORD=null\nAPP_PUBLIC_PATH=site\nFILESYSTEM_PUBLIC_ROOT=uploads\nAWS_TEMPORARY_URL=https://downloads.example.test\nAPP_DISABLED_RUNNERS=\"queue, schedul?, ,app:*\"\nSANCTUM_STATEFUL_DOMAINS=localhost:3000,spa.example:8443\nSMS_DRIVER=tencent\nSMS_TENCENT_MARKET=international\nSMS_TENCENT_REGION=ap-hongkong\nSMS_ALIYUN_MARKET=international\nSMS_ALIYUN_ACCESS_KEY_ID=global-id\n",
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
    assert_eq!(
        defaults["filesystems"]["disks"]["web"]["root"],
        json!(root.join("public"))
    );
    assert_eq!(
        configured["filesystems"]["disks"]["web"]["root"],
        json!(root.join("site"))
    );
    assert_eq!(defaults["app"]["public_path"], "public");
    assert_eq!(configured["app"]["public_path"], "site");
    assert_eq!(
        defaults["filesystems"]["disks"]["web"]["url"],
        defaults["app"]["url"]
    );
    assert_eq!(
        configured["filesystems"]["disks"]["web"]["url"],
        configured["app"]["url"]
    );
    assert!(configured["app"].get("public_url_prefix").is_none());
    assert_eq!(configured["app"]["name"], "ExampleStore");
    assert_eq!(configured["app"]["debug"], true);
    let package_defaults =
        LoadConfiguration::bootstrap(&root, &environment, |_, _| Ok(Default::default())).unwrap();
    #[cfg(feature = "sanctum")]
    {
        assert_eq!(
            package_defaults.get("sanctum.stateful"),
            Some(json!("localhost:3000,spa.example:8443"))
        );
        let package_override = LoadConfiguration::bootstrap(&root, &environment, |_, _| {
            Ok([("sanctum".into(), json!({"token_prefix": "app_"}))].into())
        })
        .unwrap();
        assert_eq!(
            package_override.get("sanctum.token_prefix"),
            Some(json!("app_"))
        );
        assert_eq!(
            package_override.get("sanctum.stateful"),
            package_defaults.get("sanctum.stateful")
        );
    }
    #[cfg(not(feature = "sanctum"))]
    assert_eq!(package_defaults.get("sanctum"), None);
    assert_eq!(
        configured["app"]["disabled_runners"],
        json!(["queue", "schedul?", "app:*"])
    );
    assert_eq!(configured["app"]["url"], "http://localhost:9081");
    assert_eq!(
        configured["filesystems"]["disks"]["s3"]["temporary_url"],
        "https://downloads.example.test"
    );
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
    assert_eq!(configured["sms"]["default"], "tencent");
    assert_eq!(
        configured["sms"]["drivers"]["aliyun"]["access_key_id"],
        "global-id"
    );
    assert_eq!(
        configured["sms"]["drivers"]["aliyun"]["market"],
        "international"
    );
    assert_eq!(
        configured["sms"]["drivers"]["tencent"]["market"],
        "international"
    );
    assert_eq!(
        configured["sms"]["drivers"]["tencent"]["region"],
        "ap-hongkong"
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

    fs::write(root.join(".env"),
        "APP_URL=https://example.test\nFILESYSTEM_WEB_URL=https://example.test/assets\nFILESYSTEM_PUBLIC_ROOT=uploads\nFILESYSTEM_PUBLIC_URL=https://example.test/media\n"
    ).unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
    let overridden = config::load(&root, &environment).unwrap();
    assert_eq!(
        overridden["filesystems"]["disks"]["web"]["url"],
        "https://example.test/assets"
    );
    assert_eq!(
        overridden["filesystems"]["disks"]["public"]["url"],
        "https://example.test/media"
    );
    assert_eq!(
        overridden["filesystems"]["disks"]["public"]["root"],
        json!(root.join("uploads"))
    );

    for value in ["", "   "] {
        fs::write(root.join(".env"), format!("APP_PUBLIC_PATH=\"{value}\"\n")).unwrap();
        let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
        assert!(matches!(
            config::load(&root, &environment),
            Err(ConfigError::InvalidType { key, .. }) if key == "APP_PUBLIC_PATH"
        ));
    }

    fs::write(root.join(".env"), "SESSION_LIFETIME=invalid\n").unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root, None).unwrap();
    assert!(matches!(
        config::load(&root, &environment),
        Err(ConfigError::InvalidType { key, .. }) if key == "SESSION_LIFETIME"
    ));
}
