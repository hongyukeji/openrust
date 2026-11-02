//! 无默认连接也保留迁移贡献；模块选择只读实际配置，不把具名库当默认库。

use figment::{Figment, providers::Serialized};
use openrust::config::{ConfigSources, DatabaseConfig, FrameworkConfig, ModulesConfig};
use openrust::core::{App, DefaultProviders, ProviderBuilder, ProviderId};
use openrust::http::axum::{body::Body, http::Request, http::StatusCode};
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tower::ServiceExt;

const TIMEOUT: Duration = Duration::from_secs(5);

fn deployment() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    super::write_discovery_fixture(root.path())?;
    for directory in ["config", "lang", "resources/views"] {
        std::fs::create_dir_all(root.path().join("modules/example").join(directory))
            .map_err(Error::internal)?;
    }
    std::fs::write(
        root.path().join("modules_statuses.json"),
        br#"{"example":true}"#,
    )
    .map_err(Error::internal)?;
    Ok(root)
}

fn sources(root: &Path, database: &Value) -> Result<ConfigSources> {
    let figment = Figment::from(Serialized::defaults(FrameworkConfig::default())).merge(
        Serialized::defaults(json!({
            "application_root":root,"app":{"env":"testing"},
            "database":database,
            "cache":{"default":"memory","stores":{"memory":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{
                "driver":"fs","root":root.join("storage/app")
            }}},
            "security":{"encryption":{"active_key_id":"test","keys":{
                "test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
            }}}
        })),
    );
    super::test_sources(&figment)
}

fn check(condition: bool, code: &'static str, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(code, message))
    }
}

async fn status(app: &App, path: &str, expected: StatusCode) -> Result<()> {
    let response = openrust::http::build_router(app)?
        .oneshot(
            Request::get(path)
                .header("host", "localhost")
                .body(Body::empty())
                .map_err(Error::internal)?,
        )
        .await
        .map_err(Error::internal)?;
    check(
        response.status() == expected,
        "example_database_route_selection",
        "模块路由没有遵守当前默认连接配置",
    )
}

#[cfg(feature = "auth")]
fn check_no_database_business(app: &App) -> Result<()> {
    let container = app.state().container();
    check(
        !container.contains::<super::services::account_security::AccountSecurity>()
            && !container.contains::<super::services::tenant::TenantRecords>()
            && !container.contains::<super::services::article_write::ArticleWriter>(),
        "example_database_business_selected",
        "无默认连接不应登记依赖默认数据库的业务绑定",
    )
}

