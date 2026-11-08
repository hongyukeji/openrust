//! 通过 `ServiceProvider` 装配本模块业务。

use openrust::Result;
use openrust::container::{AttributeValue, ParameterValue, ServiceKey};
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::modules::example::routes::ExampleResponseMarker;
#[cfg(feature = "auth")]
use crate::modules::example::services::article_write::ArticleWriter;
#[cfg(feature = "http-client")]
use crate::modules::example::services::upstream::ArticleSummaryService;
use crate::modules::example::services::{
    EXAMPLE_MESSAGE_ALIAS, EXAMPLE_PRESENTER_ALIAS, EXAMPLE_PRESENTER_CLASS, ExampleConfig,
    ExampleGreetingConstructed, ExampleMessage, ExamplePresenter, ExampleServedAt,
    GreetingPresenter,
};
#[cfg(feature = "auth")]
use openrust::database::sea_orm::{EntityTrait, QuerySelect};
/// 将综合示例路由接入 Provider 引导阶段。
pub(crate) struct ExampleServiceProvider {
    root: std::path::PathBuf,
    directory: std::path::PathBuf,
    module_repository: bool,
    runtime_policy_enabled: bool,
    database_override: Option<bool>,
    database_selected_at_register: AtomicBool,
}

impl ExampleServiceProvider {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self {
            directory: root.join("modules/example"),
            module_repository: false,
            root,
            runtime_policy_enabled: false,
            database_override: None,
            database_selected_at_register: AtomicBool::new(false),
        }
    }

    /// 与官方普通模块 Provider 一样，到 boot 使用模块仓储的真实目录。
    pub(crate) fn with_module_repository(mut self) -> Self {
        self.module_repository = true;
        self
    }

    /// 仅程序化模块描述可显式覆盖；生产路径在 register/boot 分别读取当前配置。
    pub(super) fn with_database_override(mut self, enabled: Option<bool>) -> Self {
        self.database_override = enabled;
        self
    }

    /// 显式开启进程本地 typed 配置演示；启用后该应用不能写配置缓存。
    #[allow(
        dead_code,
        reason = "应用可按需选择此 Provider 选项，默认模块保持可缓存"
    )]
    pub(crate) fn with_runtime_policy(mut self) -> Self {
        self.runtime_policy_enabled = true;
        self
    }
}

#[cfg(feature = "auth-fortify")]
async fn register_fortify_views(app: &mut RegisterContext<'_>) -> Result<()> {
    use crate::modules::example::http::controllers::fortify::ExampleFortifyViews;
    use openrust::auth::fortify::{
        ConfirmPasswordViewResponse, FortifyConfig, LoginViewResponse,
        TwoFactorChallengeViewResponse,
    };

    // 工厂在请求作用域读取最终配置；调用方已有绑定优先。
    macro_rules! bind_view {
        ($contract:ident) => {
            if !app.is_bound::<dyn $contract>() {
                app.scoped::<dyn $contract, _>(|resolver| {
                    Box::pin(async move {
                        let config = resolver.section::<FortifyConfig>("fortify")?;
                        Ok(Arc::new(ExampleFortifyViews::new(config.username))
                            as Arc<dyn $contract>)
                    })
                })
                .await?;
            }
        };
    }
    bind_view!(LoginViewResponse);
    bind_view!(ConfirmPasswordViewResponse);
    bind_view!(TwoFactorChallengeViewResponse);
    Ok(())
}

fn register_example_middleware(app: &mut BootContext<'_>) -> Result<()> {
    app.middleware_alias_terminable::<ExampleResponseMarker>("example.response-marker")?;
    app.middleware_group("example.response", ["example.response-marker"])?;
    #[cfg(feature = "auth")]
    {
        app.middleware_alias(
            "example.article-auth",
            openrust::auth::AuthLayer::required(openrust::auth::AuthMode::Jwt),
        )?;
        // 确认位必须在认证拒绝前设置，错误响应也保留预检协议头。
        app.append_middleware_priority("example.article-auth")?;
    }
    Ok(())
}

