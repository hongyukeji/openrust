use figment::{Figment, providers::Serialized};
use openrust::config::repository::ConfigRepository;
use openrust::config::{ConfigLoader, ConfigManifest, Environment, NoConfig};
use openrust::foundation::ApplicationDefinition;
use openrust::support::ServiceProvider;
use openrust::testing::TestApp;
use serde_json::json;
use std::collections::BTreeMap;

struct LateReader(&'static str);

impl openrust::support::ServiceProvider for LateReader {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(self.0)).await?;
            Ok(())
        })
    }
}

#[test]
fn external_consumer_registers_and_forces_provider_on_same_application()
-> Result<(), Box<dyn std::error::Error>> {
    openrust::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let source = Figment::from(Serialized::defaults(
                openrust::config::FrameworkConfig::default(),
            ))
            .merge(Serialized::defaults(json!({"app": {"name": "外部读取者"}})));
            let mut builder = openrust::foundation::ProviderBuilder::new();
            builder.provider(config_repository_reading_consumer::ReadingProvider);
            let mut app =
                openrust::Application::from_provider_runtime(builder.start(source).await?)?;
            assert_eq!(app.environment().as_str(), "production");
            assert!(app.environment_matches(["pro*"]));
            assert!(app.is_production());
            assert!(!app.running_unit_tests());
            assert_eq!(app.section::<String>("app.name")?, "外部读取者");
            let first = app.register_provider(LateReader("初次登记")).await?;
            let old = app.state().resolve::<&'static str>().await?;
            assert_eq!(*old, "初次登记");
            assert_eq!(app.register_provider(LateReader("忽略重复")).await?, first);
            assert_eq!(*app.state().resolve::<&'static str>().await?, "初次登记");
            assert_eq!(
                app.force_register_provider(LateReader("重新登记")).await?,
                first
            );
            assert_eq!(*old, "初次登记");
            assert_eq!(*app.state().resolve::<&'static str>().await?, "重新登记");
            app.shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await?;
            Ok::<(), Box<dyn std::error::Error>>(())
        })
}

fn repository(value: serde_json::Value) -> ConfigRepository {
    ConfigRepository::new(Figment::from(Serialized::defaults(value)))
}

#[test]
fn public_typed_reader_is_live_and_rejects_wrong_type() -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository(json!({"app": {"name": "原名称"}}));
    assert_eq!(
        config_repository_reading_consumer::public_display_name(&repository)?,
        "原名称"
    );
    repository.set("app.name", json!("新名称"))?;
    assert_eq!(
        config_repository_reading_consumer::public_display_name(&repository)?,
        "新名称"
    );
    repository.set("app.name", json!(7))?;
    let error = config_repository_reading_consumer::public_display_name(&repository)
        .expect_err("名称不是字符串时必须报错");
    assert!(
        error
            .to_string()
            .contains("app.name: expected string, got integer")
    );
    Ok(())
}

#[test]
fn public_reader_distinguishes_missing_null_and_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = repository(json!({
        "app": {"name": "读书会", "locale": null, "enabled": false, "count": 0, "empty": ""}
    }));
    assert_eq!(
        config_repository_reading_consumer::public_display_name(&repository)?,
        "读书会"
    );
    assert_eq!(repository.get("app.absent")?, None);
    assert_eq!(repository.get("app.locale")?, Some(json!(null)));
    assert!(repository.has("app.locale")?);
    assert!(!repository.has("app.absent")?);
    for (key, expected) in [
        ("app.locale", json!(null)),
        ("app.enabled", json!(false)),
        ("app.count", json!(0)),
        ("app.empty", json!("")),
    ] {
        assert_eq!(repository.get_or(key, json!("fallback"))?, expected);
    }
    assert_eq!(
        repository.get_or("app.absent", json!("fallback"))?,
        json!("fallback")
    );
    assert_eq!(
        config_repository_reading_consumer::public_fields(&repository)?,
        vec![
            ("app.name".into(), json!("读书会")),
            ("app.locale".into(), json!(null))
        ]
    );
    Ok(())
}

#[test]
fn literal_dot_key_and_batch_order_match_public_contract() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = repository(json!({
        "title.public": "字面名称",
        "title": {"public": "嵌套名称"},
        "list": [{"name": "首项"}]
    }));
    assert_eq!(repository.get("title.public")?, Some(json!("字面名称")));
    assert_eq!(repository.get("list.0.name")?, Some(json!("首项")));
    assert_eq!(repository.get("list.00.name")?, None);
    assert_eq!(
        repository.get_many(&["list.0.name", "absent", "title.public", "absent"])?,
        vec![
            ("list.0.name".into(), json!("首项")),
            ("absent".into(), json!(null)),
            ("title.public".into(), json!("字面名称")),
        ]
    );
    assert_eq!(
        repository.get_many_with_defaults(&[
            ("absent", json!("first")),
            ("title.public", json!("unused")),
            ("absent", json!("last"))
        ])?,
        vec![
            ("absent".into(), json!("last")),
            ("title.public".into(), json!("字面名称"))
        ]
    );
    Ok(())
}