#[tokio::test]
async fn no_url_host_registers_module_migrations_and_seed_without_running_sql() -> Result<()> {
    use openrust::db::MigrationServiceProvider;
    use openrust::db::sea_orm_migration::MigratorTrait;

    let root = deployment()?;
    let sources = sources(
        root.path(),
        &json!({"url":null,"instances":{},"read_replicas":[]}),
    )?;
    let configuration = sources
        .figment()
        .extract::<crate::bootstrap::Configuration>()
        .map_err(Error::internal)?;
    let builder = ProviderBuilder::for_application::<crate::bootstrap::Server>(&configuration);
    let mut app = App::from_provider_runtime(builder.start(sources).await?)?;
    let outcome: Result<()> = async {
        let commands = app
            .commands()
            .map(openrust::Command::name)
            .collect::<Vec<_>>();
        check(
            commands.contains(&"migrate") && commands.contains(&"db seed"),
            "example_database_commands_missing",
            "无默认连接仍应登记迁移和种子命令",
        )?;
        check(
            super::database::migrations::Migrator::migrations()
                .iter()
                .any(|migration| migration.name() == "m20261028_000001_example_heartbeats"),
            "example_migration_source_missing",
            "通用模块迁移源码应在未启用内置驱动时仍可编译",
        )?;
        check(
            app.state()
                .resolve::<openrust::Db>()
                .await?
                .connection()
                .is_err(),
            "example_database_unexpected_connection",
            "无默认 URL 不应建立默认数据库连接",
        )?;
        status(&app, "/api/example", StatusCode::OK).await?;
        status(&app, "/api/example/articles", StatusCode::NOT_FOUND).await?;
        #[cfg(feature = "auth")]
        check_no_database_business(&app)?;
        // 复用公开 force 生命周期贡献同一模块，证明真实私有 catalog 已登记，且无需 SQL。
        let duplicate = app
            .force_register_provider(
                MigrationServiceProvider::<super::database::migrations::Migrator>::new()
                    .for_module("example"),
            )
            .await
            .err()
            .ok_or_else(|| {
                Error::custom(
                    "example_migration_catalog_missing",
                    "重复模块迁移贡献未被真实目录拒绝",
                )
            })?;
        check(
            duplicate.code() == "module_migration_conflict",
            "example_migration_catalog_wrong_error",
            "重复模块迁移贡献没有返回目录冲突",
        )?;
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome.and(shutdown)
}

#[tokio::test]
async fn final_named_only_sources_do_not_select_default_database_business() -> Result<()> {
    let root = deployment()?;
    // 先创建生产 metadata 工厂，再把最终数据库来源交给同一启动器求值。
    let manifest = super::manifest(root.path(), &ModulesConfig::default());
    let final_sources = sources(
        root.path(),
        &json!({
            "url":null,"read_replicas":[],
            "instances":{"reporting":{"url":"uncompiled://unused/reporting"}}
        }),
    )?;
    let mut builder = ProviderBuilder::new();
    builder
        .default_providers(DefaultProviders::auto())
        // 此测试只验模块选择，不声称验证真实具名连接；正常生产仍保留 DB 驱动预检。
        .remove_provider(ProviderId::of::<openrust::db::DatabaseServiceProvider>())
        .module_manifest(manifest);
    let app = App::from_provider_runtime(builder.start(final_sources).await?)?;
    let outcome: Result<()> = async {
        let config: DatabaseConfig = app.config_repository().section("database")?;
        check(
            !config.has_default_connection() && config.instances.contains_key("reporting"),
            "example_database_named_fixture_invalid",
            "具名连接 fixture 不应提供默认 URL",
        )?;
        check(
            !app.state().container().contains::<openrust::Db>(),
            "example_database_provider_not_removed",
            "选择测试不得意外建立真实数据库驱动",
        )?;
        #[cfg(feature = "auth")]
        check_no_database_business(&app)?;
        #[cfg(feature = "queue")]
        check(
            !app.state().container().contains::<openrust::queue::Queue>(),
            "example_database_named_selected_queue",
            "仅具名连接不应自动选择默认数据库队列",
        )?;
        status(&app, "/api/example", StatusCode::OK).await?;
        status(&app, "/api/example/articles", StatusCode::NOT_FOUND).await?;
        status(&app, "/api/example/jobs/heartbeat", StatusCode::NOT_FOUND).await
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome.and(shutdown)
}

#[tokio::test]
async fn explicit_descriptor_override_does_not_require_a_database_section() -> Result<()> {
    use openrust::config::{ConfigLoader, ConfigManifest};
    use openrust::core::ModuleManifest;

    let root = deployment()?;
    let source = ConfigLoader::application_code_sources_with_overrides(
        root.path(),
        "EXAMPLE_DB_SELECTION_",
        None,
        &ConfigManifest::new(),
        [
            ("app", json!({"env":"testing"})),
            (
                "cache",
                json!({"default":"memory","stores":{"memory":{"driver":"memory"}}}),
            ),
            (
                "security",
                json!({"encryption":{"active_key_id":"test","keys":{
                    "test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
                }}}),
            ),
        ],
    )?;
    let mut builder = ProviderBuilder::new();
    builder
        .default_providers(DefaultProviders::for_runtime(false, false))
        .remove_provider(ProviderId::of::<openrust::db::DatabaseServiceProvider>())
        .module_manifest(
            ModuleManifest::new()
                .module(super::descriptor(false, root.path().to_path_buf()))
                .enable("example"),
        );
    let app = App::from_provider_runtime(builder.start(source).await?)?;
    let outcome: Result<()> = async {
        check(
            app.config_repository().get("database")?.is_none(),
            "example_descriptor_database_section_added",
            "显式 false 覆写的 fixture 不应暗中提供数据库区段",
        )?;
        #[cfg(feature = "auth")]
        check_no_database_business(&app)?;
        status(&app, "/api/example", StatusCode::OK).await?;
        status(&app, "/api/example/articles", StatusCode::NOT_FOUND).await
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome.and(shutdown)
}

#[cfg(feature = "auth")]
struct RemoveDefaultUrl;

#[cfg(feature = "auth")]
impl openrust::core::ServiceProvider for RemoveDefaultUrl {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.overlay("database", json!({"url":null})) })
    }
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn earlier_boot_overlay_controls_routes_without_unloading_registered_business() -> Result<()>
{
    let root = deployment()?;
    let source = sources(
        root.path(),
        &json!({
            "url":"uncompiled://unused/default","instances":{},"read_replicas":[]
        }),
    )?;
    let mut builder = ProviderBuilder::new();
    builder
        .default_providers(DefaultProviders::for_runtime(false, false))
        .remove_provider(ProviderId::of::<openrust::db::DatabaseServiceProvider>())
        .provider(RemoveDefaultUrl)
        .provider(super::providers::ExampleServiceProvider::new(
            root.path().to_path_buf(),
        ));
    let app = App::from_provider_runtime(builder.start(source).await?)?;
    let outcome: Result<()> = async {
        check(
            !app.config_repository()
                .section::<DatabaseConfig>("database")?
                .has_default_connection(),
            "example_database_overlay_missing",
            "前序 boot 没有移除默认连接来源",
        )?;
        check(
            app.state()
                .container()
                .contains::<super::services::article_write::ArticleWriter>(),
            "example_database_binding_unloaded",
            "boot 覆盖不能冒称卸载此前已登记的业务绑定",
        )?;
        status(&app, "/api/example", StatusCode::OK).await?;
        status(&app, "/api/example/articles", StatusCode::NOT_FOUND).await
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome.and(shutdown)
}

struct EnableDefaultUrl(Arc<AtomicBool>);
struct SelectionResource(Arc<AtomicBool>);

impl openrust::core::ServiceProvider for EnableDefaultUrl {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(SelectionResource(Arc::clone(&self.0))))
                .await
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let resource = app.resolve::<SelectionResource>().await?;
            app.resources().on_shutdown(move || {
                Box::pin(async move {
                    resource.0.store(true, Ordering::Release);
                    Ok(())
                })
            })?;
            app.overlay("database", json!({"url":"uncompiled://unused/default"}))
        })
    }
}

