//! 核对应用语言与既有文件、数据库及环境来源共用配置优先级。

// figment::Jail 的闭包按其契约返回较大的错误类型。
#![allow(clippy::result_large_err)]

use super::*;

#[test]
fn ordered_sources_keep_file_namespace_and_environment_precedence() {
    let _environment_guard = crate::lock_test_env();
    figment::Jail::expect_with(|jail| {
        jail.create_dir("config")?;
        jail.create_file("config/part10.toml", "last = 10\nshared = 10\n")?;
        jail.create_file(
            "config/part2.toml",
            "z = 1\na = 2\nshared = 2\nitems = [9, 8]\n",
        )?;
        jail.set_env("APP_PART2_A", "3");
        let sources = ConfigLoader::new("config", Environment::Test)
            .sources()
            .map_err(|error| figment::Error::from(error.to_string()))?;
        let repository = repository::ConfigRepository::new(sources.clone());
        let raw = repository
            .all()
            .map_err(|error| figment::Error::from(error.to_string()))?;
        assert_eq!(
            raw.as_object().map(|items| items
                .keys()
                .take(4)
                .map(String::as_str)
                .collect::<Vec<_>>()),
            Some(vec!["app", "server", "api", "log"])
        );
        assert!(raw.get("modules").is_none());
        assert!(raw.get("i18n").is_none());
        let ordered = repository
            .get("part2")
            .map_err(|error| figment::Error::from(error.to_string()))?
            .ok_or_else(|| figment::Error::from("ordered section missing"))?;
        assert_eq!(
            ordered
                .as_object()
                .map(|items| items.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["z", "a", "shared", "items"])
        );
        assert_eq!(ordered["a"], 3);
        assert_eq!(ordered["shared"], 2);
        assert_eq!(ordered["items"], serde_json::json!([9, 8]));
        assert_eq!(
            repository
                .get("part10.shared")
                .map_err(|error| figment::Error::from(error.to_string()))?,
            Some(serde_json::json!(10))
        );
        assert_eq!(sources.figment().extract_inner::<u32>("part2.a")?, 3);
        assert_eq!(
            sources
                .figment()
                .find_metadata("part2.z")
                .map(|metadata| metadata.name.as_ref()),
            Some("TOML file")
        );
        Ok(())
    });
}

#[test]
fn laravel_style_database_and_redis_variables_build_connection_urls() {
    let _environment_guard = crate::lock_test_env();
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env("DB_CONNECTION", "mysql");
        jail.set_env("DB_HOST", "db.example.test");
        jail.set_env("DB_DATABASE", "shop");
        jail.set_env("DB_USERNAME", "app user");
        jail.set_env("DB_PASSWORD", "p@ss:word");
        jail.set_env("REDIS_HOST", "cache.example.test");
        jail.set_env("REDIS_PASSWORD", "redis:secret");
        jail.set_env("REDIS_PORT", "6380");
        jail.set_env("REDIS_DB", "4");
        jail.set_env("REDIS_CACHE_DB", "9");
        jail.set_env("DB_TABLE_PREFIX", "shop_");
        jail.set_env("CACHE_STORE", "redis");
        jail.set_env("MAIL_DRIVER", "smtp");
        jail.set_env("MAIL_SMTP_HOST", "mail.example.test");
        jail.set_env("SMS_DRIVER", "log");
        jail.set_env("APP_LOCALE", "zh-cn");
        jail.set_env("APP_FALLBACK_LOCALE", "en-GB");
        #[cfg(feature = "queue")]
        jail.set_env("QUEUE_CONNECTION", "redis");

        jail.create_dir("empty-config")?;
        let loader = ConfigLoader::new("empty-config", Environment::Test);
        let database: serde_json::Value = loader
            .figment()
            .map_err(|error| figment::Error::from(error.to_string()))?
            .extract_inner("database")?;
        let database_url = url::Url::parse(database["url"].as_str().unwrap_or_default())
            .map_err(|error| figment::Error::from(error.to_string()))?;
        assert_eq!(database_url.scheme(), "mysql");
        assert_eq!(database_url.host_str(), Some("db.example.test"));
        assert_eq!(database_url.path(), "/shop");
        assert_eq!(database_url.username(), "app%20user");
        assert_eq!(database_url.password(), Some("p%40ss%3Aword"));
        assert_eq!(database["table_prefix"], "shop_");

        let cache: serde_json::Value = loader
            .figment()
            .map_err(|error| figment::Error::from(error.to_string()))?
            .extract_inner("cache")?;
        let redis_url =
            url::Url::parse(cache["stores"]["redis"]["dsn"].as_str().unwrap_or_default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
        assert_eq!(redis_url.host_str(), Some("cache.example.test"));
        assert_eq!(redis_url.port(), Some(6380));
        assert_eq!(redis_url.path(), "/4");
        assert_eq!(redis_url.password(), Some("redis%3Asecret"));
        assert_eq!(cache["default"], "redis");
        let mail: serde_json::Value = loader
            .figment()
            .map_err(|error| figment::Error::from(error.to_string()))?
            .extract_inner("mail")?;
        assert_eq!(mail["driver"], "smtp");
        assert_eq!(mail["smtp_host"], "mail.example.test");
        let sms: serde_json::Value = loader
            .figment()
            .map_err(|error| figment::Error::from(error.to_string()))?
            .extract_inner("sms")?;
        assert_eq!(sms["driver"], "log");
        let app: AppConfig = loader
            .figment()
            .map_err(|error| figment::Error::from(error.to_string()))?
            .extract_inner("app")?;
        assert_eq!(app.locale.as_str(), "zh-CN");
        assert_eq!(app.fallback_locale.as_str(), "en-GB");
        #[cfg(feature = "queue")]
        {
            let queue: serde_json::Value = loader
                .figment()
                .map_err(|error| figment::Error::from(error.to_string()))?
                .extract_inner("queue")?;
            assert_eq!(queue["default"], "redis");
            let queue_url = url::Url::parse(
                queue["connections"]["redis"]["redis_url"]
                    .as_str()
                    .unwrap_or_default(),
            )
            .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(queue_url.host_str(), Some("cache.example.test"));
            assert_eq!(queue_url.path(), "/4");
            assert_eq!(queue_url.password(), Some("redis%3Asecret"));
        }
        Ok(())
    });
}