#[test]
fn all_returns_an_independent_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository(json!({"app": {"name": "原名称"}}));
    let mut first = repository.all()?;
    first["app"]["name"] = json!("改动仅在快照");
    assert_eq!(repository.all()?["app"]["name"], "原名称");
    assert_eq!(repository.get("app.name")?, Some(json!("原名称")));
    Ok(())
}

#[test]
fn public_writer_updates_only_its_fixed_business_key() -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository(json!({"app": {"name": "原名称", "locale": "zh_CN"}}));
    assert_eq!(
        config_repository_reading_consumer::review_display_name(&repository)?,
        "已复核应用"
    );
    assert_eq!(repository.get("app.locale")?, Some(json!("zh_CN")));
    assert_eq!(repository.get("app.name")?, Some(json!("已复核应用")));
    Ok(())
}

#[test]
fn ordered_public_source_keeps_business_keys_after_set() -> Result<(), Box<dyn std::error::Error>> {
    let sources = openrust::config::ConfigSources::from_ordered_entries([
        ("reader", json!({"z_label": "先声明", "a_label": "后声明"})),
        ("app", json!({"name": "原名称", "locale": "zh_CN"})),
    ])?;
    let repository = ConfigRepository::new(sources);
    assert_eq!(
        config_repository_reading_consumer::public_reader_key_order(&repository)?,
        ["z_label", "a_label"]
    );
    repository.set("reader.z_label", json!("已复核"))?;
    assert_eq!(
        config_repository_reading_consumer::public_reader_key_order(&repository)?,
        ["z_label", "a_label"]
    );
    assert_eq!(repository.get("reader.z_label")?, Some(json!("已复核")));
    Ok(())
}

#[test]
fn public_console_loader_selects_file_without_changing_plain_application_loader()
-> Result<(), Box<dyn std::error::Error>> {
    struct FixtureDir(std::path::PathBuf);
    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "openrust-console-env-consumer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&root)?;
    let fixture = FixtureDir(root);
    std::fs::write(fixture.0.join(".env"), "CONSUMER_NAME=plain\n")?;
    std::fs::write(fixture.0.join(".env.local"), "CONSUMER_NAME=local\n")?;
    let plain = ConfigLoader::application_sources(&fixture.0, "CONSUMER_")?;
    let console =
        ConfigLoader::console_application_sources(&fixture.0, "CONSUMER_", Some("local"))?;
    assert_eq!(
        plain.figment().extract_inner::<String>("app.name")?,
        "plain"
    );
    assert_eq!(
        console.figment().extract_inner::<String>("app.name")?,
        "local"
    );
    std::fs::write(fixture.0.join(".env.local"), "CONSUMER_SUFFIX=from-code\n")?;
    let manifest = ConfigManifest::new()
        .section("app", |ctx| {
            Ok(json!({"name": ctx.env_or("CONSUMER_SUFFIX", "missing")}))
        })
        .typed("reader.factory", |_| Ok(17_u32));
    let coded =
        ConfigLoader::application_code_sources(&fixture.0, "CONSUMER_", Some("local"), &manifest)?;
    let coded = ConfigRepository::new(coded);
    assert_eq!(coded.string("app.name")?, "from-code");
    assert_eq!(coded.typed::<u32>("reader.factory")?.as_deref(), Some(&17));
    Ok(())
}

#[test]
fn file_discovery_reaches_external_provider_and_app() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = openrust::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        struct ReadingApplication;

        impl ApplicationDefinition for ReadingApplication {
            type Config = NoConfig;

            fn default_providers() -> openrust::support::DefaultProviders {
                openrust::support::DefaultProviders::empty()
            }

            fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
                vec![Box::new(
                    config_repository_reading_consumer::ReadingProvider,
                )]
            }
        }

        struct FixtureDir(std::path::PathBuf);

        impl Drop for FixtureDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let root = std::env::temp_dir().join(format!(
            "openrust-config-discovery-consumer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&root)?;
        let fixture = FixtureDir(root);
        std::fs::write(
            fixture.0.join("reader.toml"),
            "z_label = '先声明'\na_label = '后声明'\n[nested]\nnew = 2\n",
        )?;

        let sources = ConfigLoader::new(&fixture.0, Environment::Test).sources()?;
        let mut app = TestApp::<ReadingApplication>::builder(sources)
            .build()
            .await?;
        let repository = app.app()?.config_repository();
        assert_eq!(repository.get("reader.nested.new")?, Some(json!(2)));
        let nested: BTreeMap<String, i64> = app.app()?.section("reader.nested")?;
        assert_eq!(nested.get("new"), Some(&2));
        assert_eq!(nested.get("old"), None);
        assert_eq!(
            config_repository_reading_consumer::public_reader_key_order(&repository)?,
            ["z_label", "a_label"]
        );
        app.shutdown(openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        Ok(())
    })
}