fn register_lifecycle_diagnostics(app: &mut BootContext<'_>) {
    use std::time::Duration;

    app.when_request_lifecycle_is_longer_than(Duration::from_secs(1), |_, request| {
        Box::pin(async move {
            tracing::debug!(status = request.response.status().as_u16(), ?request.elapsed, "示例慢请求");
            Ok(())
        })
    });
    app.when_command_lifecycle_is_longer_than(Duration::from_secs(5), |_, command| {
        Box::pin(async move {
            tracing::debug!(status = command.status, ?command.elapsed, "示例慢命令");
            Ok(())
        })
    });
}

async fn register_binding_extension_route(app: &mut BootContext<'_>) -> Result<()> {
    let boot_message = app.resolve::<ExampleMessage>().await?;
    app.routes(move |routes| {
        let boot_message = Arc::clone(&boot_message);
        routes.get(
            "/api/example/container-extension",
            move |openrust::http::Inject(message): openrust::http::Inject<ExampleMessage>| {
                let boot_message = Arc::clone(&boot_message);
                async move {
                    openrust::http::Json(serde_json::json!({
                        "message": message.text(),
                        "binding_extension": message.binding_extension(),
                        "same_boot_instance": Arc::ptr_eq(&message, &boot_message),
                    }))
                }
            },
        )
    })?;
    Ok(())
}

