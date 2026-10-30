//! 通过 `ServiceProvider` 装配本模块业务。

use openrust::Result;
use openrust::core::{BootContext, BoxFuture, ModuleConfigFiles, RegisterContext, ServiceProvider};
use std::sync::Arc;
#[cfg(feature = "session")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use crate::modules::example::services::article_write::ArticleWriter;
#[cfg(feature = "http-client")]
use crate::modules::example::services::upstream::ArticleSummaryService;
use crate::modules::example::services::{ExampleConfig, ExampleMessage};
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use openrust::db::sea_orm::{EntityTrait, QuerySelect};
/// 将综合示例路由接入 Provider 引导阶段。
pub(crate) struct ExampleServiceProvider {
    root: std::path::PathBuf,
}

impl ExampleServiceProvider {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

fn register_example_middleware(app: &mut BootContext<'_>) -> Result<()> {
    app.middleware_alias(
        "example.response-marker",
        openrust::http::axum::middleware::from_fn(
            crate::modules::example::routes::response_marker_middleware,
        ),
    )?;
    app.middleware_group("example.response", ["example.response-marker"])?;
    Ok(())
}

#[cfg(feature = "session")]
fn session_capability(app: &mut BootContext<'_>) -> Arc<AtomicBool> {
    let enabled = Arc::new(AtomicBool::new(false));
    let final_enabled = Arc::clone(&enabled);
    let store_bound = app.is_bound::<openrust::session::SessionStoreHandle>()
        && app.is_bound::<openrust::session::Session>();
    let repository = app.config_repository();
    app.validate_at_finalize(move || {
        Box::pin(async move {
            if store_bound {
                let config: openrust::config::SessionConfig = repository.section("session")?;
                final_enabled.store(config.driver.is_some(), Ordering::Release);
            }
            Ok(())
        })
    });
    enabled
}

impl ServiceProvider for ExampleServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.config_section(
            "example",
            ExampleConfig::default(),
            openrust::config::repository::SectionSchema::object([
                (
                    "greeting",
                    openrust::config::repository::SectionSchema::String,
                ),
                (
                    "article_summary_origin",
                    openrust::config::repository::SectionSchema::String.nullable(),
                ),
                #[cfg(all(
                    feature = "auth-oidc",
                    any(feature = "db-postgres", feature = "db-mysql")
                ))]
                (
                    "oidc",
                    openrust::config::repository::SectionSchema::object([
                        (
                            "issuer",
                            openrust::config::repository::SectionSchema::String,
                        ),
                        (
                            "client_id",
                            openrust::config::repository::SectionSchema::String,
                        ),
                        (
                            "client_secret",
                            openrust::config::repository::SectionSchema::String
                                .nullable()
                                .sensitive(),
                        ),
                        (
                            "redirect_uri",
                            openrust::config::repository::SectionSchema::String,
                        ),
                        (
                            "allow_loopback",
                            openrust::config::repository::SectionSchema::Boolean,
                        ),
                    ])
                    .nullable(),
                ),
            ]),
            ExampleConfig::validate,
        )?;
        app.singleton::<ExampleMessage, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<ExampleConfig>("example")?;
                Ok(Arc::new(ExampleMessage::new(config.greeting)))
            })
        });
        #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
        app.singleton::<ArticleWriter, _>(|resolver| {
            Box::pin(async move {
                Ok(Arc::new(ArticleWriter::new(
                    resolver.resolve::<openrust::Db>().await?,
                    resolver.resolve::<openrust::events::Events>().await?,
                    #[cfg(feature = "queue")]
                    resolver
                        .resolve::<openrust::notifications::Notifier>()
                        .await?,
                )))
            })
        });
        #[cfg(feature = "http-client")]
        app.singleton::<ArticleSummaryService, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<ExampleConfig>("example")?;
                let origin = config.article_summary_origin.ok_or_else(|| {
                    openrust::Error::custom(
                        "example_article_summary_not_configured",
                        "文章摘要上游尚未配置",
                    )
                })?;
                Ok(Arc::new(ArticleSummaryService::new(&origin)?))
            })
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines, reason = "模块 boot 同时装配业务资源和路由")]
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            register_example_middleware(app)?;
            let root = self.root.join("modules/example");
            let files = ModuleConfigFiles::new("example", root.clone())?;
            files.overlay(app)?;
            if app.is_bound::<openrust::i18n::I18n>() {
                let source = root.join("lang");
                let published = self.root.join("resources/lang/vendor/example");
                app.resolve::<openrust::i18n::I18n>()
                    .await?
                    .add_namespace("example", source, published)?;
            }
            #[cfg(feature = "view")]
            if app.is_bound::<openrust::view::Views>() {
                let source = files.resource("resources/views")?;
                let published = self.root.join("resources/views/vendor/example");
                app.resolve::<openrust::view::Views>()
                    .await?
                    .add_namespace("example", source, published)?;
            }
            let config = app.section::<openrust::config::DatabaseConfig>("database")?;
            let database_enabled = config.url.is_some()
                || !config.instances.is_empty()
                || !config.read_replicas.is_empty();
            #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
            if database_enabled && app.is_bound::<openrust::core::Readiness>() {
                let db = app.resolve::<openrust::Db>().await?;
                app.resolve::<openrust::core::Readiness>()
                    .await?
                    .register_probe("example.articles", move || {
                        let db = Arc::clone(&db);
                        async move {
                            crate::modules::example::models::article::Entity::find()
                                .limit(1)
                                .one(&db.primary_connection()?)
                                .await
                                .map_err(openrust::Error::internal)?;
                            Ok(())
                        }
                    })?;
            }
            #[cfg(feature = "view")]
            let view_enabled = app.is_bound::<openrust::view::Views>();
            #[cfg(not(feature = "view"))]
            let view_enabled = false;
            let i18n_enabled = app.is_bound::<openrust::i18n::I18n>();
            let example_config = app.section::<ExampleConfig>("example")?;
            let summary_origin = example_config.article_summary_origin;
            #[cfg(feature = "session")]
            let session_enabled = session_capability(app);
            #[cfg(all(
                feature = "auth-oidc",
                any(feature = "db-postgres", feature = "db-mysql")
            ))]
            let oidc_config = example_config.oidc;
            #[cfg(all(
                feature = "auth-oidc",
                any(feature = "db-postgres", feature = "db-mysql")
            ))]
            {
                let repository = app.config_repository();
                let expected = oidc_config.clone();
                app.validate_at_finalize(move || {
                    Box::pin(async move {
                        let final_config: ExampleConfig = repository.section("example")?;
                        if final_config.oidc != expected {
                            return Err(openrust::Error::custom(
                                "example_oidc_config_changed_after_routes",
                                "OIDC 配置在模块路由登记后改变，必须在该模块启动时覆盖",
                            ));
                        }
                        Ok(())
                    })
                });
            }
            #[cfg(all(
                feature = "auth-oidc",
                any(feature = "db-postgres", feature = "db-mysql")
            ))]
            let oidc_enabled = oidc_config.is_some();
            let config_reading_repository = app.config_repository();
            app.routes(move |routes| {
                crate::modules::example::routes::register(
                    routes,
                    &crate::modules::example::routes::RouteCapabilities {
                        database: database_enabled,
                        #[cfg(feature = "session")]
                        session: session_enabled.load(Ordering::Acquire),
                        view: view_enabled,
                        i18n: i18n_enabled,
                        summary_origin: summary_origin.clone(),
                        #[cfg(all(
                            feature = "auth-oidc",
                            any(feature = "db-postgres", feature = "db-mysql")
                        ))]
                        oidc: oidc_enabled,
                    },
                    config_reading_repository.clone(),
                )
            })?;
            Ok(())
        })
    }
}
