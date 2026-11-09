//! 标准Provider的SQLite具名连接、完整迁移及本地认证沿唯一业务模块验证。

use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};

#[tokio::test]
async fn named_sqlite_reporting_uses_standard_provider_and_module_http() -> openrust::Result<()> {
    let source = figment::Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({
            "app":{"env":"testing"},
            "modules":{"activator":"registered"},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"},"redis":{"driver":"memory"}}},
            "database":{"url":null,"instances":{"reporting":{"url":"sqlite::memory:","max_connections":4,"foreign_key_constraints":null,"busy_timeout":-1,"synchronous":1,"pragmas":{"foreign_keys":0,r#""main"."user_version""#:"41; CREATE TABLE unexpected_tail (id INTEGER)"}}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        }),
    ));
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        sqlite_test_sources(&source)?,
    )
    .build()
    .await
    .inspect_err(|error| eprintln!("SQLite 宿主启动失败: {error}"))?;
    let outcome: openrust::Result<()> = async {
        app.request(Request::get("/api/example/reporting-connection").header("host","localhost").body(Body::empty()).map_err(openrust::Error::internal)?)
            .await?.assert_status(StatusCode::OK)?.assert_json(&serde_json::json!({"success":true,"code":"ok","message":"ok","data":{"reachable":true,"backend":"sqlite"}}))?;
        let default = app.app()?.state().resolve::<openrust::Db>().await?;
        assert!(default.backend().is_err());
        let reporting = app.app()?.state().resolve::<openrust::database::Databases>().await?.get("reporting")?;
        let connection = reporting.primary_connection()?;
        assert_eq!(scalar(&connection, "PRAGMA foreign_keys").await?, 0);
        assert_eq!(scalar(&connection, "PRAGMA busy_timeout").await?, 0);
        assert_eq!(scalar(&connection, "PRAGMA synchronous").await?, 1);
        assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 41);
        assert_eq!(scalar(&connection, "SELECT count(*) FROM sqlite_master WHERE name = 'unexpected_tail'").await?, 0);
        Ok(())
    }.await;
    let cleanup = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    cleanup
}

// 精简宿主显式启用完整 TOML cache 区段，保留代码配置环境并隔离默认 Redis。
// 该 fixture 不承担默认配置与 dotenv 优先级验收，数据库仍走根宿主和标准 Provider。
fn sqlite_test_sources(
    source: &figment::Figment,
) -> openrust::Result<openrust::config::ConfigSources> {
    use openrust::foundation::ApplicationDefinition;
    let fixture = tempfile::tempdir().map_err(openrust::Error::internal)?;
    let config = fixture.path().join("config");
    std::fs::create_dir(&config).map_err(openrust::Error::internal)?;
    std::fs::write(
        config.join("cache.toml"),
        "default = 'memory'\n[stores.memory]\ndriver = 'memory'\n[stores.redis]\ndriver = 'memory'\n",
    )
    .map_err(openrust::Error::internal)?;
    let mut entries = source
        .clone()
        .extract::<serde_json::Map<String, serde_json::Value>>()
        .map_err(openrust::config::ConfigError::from)?;
    entries.remove("cache");
    entries.insert(
        "application_root".into(),
        serde_json::json!(env!("CARGO_MANIFEST_DIR")),
    );
    Ok(
        openrust::config::ConfigLoader::application_code_sources_with_overrides(
            fixture.path(),
            "APP_",
            None,
            &crate::bootstrap::Server::config().with_toml(),
            entries,
        )?,
    )
}

async fn scalar(
    connection: &openrust::database::sea_orm::DatabaseConnection,
    sql: &str,
) -> openrust::Result<i64> {
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
    connection
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| openrust::Error::custom("missing_sqlite_result", "缺少 SQLite 配置结果"))?
        .try_get_by_index(0)
        .map_err(openrust::Error::internal)
}

struct EmptyMigrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl openrust::database::sea_orm_migration::MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

// 真实框架与唯一模块迁移 Provider 的完整清单；Root+Auth 的 S3 缺口独立保留。
#[tokio::test]
async fn sqlite_file_catalog_providers_apply_rollback_and_reapply_all_registered_migrations()
-> openrust::Result<()> {
    let database = openrust::testing::TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let mut providers = openrust::foundation::ProviderBuilder::new();
    providers
        .provider(openrust::database::DatabaseServiceProvider)
        .provider(openrust::database::MigrationServiceProvider::<
            openrust::database::FrameworkMigrator,
        >::new())
        .provider(
            openrust::database::MigrationServiceProvider::<super::database::migrations::Migrator>::new()
                .for_module("example"),
        );
    let runtime = providers
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(
                serde_json::json!({"database":{"url":database.url()}}),
            ),
        ))
        .await?;
    let app = openrust::foundation::Application::from_provider_runtime(runtime)?;
    let outcome = exercise_catalog(&app, &database).await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    let cleanup = database.close().await;
    outcome?;
    shutdown?;
    cleanup
}