async fn overlay_example_config_and_register_resources(
    app: &mut BootContext<'_>,
    application_root: &std::path::Path,
    root: &std::path::Path,
    runtime_policy_enabled: bool,
) -> Result<()> {
    app.code_config(&crate::modules::example::config::boot())?;
    if runtime_policy_enabled {
        app.code_config(&crate::modules::example::config::runtime_policy_manifest())?;
        let policy = app
            .config_repository()
            .typed::<crate::modules::example::config::ExampleRuntimePolicy>(
                "example.runtime_policy",
            )?
            .ok_or_else(|| {
                openrust::Error::custom("example_policy_missing", "示例模块缺少进程本地配置策略")
            })?;
        app.routes(move |routes| {
            let policy = Arc::clone(&policy);
            routes.get("/api/example/config-policy", move || {
                let policy = Arc::clone(&policy);
                async move {
                    openrust::http::Json(serde_json::json!({
                        "greeting_prefix": policy.greeting_prefix.clone(),
                    }))
                }
            })
        })?;
    }
    if app.is_bound::<openrust::translation::Translator>() {
        let source = root.join("lang");
        let published = application_root.join("resources/lang/vendor/example");
        app.resolve::<openrust::translation::Translator>()
            .await?
            .add_namespace("example", source, published)?;
    }
    #[cfg(feature = "view")]
    if app.is_bound::<openrust::view::Views>() {
        let files = openrust_modules::ModuleConfigFiles::new("example", root)?;
        let source = files.resource("resources/views")?;
        let published = application_root.join("resources/views/vendor/example");
        app.resolve::<openrust::view::Views>()
            .await?
            .add_namespace("example", source, published)?;
    }
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

fn register_example_schema(app: &mut RegisterContext<'_>) -> Result<()> {
    app.config_section(
        "example",
        ExampleConfig::default(),
        openrust::config::repository::SectionSchema::object([
            (
                "greeting",
                openrust::config::repository::SectionSchema::String,
            ),
            (
                "greeting_suffix",
                openrust::config::repository::SectionSchema::String,
            ),
            (
                "article_summary_origin",
                openrust::config::repository::SectionSchema::String.nullable(),
            ),
            (
                "logging_demo",
                openrust::config::repository::SectionSchema::Boolean,
            ),
            (
                "feature",
                openrust::config::repository::SectionSchema::object([(
                    "enabled",
                    openrust::config::repository::SectionSchema::Boolean,
                )]),
            ),
            (
                "trusted_hosts",
                openrust::config::repository::SectionSchema::object([
                    (
                        "at",
                        openrust::config::repository::SectionSchema::array(
                            openrust::config::repository::SectionSchema::String,
                        )
                        .nullable(),
                    ),
                    (
                        "subdomains",
                        openrust::config::repository::SectionSchema::Boolean,
                    ),
                ])
                .nullable(),
            ),
            #[cfg(feature = "auth-oidc")]
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
    Ok(())
}

fn register_greeting_attributes(app: &RegisterContext<'_>) -> Result<()> {
    app.when_has_attribute::<ExampleServedAt>(|_, _, _| {
        Box::pin(async {
            Ok(AttributeValue::One(ParameterValue::new(
                chrono::Utc::now().to_rfc3339(),
            )))
        })
    })?;
    app.after_resolving_attribute::<ExampleGreetingConstructed>(|event, _| {
        Box::pin(async move {
            if let AttributeValue::One(value) = event.value() {
                let _ = value.downcast::<Arc<ExamplePresenter>>()?;
                tracing::debug!(
                    target: "openrust.example.greeting.attribute",
                    "示例呈现器已完成构造"
                );
            }
            Ok(())
        })
    })
}

impl ServiceProvider for ExampleServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            register_example_schema(app)?;
            register_greeting_attributes(app)?;
            app.instance(Arc::new(ExampleResponseMarker)).await?;
            app.alias(
                ServiceKey::named(EXAMPLE_PRESENTER_CLASS),
                ServiceKey::named(EXAMPLE_PRESENTER_ALIAS),
            )?;
            app.bind_class::<dyn GreetingPresenter>(
                ServiceKey::typed::<dyn GreetingPresenter>(),
                ServiceKey::named(EXAMPLE_PRESENTER_CLASS),
            )
            .await?;
            app.after_resolving(Some(ServiceKey::typed::<dyn GreetingPresenter>()), |event, _| {
                Box::pin(async move {
                    if let Some(presenter) = event.value::<dyn GreetingPresenter>()? {
                        tracing::debug!(served_at = %presenter.served_at(), "示例问候呈现器已解析");
                    }
                    Ok(())
                })
            })?;
            #[cfg(feature = "auth-fortify")]
            register_fortify_views(app).await?;
            app.singleton_if::<ExampleMessage, _>(|resolver| {
                Box::pin(async move {
                    let config = resolver.section::<ExampleConfig>("example")?;
                    Ok(Arc::new(ExampleMessage::new(config.greeting)))
                })
            })
            .await?;
            app.alias(
                ServiceKey::typed::<ExampleMessage>(),
                ServiceKey::named(EXAMPLE_MESSAGE_ALIAS),
            )?;
            app.extend::<ExampleMessage, _>(|original, _resolver| {
                Box::pin(async move { Ok(Arc::new(original.with_binding_extension())) })
            })
            .await?;
            #[cfg(feature = "session")]
            app.extend::<openrust::session::ErrorBagCodecs, _>(|original, _resolver| {
                Box::pin(async move {
                    let mut codecs = (*original).clone();
                    codecs
                        .register::<crate::modules::example::services::draft_errors::DraftErrors>(
                            "example.draft-review",
                            1,
                            crate::modules::example::services::draft_errors::encode,
                            crate::modules::example::services::draft_errors::decode,
                        )?;
                    Ok(Arc::new(codecs))
                })
            })
            .await?;
            let database_enabled = match self.database_override {
                Some(enabled) => enabled,
                None => app
                    .config_repository()
                    .section::<openrust::config::DatabaseConfig>("database")?
                    .has_default_connection(),
            };
            self.database_selected_at_register
                .store(database_enabled, Ordering::Release);
            #[cfg(feature = "auth")]
            if database_enabled {
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
                })
                .await?;
            }
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
            })
            .await?;
            Ok(())
        })
    }

    #[allow(clippy::too_many_lines, reason = "模块 boot 同时装配业务资源和路由")]
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            #[cfg(feature = "hashing")]
            app.command(super::secret_hash::HashSecretFile);
            let database_enabled = match self.database_override {
                Some(enabled) => enabled,
                None => app
                    .section::<openrust::config::DatabaseConfig>("database")?
                    .has_default_connection(),
            };
            // 注册时未选择业务子项，boot 后加 URL 不能把可达候选冒充已登记依赖。
            if database_enabled && !self.database_selected_at_register.load(Ordering::Acquire) {
                return Err(openrust::Error::custom(
                    "example_database_selection_changed",
                    "示例模块在 register 后新增默认数据库连接，请重新 bootstrap 应用",
                ));
            }
            app.exceptions(|handler| {
                handler.or_api_envelope_when(|request, _| {
                    request.path() == "/api/example" || request.path().starts_with("/api/example/")
                });
            })?;
            if app
                .config_repository()
                .has("database.instances.reporting")?
            {
                app.routes(crate::modules::example::routes::reporting_connection)?;
            }
            register_example_middleware(app)?;
            register_lifecycle_diagnostics(app);
            app.terminating(|state| {
                Box::pin(async move {
                    tracing::trace!(
                        environment = state.environment().as_str(),
                        "示例应用入口已完成"
                    );
                    Ok(())
                })
            });
            let directory = if self.module_repository {
                app.resolve::<openrust_modules::ModuleRepository>()
                    .await?
                    .find_or_fail("example")?
                    .path()
                    .to_path_buf()
            } else {
                self.directory.clone()
            };
            overlay_example_config_and_register_resources(
                app,
                &self.root,
                &directory,
                self.runtime_policy_enabled,
            )
            .await?;
            #[cfg(feature = "auth")]
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
            let cookie_enabled = app.is_bound::<openrust::http::CookieFactory>();
            let i18n_enabled = app.is_bound::<openrust::translation::Translator>();
            let example_config = app.section::<ExampleConfig>("example")?;
            if let Some(trusted_hosts) = example_config.trusted_hosts.clone() {
                app.trust_hosts(trusted_hosts)?;
            }
            app.exceptions(|handler| {
                handler
                    .report::<crate::modules::example::http::controllers::example::ExampleDomainConflict, _>(
                        |_| openrust::foundation::ReportDecision::Stop,
                    )
                    .render::<crate::modules::example::http::controllers::example::ExampleDomainConflict, _>(
                        |_, _| {
                            use openrust::http::axum::response::IntoResponse;
                            Some((openrust::http::axum::http::StatusCode::CONFLICT,
                                openrust::http::axum::Json(serde_json::json!({"message": "示例业务冲突"})))
                                .into_response())
                        },
                    )
                    .respond(|mut response, _, request| {
                        if request.path() == "/api/example/exception" {
                            response.headers_mut().insert(
                                "x-example-exception",
                                openrust::http::axum::http::HeaderValue::from_static("rendered"),
                            );
                        }
                        response
                    });
            })?;
            let summary_origin = example_config.article_summary_origin;
            let logging_demo = example_config.logging_demo;
            #[cfg(feature = "session")]
            let session_enabled = session_capability(app);
            #[cfg(feature = "auth-oidc")]
            let oidc_config = example_config.oidc;
            #[cfg(feature = "auth-oidc")]
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
            #[cfg(feature = "auth-oidc")]
            let oidc_enabled = oidc_config.is_some();
            register_binding_extension_route(app).await?;
            #[cfg(feature = "auth-fortify")]
            let jwt_api_guard = if app.is_bound::<openrust::auth::fortify::FortifyJwtApi>() {
                Some(
                    app.section::<openrust::auth::fortify::FortifyJwtApiConfig>("fortify-api")?
                        .guard,
                )
            } else {
                None
            };
            let config_reading_repository = app.config_repository();
            app.routes(move |routes| {
                crate::modules::example::routes::register(
                    routes,
                    &crate::modules::example::routes::RouteCapabilities {
                        database: database_enabled,
                        cookie: cookie_enabled,
                        #[cfg(feature = "auth-fortify")]
                        jwt_api_guard: jwt_api_guard.clone(),
                        #[cfg(feature = "session")]
                        session: session_enabled.load(Ordering::Acquire),
                        view: view_enabled,
                        i18n: i18n_enabled,
                        logging_demo,
                        summary_origin: summary_origin.clone(),
                        #[cfg(feature = "auth-oidc")]
                        oidc: oidc_enabled,
                    },
                    config_reading_repository.clone(),
                )
            })?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod extension_tests;
