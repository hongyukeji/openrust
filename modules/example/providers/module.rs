//! 普通模块父 Provider：在唯一内核中顺序注册各自独立的业务子 Provider。

use super::{ExampleServiceProvider, module_state};
use crate::modules::example;
use openrust::Result;
use openrust::config::DatabaseConfig;
use openrust::foundation::ProviderId;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use std::path::PathBuf;
use std::sync::Arc;

/// 将示例模块的 Provider 清单纳入真正的嵌套注册生命周期。
pub(crate) struct ExampleModuleServiceProvider {
    database_override: Option<bool>,
    application_root: PathBuf,
    module_repository: bool,
}

impl ExampleModuleServiceProvider {
    /// 普通启动在真实 register 时读取当前默认连接配置，不捕获启动前的能力布尔值。
    pub(crate) fn from_config(application_root: PathBuf) -> Self {
        Self {
            database_override: None,
            module_repository: false,
            application_root,
        }
    }

    /// 程序化 fixture 可明确覆盖默认连接判定；不改变生产配置选择。
    #[cfg(test)]
    pub(crate) fn new(uses_database: bool, application_root: PathBuf) -> Self {
        Self {
            database_override: Some(uses_database),
            ..Self::from_config(application_root)
        }
    }

    /// 普通发现路径在实际业务 boot 中通过同一模块仓储读取资源根。
    pub(crate) fn with_module_repository(mut self) -> Self {
        self.module_repository = true;
        self
    }

    fn database_provider_ids(&self) -> Vec<ProviderId> {
        if self.database_override == Some(false) {
            return Vec::new();
        }
        vec![
            #[cfg(feature = "auth")]
            ProviderId::of::<example::services::account_security::AccountSecurityProvider>(),
            #[cfg(feature = "auth")]
            ProviderId::of::<example::services::tenant::TenantServiceProvider>(),
            #[cfg(feature = "auth-oidc")]
            ProviderId::of::<example::services::oidc::ExampleOidcServiceProvider>(),
            #[cfg(all(feature = "auth", feature = "data-exchange"))]
            ProviderId::of::<example::services::article_transfer::ArticleTransferProvider>(),
            #[cfg(all(feature = "auth", feature = "storage-local"))]
            ProviderId::of::<super::ArticleAttachmentsProvider>(),
        ]
    }

    #[cfg(feature = "auth")]
    async fn register_database_providers(app: &mut RegisterContext<'_>) -> Result<()> {
        #[cfg(feature = "auth")]
        {
            app.register_provider(example::services::account_security::AccountSecurityProvider)
                .await?;
            app.register_provider(example::services::tenant::TenantServiceProvider)
                .await?;
        }
        #[cfg(feature = "auth-oidc")]
        app.register_provider(example::services::oidc::ExampleOidcServiceProvider)
            .await?;
        #[cfg(all(feature = "auth", feature = "data-exchange"))]
        app.register_provider(example::services::article_transfer::ArticleTransferProvider)
            .await?;
        #[cfg(all(feature = "auth", feature = "storage-local"))]
        app.register_provider(super::ArticleAttachmentsProvider)
            .await?;
        Ok(())
    }
}

