use super::*;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::database::sea_orm_migration::MigrationTrait;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::database::sea_orm_migration::MigratorTrait;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::database::sea_orm_migration::SchemaManager;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::foundation::Application;
use openrust::foundation::ProviderBuilder;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::testing::TestDatabase;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::translation::Locale;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::translation::MessageArgs;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::translation::TranslationConfig;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::translation::TranslationLoader;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
use openrust::translation::TranslationResource;
use openrust::translation::Translator;

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
struct EmptyMigrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
async fn empty_database() -> Result<Option<TestDatabase>> {
    if let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") {
        return TestDatabase::create::<EmptyMigrator>(&url).await.map(Some);
    }
    #[cfg(feature = "db-sqlite")]
    {
        TestDatabase::sqlite_file::<EmptyMigrator>().await.map(Some)
    }
    #[cfg(not(feature = "db-sqlite"))]
    {
        eprintln!("未执行数据库翻译Provider迁移回归：需OPENRUST_TEST_DATABASE_URL或db-sqlite");
        Ok(None)
    }
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
async fn translation_tables(database: &TestDatabase, expected: bool) -> Result<()> {
    let connection = database.connection()?;
    let schema = SchemaManager::new(&connection);
    for table in ["translation_lines", "translation_revisions"] {
        assert_eq!(
            schema.has_table(table).await.map_err(Error::internal)?,
            expected,
            "{table}"
        );
    }
    Ok(())
}

