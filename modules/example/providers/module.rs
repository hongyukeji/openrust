//! 普通模块父 Provider：在唯一内核中顺序注册各自独立的业务子 Provider。

use super::{ExampleServiceProvider, module_state};
use crate::modules::example;
use openrust::Result;
use openrust::core::{BoxFuture, ProviderId, RegisterContext, ServiceProvider};
use std::path::PathBuf;
use std::sync::Arc;

/// 将示例模块的 Provider 清单纳入真正的嵌套注册生命周期。
pub(crate) struct ExampleModuleServiceProvider {
    uses_database: bool,
    application_root: PathBuf,
    module_repository: bool,
}

impl ExampleModuleServiceProvider {
    pub(crate) fn new(uses_database: bool, application_root: PathBuf) -> Self {
        Self {
            uses_database,
            module_repository: false,
            application_root,
        }
    }

    /// 普通发现路径在实际业务 boot 中通过同一模块仓储读取资源根。
    pub(crate) fn with_module_repository(mut self) -> Self {
        self.module_repository = true;
        self
    }

    #[cfg(any(feature = "queue", feature = "db-postgres", feature = "db-mysql"))]
    fn database_provider_ids(&self) -> Vec<ProviderId> {
        if !self.uses_database {
            return Vec::new();
        }
        let mut providers = Vec::new();
        #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
        providers.extend([
            ProviderId::of::<example::services::account_security::AccountSecurityProvider>(),
            ProviderId::of::<example::services::tenant::TenantServiceProvider>(),
        ]);
        #[cfg(all(
            feature = "auth-oidc",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        providers.push(ProviderId::of::<
            example::services::oidc::ExampleOidcServiceProvider,
        >());
        #[cfg(all(
            feature = "auth",
            feature = "data-exchange",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        providers.push(ProviderId::of::<
            example::services::article_transfer::ArticleTransferProvider,
        >());
        #[cfg(all(
            feature = "auth",
            feature = "storage-local",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        providers.push(ProviderId::of::<super::ArticleAttachmentsProvider>());
        #[cfg(feature = "queue")]
        providers.push(ProviderId::of::<example::jobs::JobsServiceProvider>());
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        providers.extend([
            ProviderId::of::<
                openrust::db::MigrationServiceProvider<example::database::migrations::Migrator>,
            >(),
            seed_provider_id(example::database::seeders::run),
        ]);
        providers
    }

    #[cfg(any(feature = "queue", feature = "db-postgres", feature = "db-mysql"))]
    async fn register_database_providers(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
        if self.uses_database {
            app.register_provider(example::services::account_security::AccountSecurityProvider)
                .await?;
            app.register_provider(example::services::tenant::TenantServiceProvider)
                .await?;
        }
        #[cfg(all(
            feature = "auth-oidc",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        if self.uses_database {
            app.register_provider(example::services::oidc::ExampleOidcServiceProvider)
                .await?;
        }
        #[cfg(all(
            feature = "auth",
            feature = "data-exchange",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        if self.uses_database {
            app.register_provider(example::services::article_transfer::ArticleTransferProvider)
                .await?;
        }
        #[cfg(all(
            feature = "auth",
            feature = "storage-local",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        if self.uses_database {
            app.register_provider(super::ArticleAttachmentsProvider)
                .await?;
        }
        #[cfg(feature = "queue")]
        if self.uses_database {
            app.register_provider(example::jobs::JobsServiceProvider)
                .await?;
        }
        #[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
        if self.uses_database {
            app.register_provider(
                openrust::db::MigrationServiceProvider::<example::database::migrations::Migrator>::new()
                    .for_module("example"),
            ).await?;
            app.register_provider(openrust::db::SeedServiceProvider::new(
                example::database::seeders::run,
            ))
            .await?;
        }
        Ok(())
    }
}

impl ServiceProvider for ExampleModuleServiceProvider {
    fn nested_providers(&self) -> Vec<ProviderId> {
        // Rust 无法反射异步 register 回调；这里只声明类型，不构造服务或执行注册。
        #[allow(unused_mut, reason = "可选子 Provider 仅在对应 feature 下加入声明")]
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
        #[cfg(feature = "realtime")]
        providers.push(ProviderId::of::<openrust::realtime::RealtimeServiceProvider>());
        #[cfg(any(
            test,
            all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
        ))]
        providers.push(ProviderId::of::<
            example::services::welcome::WelcomeServiceProvider,
        >());
        #[cfg(any(feature = "queue", feature = "db-postgres", feature = "db-mysql"))]
        providers.extend(self.database_provider_ids());
        providers
    }

    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // 只有活跃模块的父 register 会构造日志和事件监听器；descriptor 不创建运行状态。
            let state_log = Arc::new(module_state::ModuleStateLog::default());
            #[allow(unused_mut, reason = "文章监听器仅在适用的认证/数据库 feature 下追加")]
            let mut events = module_state::events_provider(Arc::clone(&state_log));
            #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
            if self.uses_database {
                example::events::article::register(&mut events);
            }
            app.register_provider(module_state::ModuleStateServiceProvider::new(state_log))
                .await?;
            app.register_provider(events).await?;
            let business = ExampleServiceProvider::new(self.application_root.clone());
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
            #[cfg(any(
                test,
                all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
            ))]
            app.register_provider(example::services::welcome::WelcomeServiceProvider)
                .await?;
            #[cfg(any(feature = "queue", feature = "db-postgres", feature = "db-mysql"))]
            self.register_database_providers(app).await?;
            #[cfg(not(any(feature = "queue", feature = "db-postgres", feature = "db-mysql")))]
            let _ = self.uses_database;
            Ok(())
        })
    }
}

// 函数 item 类型无法直接命名；只让编译器推导与 new(run) 完全相同的 F，不调用种子或构造服务。
#[cfg(any(feature = "db-postgres", feature = "db-mysql"))]
fn seed_provider_id<F: 'static>(_seed: F) -> ProviderId {
    ProviderId::of::<openrust::db::SeedServiceProvider<F>>()
}
