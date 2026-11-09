//! 模块装配声明的唯一 Provider 入口，不建立扩展自己的启动循环。

use crate::{ModuleManifest, ModuleRepository, ModulesConfig, module};
use openrust::Result;
use openrust::container::ServiceKey;
use openrust::foundation::BootContext;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::ProviderPreparation;
use openrust::foundation::ProviderRegistration;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use std::sync::Arc;

/// 已编译模块清单的预装配声明，展开时点早于所有 Provider 回调。
pub(crate) struct ModulesRegistration {
    manifest: ModuleManifest,
}
impl ModulesRegistration {
    /// 使用同一已编译清单，不在此读取资源或启动 Provider。
    fn new(manifest: ModuleManifest) -> Self {
        Self { manifest }
    }
}
impl ProviderRegistration for ModulesRegistration {
    fn prepare(
        self: Box<Self>,
        context: &mut ProviderPreparation<'_>,
    ) -> Result<Vec<Box<dyn ServiceProvider>>> {
        if self.manifest.has_discovery() {
            let provider = self.manifest.into_discovery_provider().ok_or_else(|| {
                openrust::Error::custom("module_manifest_invalid", "运行发现清单缺少编译绑定表")
            })?;
            return if context.is_provider_removed(provider.id()) {
                Ok(Vec::new())
            } else {
                Ok(vec![Box::new(provider)])
            };
        }
        let prepared = self.manifest.prepare_runtime()?;
        let seed = Arc::new(prepared.seed);
        context.singleton::<ModuleRepository, _>(move |resolver| {
            let seed = Arc::clone(&seed);
            Box::pin(async move { Ok(Arc::new(ModuleRepository::from_seed(seed, resolver))) })
        });
        for descriptor in prepared.modules {
            for provider in descriptor.providers {
                context.append_provider(provider);
            }
        }
        Ok(Vec::new())
    }
}

pub(crate) fn register_repository_alias(app: &mut RegisterContext<'_>) -> Result<()> {
    app.alias(
        ServiceKey::typed::<ModuleRepository>(),
        ServiceKey::named("modules"),
    )
}

/// 在普通 Provider 清单安装模块；清单重配复用核心声明替换与槽位规则。
pub trait ModuleProviderBuilderExt {
    /// 选择发现或描述符清单，并在 bootstrap 组安装模块配置与命令。
    fn module_manifest(&mut self, manifest: ModuleManifest) -> &mut Self;
    /// 仅对当前构建禁用模块，不改持久激活器；不移动已有声明槽位。
    /// 未安装模块扩展时不执行操作，不隐式加入配置、事件、仓储或命令。
    fn remove_module(&mut self, name: impl Into<String>) -> &mut Self;
}
impl ModuleProviderBuilderExt for ProviderBuilder {
    fn module_manifest(&mut self, manifest: ModuleManifest) -> &mut Self {
        self.bootstrap_provider(ModulesCommandsProvider);
        self.registration(ModulesRegistration::new(manifest))
    }
    fn remove_module(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        if let Some(registration) = self.registration_mut::<ModulesRegistration>() {
            registration.manifest.disable(name);
        }
        self
    }
}

/// 扩展的配置与命令 Provider；框架核心不再识别模块命令或模块配置。
pub(crate) struct ModulesCommandsProvider;
impl ServiceProvider for ModulesCommandsProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use openrust::config::repository::SectionSchema as S;
            app.config_section(
                "modules",
                ModulesConfig::default(),
                S::object([
                    ("paths", S::object([("modules", S::String)]).open()),
                    (
                        "scan",
                        S::object([("enabled", S::Boolean), ("paths", S::array(S::String))]).open(),
                    ),
                    ("activator", S::String),
                    (
                        "activators",
                        S::object([("file", S::object([("statuses-file", S::String)]).open())])
                            .open(),
                    ),
                ])
                .open(),
                |_: &ModulesConfig| Ok(()),
            )?;
            // 编译清单在预装配时已登记仓储；运行发现由自己的 Provider 登记。
            if app.is_bound::<ModuleRepository>() {
                register_repository_alias(app)?;
            }
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.is_bound::<ModuleRepository>() {
                module::register_commands(app);
            }
            Ok(())
        })
    }
}