// 防止显式测试覆盖丢失生产 loader 的同次 dotenv context 或原 typed 工厂。
#[test]
fn ordered_source_overrides_keep_original_environment_for_module_config()
-> Result<(), Box<dyn std::error::Error>> {
    struct ModuleConfig;
    impl ServiceProvider for ModuleConfig {
        fn boot<'a>(
            &'a self,
            app: &'a mut openrust::foundation::BootContext<'_>,
        ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
            Box::pin(async move {
                app.code_config(&ConfigManifest::new().section("module", |context| {
                    Ok(json!({"token": context.env_or("CONSUMER_TOKEN", "missing")}))
                }))?;
                Ok(())
            })
        }
    }
    let root = std::path::Path::new("/tmp").join(format!(
        "openrust-config-overlay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
    ));
    std::fs::create_dir(&root)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(root.join(".env"), "CONSUMER_TOKEN=captured-once\n")?;
        let manifest = ConfigManifest::new()
            .section("reader", |_| Ok(json!({"name": "base", "keep": true})))
            .typed("consumer.factory", |_| Ok(17_u32));
        let base = ConfigLoader::application_code_sources(&root, "APP_", None, &manifest)?;
        let merged = base
            .clone()
            .merge_ordered_entries([("reader", json!({"name": "override", "z": 1, "a": 2}))])?;
        assert!(
            base.merge_ordered_entries([("consumer", json!({"factory": 2}))])
                .is_err()
        );
        std::fs::write(root.join(".env"), "CONSUMER_TOKEN=changed-after-load\n")?;
        openrust::tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let mut builder = openrust::foundation::ProviderBuilder::new();
                builder.provider(ModuleConfig);
                let app =
                    openrust::Application::from_provider_runtime(builder.start(merged).await?)?;
                let repository = app.config_repository();
                assert_eq!(repository.string("reader.name")?, "override");
                assert!(repository.boolean("reader.keep")?);
                assert_eq!(repository.string("module.token")?, "captured-once");
                assert_eq!(
                    repository.typed::<u32>("consumer.factory")?.as_deref(),
                    Some(&17)
                );
                app.shutdown(
                    openrust::tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                )
                .await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            })
    })();
    std::fs::remove_dir_all(&root)?;
    result
}

// 防止跨领域默认读取到覆盖前的值，或晚快照覆盖偷偷重算已执行的函数。
#[test]
fn ordered_dependencies_read_pre_overrides_once_and_late_merge_stays_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct FixtureDir(std::path::PathBuf);
    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "openrust-dependency-consumer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
    ));
    std::fs::create_dir(&root)?;
    let fixture = FixtureDir(root);
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = Arc::clone(&calls);
    let app_calls = Arc::clone(&calls);
    let auth_calls = Arc::clone(&calls);
    let manifest = ConfigManifest::new()
        .section("server", move |_| {
            server_calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"port": 9090}))
        })
        .section("app", move |context| {
            app_calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({
                "name": "code-default",
                "url": format!("http://localhost:{}", context.require::<u16>("server.port")?),
            }))
        })
        .section("auth", move |context| {
            auth_calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"jwt": {
                "audience": context.require::<String>("app.name")?,
                "issuer": context.require::<String>("app.url")?,
            }}))
        });
    let sources = ConfigLoader::application_code_sources_with_overrides(
        &fixture.0,
        "DEPENDENCY_CONSUMER_",
        None,
        &manifest,
        [
            ("server", json!({"port": 8123})),
            ("app", json!({"name": "single-source"})),
            (
                "auth",
                json!({"jwt": {"issuer": "https://independent.test"}}),
            ),
        ],
    )?;
    let repository = ConfigRepository::new(sources.clone());
    assert_eq!(repository.string("app.name")?, "single-source");
    assert_eq!(repository.string("app.url")?, "http://localhost:8123");
    assert_eq!(repository.string("auth.jwt.audience")?, "single-source");
    assert_eq!(
        repository.string("auth.jwt.issuer")?,
        "https://independent.test"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let late = sources.merge_ordered_entries([("app", json!({"name": "late-explicit"}))])?;
    let late = ConfigRepository::new(late);
    assert_eq!(late.string("app.name")?, "late-explicit");
    assert_eq!(late.string("auth.jwt.audience")?, "single-source");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}
