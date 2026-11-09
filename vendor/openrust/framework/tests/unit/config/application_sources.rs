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

#[allow(
    clippy::too_many_lines,
    reason = "同一 Provider 生命周期需核对文件来源、环境覆盖与 set 后的一致性"
)]
#[tokio::test]
async fn exact_file_sections_keep_shallow_provider_defaults_through_boot_and_set()
-> crate::core::Result<()> {
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::foundation::RegisterContext;
    use crate::support::{BoxFuture, ServiceProvider};
    use repository::SectionSchema;

    struct DefaultsProbe {
        file_source: bool,
    }

    impl ServiceProvider for DefaultsProbe {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                app.config_section(
                    "fixture",
                    serde_json::json!({"label": "base", "nested": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )?;
                let fixture: serde_json::Value = app.config_repository().section("fixture")?;
                assert_eq!(
                    fixture["nested"],
                    if self.file_source {
                        serde_json::json!({"new": 2})
                    } else {
                        serde_json::json!({"old": 1, "new": 2})
                    }
                );
                app.config_section(
                    "cache",
                    serde_json::json!({"stores": {"array": {"driver": "array"}}, "nested": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )
            })
        }

        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                let fixture: serde_json::Value = app.section("fixture")?;
                let expected = if self.file_source {
                    serde_json::json!({"new": 2})
                } else {
                    serde_json::json!({"old": 1, "new": 2})
                };
                assert_eq!(fixture["nested"], expected);
                Ok(())
            })
        }
    }

    struct CacheOnly;
    impl ServiceProvider for CacheOnly {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                app.config_section(
                    "cache",
                    serde_json::json!({"before": "base", "nested": {"old": 1}, "custom": {"old": 1}}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )?;
                assert_eq!(
                    app.config_repository().get("cache.custom")?,
                    Some(serde_json::json!({"old": 1}))
                );
                Ok(())
            })
        }
    }

    let directory = tempfile::tempdir().map_err(crate::core::Error::internal)?;
    std::fs::write(directory.path().join("fixture.toml"), "[nested]\nnew = 2\n")
        .map_err(crate::core::Error::internal)?;
    std::fs::write(
        directory.path().join("cache.toml"),
        "[stores.redis]\ndriver = 'redis'\n[nested]\nnew = 2\n",
    )
    .map_err(crate::core::Error::internal)?;
    let mut loader = ConfigLoader::new(directory.path(), Environment::Test);
    // 此用例在下方显式提供环境层，避免读取其他测试临时设置的进程环境。
    loader.environment = Some(EnvironmentSnapshot::default());
    let mut sources = loader.sources()?;
    let environment = serde_json::json!({
        "cache": {"stores": {"redis": {"dsn": "redis://environment"}}, "optional": null}
    });
    sources.merge(Serialized::defaults(environment.clone()), environment)?;
    let profile = sources.figment().profile().clone();
    let mut builder = ProviderBuilder::new();
    builder.provider(DefaultsProbe { file_source: true });
    let runtime = builder.start(sources).await?;
    let repository = runtime.config();
    assert_eq!(
        repository.get("fixture")?,
        Some(serde_json::json!({"label": "base", "nested": {"new": 2}}))
    );
    assert_eq!(repository.get("fixture.nested.old")?, None);
    assert_eq!(
        repository.get("cache.stores.array.driver")?,
        Some(serde_json::json!("array"))
    );
    assert_eq!(
        repository.get("cache.stores.redis.driver")?,
        Some(serde_json::json!("redis"))
    );
    assert_eq!(
        repository.get("cache.stores.redis.dsn")?,
        Some(serde_json::json!("redis://environment"))
    );
    assert_eq!(
        repository.get("cache.optional")?,
        Some(serde_json::Value::Null)
    );
    assert_eq!(
        repository.get("cache.nested")?,
        Some(serde_json::json!({"new": 2}))
    );
    assert_eq!(repository.snapshot().profile(), &profile);
    assert_eq!(
        repository
            .snapshot()
            .find_metadata("fixture.nested.new")
            .map(|metadata| metadata.name.as_ref()),
        Some("TOML file")
    );
    repository.set("fixture.nested.new", serde_json::json!(4))?;
    assert_eq!(
        repository.get("fixture.nested")?,
        Some(serde_json::json!({"new": 4}))
    );
    assert_eq!(
        repository.section::<serde_json::Value>("fixture")?["nested"],
        serde_json::json!({"new": 4})
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await?;

    let programmatic = ConfigSources::from_ordered_entries([
        ("fixture", serde_json::json!({"nested": {"new": 2}})),
        (
            "cache",
            serde_json::json!({"stores": {"redis": {"driver": "redis"}}, "nested": {"new": 2}}),
        ),
    ])?;
    let mut builder = ProviderBuilder::new();
    builder.provider(DefaultsProbe { file_source: false });
    let runtime = builder.start(programmatic).await?;
    assert_eq!(
        runtime.config().get("fixture.nested")?,
        Some(serde_json::json!({"old": 1, "new": 2}))
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await?;

    let nested = tempfile::tempdir().map_err(crate::core::Error::internal)?;
    std::fs::create_dir(nested.path().join("cache")).map_err(crate::core::Error::internal)?;
    std::fs::write(nested.path().join("cache/custom.toml"), "new = 2\n")
        .map_err(crate::core::Error::internal)?;
    let mut loader = ConfigLoader::new(nested.path(), Environment::Test);
    // 此用例在下方显式提供环境层，避免读取其他测试临时设置的进程环境。
    loader.environment = Some(EnvironmentSnapshot::default());
    let mut child_sources = loader.sources()?;
    let child_environment = serde_json::json!({"cache": {"before": "environment"}});
    child_sources.merge(
        Serialized::defaults(child_environment.clone()),
        child_environment,
    )?;
    let child_profile = child_sources.figment().profile().clone();
    let environment_metadata = child_sources
        .figment()
        .find_metadata("cache.before")
        .map(|metadata| metadata.name.to_string());
    let mut builder = ProviderBuilder::new();
    builder.provider(CacheOnly);
    let runtime = builder.start(child_sources).await?;
    assert_eq!(
        runtime.config().get("cache.nested.old")?,
        Some(serde_json::json!(1))
    );
    assert_eq!(
        runtime.config().get("cache.custom")?,
        Some(serde_json::json!({"old": 1}))
    );
    assert_eq!(
        runtime.config().get("cache.custom.old")?,
        Some(serde_json::json!(1))
    );
    assert_eq!(runtime.config().get("cache.custom.new")?, None);
    assert_eq!(
        runtime.config().get("cache.before")?,
        Some(serde_json::json!("environment"))
    );
    assert_eq!(runtime.config().snapshot().profile(), &child_profile);
    assert_eq!(
        runtime
            .config()
            .snapshot()
            .find_metadata("cache.before")
            .map(|metadata| metadata.name.to_string()),
        environment_metadata
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await?;
    Ok(())
}