impl ServiceProvider for ExampleModuleServiceProvider {
    fn nested_providers(&self) -> Vec<ProviderId> {
        // Rust 无法反射异步 register；这里只声明始终会登记的类型，不构造服务。
        let mut providers = vec![
            ProviderId::of::<module_state::ModuleStateServiceProvider>(),
            ProviderId::of::<openrust::events::EventsServiceProvider>(),
            ProviderId::of::<ExampleServiceProvider>(),
            ProviderId::of::<example::extensions::PrefixCacheServiceProvider>(),
        ];
        #[cfg(feature = "grpc")]
        providers.extend([
            ProviderId::of::<openrust::grpc::GrpcServiceProvider>(),
            ProviderId::of::<example::grpc::ExampleGrpcServiceProvider>(),
        ]);
        #[cfg(feature = "queue")]
        providers.push(ProviderId::of::<example::jobs::JobsServiceProvider>());
        #[cfg(feature = "realtime")]
        providers.push(ProviderId::of::<openrust::realtime::RealtimeServiceProvider>());
        #[cfg(any(test, feature = "auth"))]
        providers.push(ProviderId::of::<
            example::services::welcome::WelcomeServiceProvider,
        >());
        providers.extend([
            ProviderId::of::<
                openrust::database::MigrationServiceProvider<
                    example::database::migrations::Migrator,
                >,
            >(),
            seed_provider_id(example::database::seeders::run),
        ]);
        #[cfg(feature = "translation-database")]
        providers.push(ProviderId::of::<
            example::services::translation_source::TranslationSourceExampleProvider,
        >());
        providers
    }

    fn possible_nested_providers(&self) -> Vec<ProviderId> {
        // 可达性候选不是必登记承诺；最终选择在 register 的同一配置仓储上求值。
        self.database_provider_ids()
    }

    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let uses_database = match self.database_override {
                Some(enabled) => enabled,
                None => app
                    .config_repository()
                    .section::<DatabaseConfig>("database")?
                    .has_default_connection(),
            };
            // 只有活跃模块的父 register 会构造日志和事件监听器；descriptor 不创建运行状态。
            let state_log = Arc::new(module_state::ModuleStateLog::default());
            #[allow(
                unused_mut,
                reason = "文章监听器仅在认证领域启用且已配置默认连接时追加"
            )]
            let mut events = module_state::events_provider(Arc::clone(&state_log));
            #[cfg(feature = "auth")]
            if uses_database {
                example::events::article::register(&mut events);
            }
            app.register_provider(module_state::ModuleStateServiceProvider::new(state_log))
                .await?;
            app.register_provider(events).await?;
            #[cfg(feature = "translation-database")]
            app.register_provider(
                example::services::translation_source::TranslationSourceExampleProvider,
            )
            .await?;
            let business = ExampleServiceProvider::new(self.application_root.clone())
                .with_database_override(self.database_override);
            let business = if self.module_repository {
                business.with_module_repository()
            } else {
                business
            };
            app.register_provider(business).await?;
            app.register_provider(example::extensions::PrefixCacheServiceProvider)
                .await?;
            #[cfg(feature = "grpc")]
            {
                app.register_provider(openrust::grpc::GrpcServiceProvider::new())
                    .await?;
                app.register_provider(example::grpc::ExampleGrpcServiceProvider)
                    .await?;
            }
            #[cfg(feature = "realtime")]
            app.register_provider(openrust::realtime::RealtimeServiceProvider::new())
                .await?;
            #[cfg(any(test, feature = "auth"))]
            app.register_provider(example::services::welcome::WelcomeServiceProvider)
                .await?;
            #[cfg(feature = "queue")]
            app.register_provider(example::jobs::JobsServiceProvider)
                .await?;
            #[cfg(feature = "auth")]
            if uses_database {
                Self::register_database_providers(app).await?;
            }
            #[cfg(not(feature = "auth"))]
            let _ = uses_database;
            // 迁移与种子贡献始终可见；SQL 只在用户实际执行相应命令时运行。
            app.register_provider(
                openrust::database::MigrationServiceProvider::<
                    example::database::migrations::Migrator,
                >::new()
                .for_module("example"),
            )
            .await?;
            app.register_provider(openrust::database::SeedServiceProvider::new(
                example::database::seeders::run,
            ))
            .await?;
            Ok(())
        })
    }
}

// 函数 item 类型无法直接命名；只让编译器推导与 new(run) 完全相同的 F，不调用种子或构造服务。
fn seed_provider_id<F: 'static>(_seed: F) -> ProviderId {
    ProviderId::of::<openrust::database::SeedServiceProvider<F>>()
}