#[test]
fn configuration_rejects_invalid_bounds_without_changing_file_default() {
    let defaults = TranslationDatabaseConfig::default();
    assert_eq!(defaults.connection, None);
    assert_eq!(defaults.max_rows, 10_000);
    assert_eq!(defaults.revision_check_interval_secs, 30);
    assert!(validate(&defaults).is_ok());
    assert!(
        validate(&TranslationDatabaseConfig {
            max_rows: 0,
            ..defaults.clone()
        })
        .is_err()
    );
    assert!(
        validate(&TranslationDatabaseConfig {
            revision_check_interval_secs: 0,
            ..defaults.clone()
        })
        .is_err()
    );
    assert!(
        validate(&TranslationDatabaseConfig {
            connection: Some(String::new()),
            ..defaults
        })
        .is_err()
    );
}
#[tokio::test]
async fn provider_requires_explicit_database_provider() {
    let mut builder = ProviderBuilder::new();
    builder.provider(DatabaseTranslationServiceProvider);
    assert!(builder.start(figment::Figment::new()).await.is_err());
}
// 通过真实容器/配置/数据库Provider接入来源，验证未安装第二个I18n服务。
#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
#[tokio::test]
async fn provider_contributes_singleton_loader_and_standard_migrations_without_overriding_i18n()
-> Result<()> {
    let Some(database) = empty_database().await? else {
        return Ok(());
    };
    let outcome: Result<()> = async {
        translation_tables(&database, false).await?;
        let config = figment::Figment::new().merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "database":{"url":database.url()},
                "translation_database":{"max_rows":5,"revision_check_interval_secs":1}
            }),
        ));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(DatabaseTranslationServiceProvider);
        let app = Application::from_provider_runtime(builder.start(config).await?)?;
        let result: Result<()> = async {
            translation_tables(&database, false).await?;
            app.run_command("migrate", Vec::new()).await?;
            translation_tables(&database, true).await?;
            let loader = app
                .state()
                .container()
                .resolve::<DatabaseTranslationLoader>()
                .await?;
            assert!(Arc::ptr_eq(
                &loader,
                &app.state()
                    .container()
                    .resolve::<DatabaseTranslationLoader>()
                    .await?
            ));
            assert!(
                app.state()
                    .container()
                    .resolve::<Translator>()
                    .await
                    .is_err()
            );
            let resource = TranslationResource {
                locale: Locale::new("fr")?,
                group: "*".into(),
                namespace: None,
            };
            loader
                .write(vec![crate::TranslationMutation::Put {
                    resource,
                    key: "hello".into(),
                    value: serde_json::json!("bonjour"),
                    enabled: true,
                }])
                .await?;
            let service =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            assert_eq!(
                service.translate(&Locale::new("fr")?, "hello", &MessageArgs::new()),
                "bonjour"
            );
            assert_eq!(loader.begin_snapshot().await?.resources().len(), 1);
            let resource = TranslationResource {
                locale: Locale::new("fr")?,
                group: "*".into(),
                namespace: None,
            };
            let extra = (0..5)
                .map(|index| crate::TranslationMutation::Put {
                    resource: resource.clone(),
                    key: format!("extra_{index}"),
                    value: serde_json::json!("extra"),
                    enabled: true,
                })
                .collect();
            loader.write(extra).await?;
            assert!(loader.refresh().await.is_err());
            // 已验证的父目录不会因来源后来超限而丢失。
            assert_eq!(
                service.translate(&Locale::new("fr")?, "hello", &MessageArgs::new()),
                "bonjour"
            );
            Ok(())
        }
        .await;
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
            .await?;
        result
    }
    .await;
    database.close().await?;
    outcome
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite"))]
#[tokio::test]
async fn named_connection_migration_contribution_creates_only_selected_schema_and_loader_uses_it()
-> Result<()> {
    let Some(default) = empty_database().await? else {
        return Ok(());
    };
    let named = match empty_database().await {
        Ok(Some(database)) => database,
        Ok(None) => {
            default.close().await?;
            return Ok(());
        }
        Err(error) => {
            default.close().await?;
            return Err(error);
        }
    };
    let outcome: Result<()> = async {
        translation_tables(&default, false).await?;
        translation_tables(&named, false).await?;
        let config = figment::Figment::new().merge(figment::providers::Serialized::defaults(
            serde_json::json!({
                "database":{"url":default.url(),"instances":{"translations":{"url":named.url()}}},
                "translation_database":{"connection":"translations"}
            }),
        ));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(DatabaseServiceProvider)
            .provider(DatabaseTranslationServiceProvider);
        let app = Application::from_provider_runtime(builder.start(config).await?)?;
        let result: Result<()> = async {
            translation_tables(&default, false).await?;
            translation_tables(&named, false).await?;
            app.run_command("migrate", vec!["--database".into(), "translations".into()])
                .await?;
            translation_tables(&default, false).await?;
            translation_tables(&named, true).await?;
            let loader = app
                .state()
                .container()
                .resolve::<DatabaseTranslationLoader>()
                .await?;
            assert!(
                app.state()
                    .container()
                    .resolve::<Translator>()
                    .await
                    .is_err()
            );
            let locale = Locale::new("en")?;
            loader
                .write(vec![crate::TranslationMutation::Put {
                    resource: TranslationResource {
                        locale: locale.clone(),
                        group: "*".into(),
                        namespace: None,
                    },
                    key: "selected".into(),
                    value: serde_json::json!("named schema"),
                    enabled: true,
                }])
                .await?;
            let snapshot = loader.begin_snapshot().await?;
            assert_eq!(
                snapshot.load(&locale, "*", None)?.get("selected"),
                Some(&serde_json::json!("named schema"))
            );
            let databases = app.state().container().resolve::<Databases>().await?;
            assert_eq!(
                databases
                    .get("translations")?
                    .table("translation_revisions")?
                    .where_eq("id", 1_i32)?
                    .value::<i64>("revision")
                    .await?,
                Some(1)
            );
            assert_eq!(
                databases
                    .get("translations")?
                    .table("translation_lines")?
                    .where_eq("line_key", "selected")?
                    .value::<String>("value_json")
                    .await?,
                Some("\"named schema\"".into())
            );
            translation_tables(&default, false).await?;
            Ok(())
        }
        .await;
        let cleanup = app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
            .await;
        result?;
        cleanup
    }
    .await;
    let (default_cleanup, named_cleanup) = tokio::join!(default.close(), named.close());
    outcome?;
    default_cleanup?;
    named_cleanup
}

#[tokio::test]
async fn cold_registration_with_no_configured_database_does_not_initialize_a_connection()
-> Result<()> {
    let config = figment::Figment::new().merge(figment::providers::Serialized::defaults(
        serde_json::json!({"database":{"url":null,"instances":{}}}),
    ));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(DatabaseTranslationServiceProvider);
    let runtime = builder.start(config).await?;
    let result: Result<()> = async {
        let databases = runtime.container().resolve::<Databases>().await?;
        assert!(databases.default_database().backend().is_err());
        let loader = runtime
            .container()
            .resolve::<DatabaseTranslationLoader>()
            .await?;
        assert!(Arc::ptr_eq(
            &loader,
            &runtime
                .container()
                .resolve::<DatabaseTranslationLoader>()
                .await?
        ));
        assert!(runtime.container().resolve::<Translator>().await.is_err());
        assert!(databases.default_database().backend().is_err());
        Ok(())
    }
    .await;
    let cleanup = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await;
    result?;
    cleanup
}