#[tokio::test]
async fn adding_default_url_in_earlier_boot_rejects_unregistered_business_and_closes_resources()
-> Result<()> {
    let root = deployment()?;
    let source = sources(
        root.path(),
        &json!({"url":null,"instances":{},"read_replicas":[]}),
    )?;
    let closed = Arc::new(AtomicBool::new(false));
    let mut builder = ProviderBuilder::new();
    builder
        // 显式清单由应用负责选择；只测模块自身的反向时序拒绝，不绕过 auto 的独立检查。
        .default_providers(DefaultProviders::for_runtime(false, false))
        .remove_provider(ProviderId::of::<openrust::db::DatabaseServiceProvider>())
        .provider(EnableDefaultUrl(Arc::clone(&closed)))
        .provider(super::providers::ExampleServiceProvider::new(
            root.path().to_path_buf(),
        ));
    let error = match builder.start(source).await {
        Err(error) => error,
        Ok(runtime) => {
            runtime
                .shutdown(tokio::time::Instant::now() + TIMEOUT)
                .await?;
            return Err(Error::custom(
                "example_database_changed_selection_accepted",
                "模块未登记数据库业务却在新增 URL 后成功启动",
            ));
        }
    };
    check(
        error.code() == "example_database_selection_changed",
        "example_database_changed_selection_wrong_error",
        "模块反向连接覆盖没有返回需要重新 bootstrap 的明确错误",
    )?;
    check(
        closed.load(Ordering::Acquire),
        "example_database_failed_boot_leaked_resource",
        "模块拒绝启动后仍须等待已登记服务资源关闭",
    )
}