async fn exercise_catalog(
    app: &openrust::foundation::Application,
    database: &openrust::testing::TestDatabase,
) -> openrust::Result<()> {
    use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
    use openrust::database::sea_orm_migration::MigratorTrait;

    let mut names = openrust::database::FrameworkMigrator::migrations()
        .into_iter()
        .chain(super::database::migrations::Migrator::migrations())
        .map(|migration| migration.name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        names.len(),
        "迁移来源不能靠去重掩盖重复"
    );
    let expected = i64::try_from(names.len()).map_err(openrust::Error::internal)?;
    app.run_command("migrate", Vec::new()).await?;
    let connection = database.connection()?;
    assert_eq!(
        scalar(&connection, "SELECT count(*) FROM migrations").await?,
        expected
    );
    let rows = connection
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT migration FROM migrations ORDER BY migration",
        ))
        .await
        .map_err(openrust::Error::internal)?;
    let actual = rows
        .into_iter()
        .map(|row| row.try_get_by_index::<String>(0))
        .collect::<Result<Vec<_>, _>>()
        .map_err(openrust::Error::internal)?;
    assert_eq!(actual, names);
    app.run_command("migrate", vec!["status".into()]).await?;
    app.run_command("migrate", vec!["down".into(), expected.to_string()])
        .await?;
    assert_eq!(
        scalar(&connection, "SELECT count(*) FROM migrations").await?,
        0
    );
    assert_eq!(scalar(&connection, "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT IN ('migrations','seaql_migrations','sqlite_sequence')").await?, 0);
    assert_eq!(scalar(&connection, "PRAGMA foreign_keys").await?, 1);
    app.run_command("migrate", Vec::new()).await?;
    assert_eq!(
        scalar(&connection, "SELECT count(*) FROM migrations").await?,
        expected
    );
    let foreign_key_errors = connection
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await
        .map_err(openrust::Error::internal)?;
    assert!(foreign_key_errors.is_empty());
    Ok(())
}

fn sqlite_default_host_source(database: &openrust::testing::TestDatabase) -> figment::Figment {
    figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    )).merge(figment::providers::Serialized::defaults(serde_json::json!({
        "app":{"env":"testing","url":"http://localhost"}, "modules":{"activator":"registered"},
        "database":{"url":database.url()},
        "cache":{"default":"memory","stores":{"memory":{"driver":"memory"},"redis":{"driver":"memory"}}},
        "session":{"driver":"array","secure":false},
        "auth":{"jwt":{"issuer":"http://openrust.test","audience":"sqlite-migration","active_key_id":"test","keys":{"test":"QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI="}}},
        "security":{
            "encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}},
            "signed_links":{"issuer":"http://openrust.test","active_key_id":"test","keys":{"test":"Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M="}}
        }
    })))
}

#[cfg(not(feature = "auth"))]
#[tokio::test]
async fn sqlite_file_default_host_without_auth_runs_the_actual_complete_catalog()
-> openrust::Result<()> {
    let database = openrust::testing::TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let source = sqlite_default_host_source(&database);
    let mut app = openrust::testing::TestApp::<crate::bootstrap::Server>::builder(
        sqlite_test_sources(&source)?,
    )
    .build()
    .await
    .inspect_err(|error| eprintln!("SQLite 宿主启动失败: {error}"))?;
    let outcome = exercise_catalog(app.app()?, &database).await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    let cleanup = database.close().await;
    outcome?;
    shutdown?;
    cleanup
}

#[cfg(feature = "auth")]
#[path = "sqlite_auth_flows.rs"]
mod sqlite_auth_flows;

#[cfg(feature = "auth")]
#[tokio::test]
async fn sqlite_default_host_runs_local_auth_and_native_session_business_contract()
-> openrust::Result<()> {
    sqlite_auth_flows::root_file_local_auth_contract().await
}
