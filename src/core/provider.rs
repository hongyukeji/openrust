//! 统一 `ServiceProvider` 的注册、配置与启动生命周期。

use std::any::{Any, TypeId, type_name};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use figment::Figment;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::time::Instant;

use crate::config::repository::{ConfigRepository, SectionSchema};
use crate::config::{ConfigError, ConfigSources, FrameworkConfig};

use super::command::Command;
use super::container::{Container, ContainerBuilder, Resolver};
use super::module::{ModuleManifest, ModuleRepository, ModuleRepositorySeed};
use super::resources::{ResourceOwner, ResourceRegistrar};
use super::{BoxFuture, Error, Result};

pub(crate) mod registration;

pub(crate) type Extensions = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

/// `TestAppBuilder` 启动身份；只有最终 Testing 环境可使用 CSRF 测试豁免。
pub(crate) struct TestingBoot;
pub(crate) type Required = Box<dyn Fn(&Container) -> BoxFuture<'_, Result<()>> + Send + Sync>;
pub(crate) type Finalizer = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;
type AppConfigRegistration = Box<dyn FnOnce(&mut ContainerBuilder, ConfigRepository) + Send>;
type BindingOverride = Box<dyn FnOnce(Container) -> BoxFuture<'static, Result<()>> + Send>;
type DeferredBootValidator = Box<dyn Fn() -> BoxFuture<'static, Result<()>> + Send + Sync>;

pub(crate) struct DeferredBootCheck {
    pub(crate) permitted_command: TypeId,
    pub(crate) validate: DeferredBootValidator,
}

#[derive(Default)]
pub(crate) struct DeferredBootChecks(pub(crate) Vec<DeferredBootCheck>);

/// Provider 的具体 Rust 类型身份；错误消息只公开类型名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId {
    pub(crate) type_id: TypeId,
    name: &'static str,
}

impl ProviderId {
    /// 从具体 Provider 类型建立身份。
    pub fn of<T: 'static + ?Sized>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            name: type_name::<T>(),
        }
    }

    /// 用于不包含用户配置值的诊断。
    pub fn name(self) -> &'static str {
        self.name
    }
}

/// 按清单顺序注册绑定，再按同一顺序启动贡献的扩展入口。
pub trait ServiceProvider: Send + Sync + 'static {
    /// 具体类型身份；不以盒装 trait 对象身份去重。
    fn id(&self) -> ProviderId {
        ProviderId::of::<Self>()
    }

    /// 声明必须先于当前 Provider 出现在清单中的依赖。
    fn dependencies(&self) -> Vec<ProviderId> {
        Vec::new()
    }

    /// 声明需参与静态依赖预检的子 Provider 身份，不自动执行它们的生命周期。
    ///
    /// 声明必须无副作用；register 返回前须实际登记或复用这些子项。
    /// Rust 无法在执行异步回调前反射其注册类型；没有外部类型依赖时可保留空声明。
    fn nested_providers(&self) -> Vec<ProviderId> {
        Vec::new()
    }

    /// 仅登记绑定、配置 schema 与默认值，不解析服务或贡献路由。
    fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// 所有 Provider 注册且基础配置验证后依序启动。
    fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Provider 注册阶段可访问的受限接口。
pub struct RegisterContext<'a> {
    container: &'a Container,
    config: &'a mut ConfigRepository,
    required: &'a mut Vec<Required>,
    owner: ProviderId,
    force: bool,
    registry: &'a mut registration::ProviderRegistry,
    live_boot: Option<registration::LiveBoot<'a>>,
}

impl<'a> RegisterContext<'a> {
    pub(crate) fn live(
        container: &'a Container,
        config: &'a mut ConfigRepository,
        required: &'a mut Vec<Required>,
        owner: ProviderId,
        force: bool,
        registry: &'a mut registration::ProviderRegistry,
        live_boot: Option<registration::LiveBoot<'a>>,
    ) -> Self {
        Self {
            container,
            config,
            required,
            owner,
            force,
            registry,
            live_boot,
        }
    }
}

impl RegisterContext<'_> {
    /// 即时注册子 Provider；返回前完成其 register，启动阶段不提前 boot。
    pub async fn register_provider<P: ServiceProvider>(
        &mut self,
        provider: P,
    ) -> Result<ProviderId> {
        self.boxed_register_provider(Box::new(provider), false)
            .await
    }

    /// 强制注册子 Provider；成功替换原槽位，失败保留旧实例和已发布贡献。
    pub async fn force_register_provider<P: ServiceProvider>(
        &mut self,
        provider: P,
    ) -> Result<ProviderId> {
        self.boxed_register_provider(Box::new(provider), true).await
    }

    /// 在同一注册执行器处理盒装子 Provider；不创建另一容器或独立 boot 管线。
    pub async fn boxed_register_provider(
        &mut self,
        provider: Box<dyn ServiceProvider>,
        force: bool,
    ) -> Result<ProviderId> {
        registration::register(
            self.registry,
            provider,
            force,
            self.container,
            self.config,
            self.required,
            self.live_boot
                .as_mut()
                .map(registration::LiveBoot::reborrow),
        )
        .await
    }

    /// 读取同一次注册过程的配置仓储；前序 register 写入立即可见。
    pub fn config_repository(&self) -> ConfigRepository {
        self.config.clone()
    }

    /// 判断已有 Provider 是否已登记服务，供默认绑定遵循应用覆盖顺序。
    pub fn is_bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.container.contains::<T>()
    }

    /// 当前应用的MIME检测登记器；Provider晚注册的后续请求立即可见。
    pub async fn mime_types(&self) -> Result<Arc<crate::http::MimeTypes>> {
        self.container.resolve::<crate::http::MimeTypes>().await
    }

    /// 判断原始来源是否声明区段，供可选领域 Provider 选择是否安装。
    pub fn source_has_section(&self, key: &str) -> Result<bool> {
        self.config.source_has_section(key).map_err(Error::from)
    }

    /// 每次解析构造新实例。
    pub async fn bind<T, F>(&mut self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.container.bind(factory).await
    }

    /// 在应用内共享成功构造的实例。
    pub async fn singleton<T, F>(&mut self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.container.singleton(factory).await
    }

    /// 在一个请求、任务或命令 scope 内共享实例。
    pub async fn scoped<T, F>(&mut self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.container.scoped(factory).await
    }

    /// 登记已存在的共享实例。
    pub async fn instance<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        value: Arc<T>,
    ) -> Result<()> {
        self.container.instance(value).await
    }

    /// 保留原 factory 并登记装饰；显式 instance 的异步链在所有 register 后、boot 前执行。
    pub async fn extend<T, F>(&mut self, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.container.extend::<T, F>(extender).await
    }

    /// 登记新缓存发布后的通知；通知可解析同型服务。
    pub async fn rebinding<T, F>(&mut self, callback: F) -> Result<Option<Arc<T>>>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.container.rebinding::<T, F>(callback).await
    }

    /// 清除持久装饰链；不撤销显式 instance 的即时链。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&mut self) -> Result<()> {
        self.container.forget_extenders::<T>().await
    }

    /// 声明启动完成前必须成功解析的服务根。
    pub fn require<T: ?Sized + Send + Sync + 'static>(&mut self) {
        self.required.push(Box::new(|container| {
            Box::pin(async move { container.resolve::<T>().await.map(|_| ()) })
        }));
    }

    /// 登记配置区段的类型、默认值与领域校验器。
    pub fn config_section<T>(
        &mut self,
        key: &'static str,
        defaults: T,
        schema: SectionSchema,
        validate: fn(&T) -> Result<()>,
    ) -> Result<()>
    where
        T: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        self.config.register_section_for(
            self.owner.type_id,
            self.force,
            key,
            defaults,
            schema,
            validate,
        )
    }

    /// 排入 Laravel 包配置的浅层默认合并。
    pub fn merge_defaults(&mut self, key: &'static str, defaults: Value) -> Result<()> {
        self.config.merge_defaults_for(key, defaults)
    }

    /// 排入 Laravel 包配置的递归默认合并。
    pub fn replace_defaults_recursively(
        &mut self,
        key: &'static str,
        defaults: Value,
    ) -> Result<()> {
        self.config.replace_defaults_recursively_for(key, defaults)
    }
}

/// Provider 启动阶段的解析、模块覆盖和领域贡献入口。
pub struct BootContext<'a> {
    pub(crate) console_command: Option<&'a str>,
    pub(crate) container: &'a Container,
    pub(crate) config: &'a mut ConfigRepository,
    pub(crate) resources: ResourceRegistrar,
    pub(crate) required: &'a mut Vec<Required>,
    pub(crate) extensions: &'a mut Extensions,
    pub(crate) commands: &'a mut Vec<Box<dyn Command>>,
    pub(crate) finalizers: &'a mut Vec<Finalizer>,
}

impl BootContext<'_> {
    /// 返回本次启动显式指定的控制台命令；普通应用引导与测试默认为 `None`。
    pub fn console_command(&self) -> Option<&str> {
        self.console_command
    }

    /// 判断引导前的候选命令名是否匹配；实际派发命令尚未完成匹配。
    pub fn is_console_command(&self, name: &str) -> bool {
        self.console_command == Some(name)
    }

    /// 在候选迁移引导中保留严格检查，供实际命令派发前执行。
    #[cfg(any(feature = "auth-mfa", test))]
    pub(crate) fn defer_boot_check_for_command<F>(
        &mut self,
        permitted_command: TypeId,
        validate: F,
    ) -> Result<()>
    where
        F: Fn() -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.extension_mut::<DeferredBootChecks>()?
            .0
            .push(DeferredBootCheck {
                permitted_command,
                validate: Box::new(validate),
            });
        Ok(())
    }

    /// 在全部 boot 和配置覆盖结束后，要求根服务成功解析。
    pub fn require<T: ?Sized + Send + Sync + 'static>(&mut self) {
        self.required.push(Box::new(|container| {
            Box::pin(async move { container.resolve::<T>().await.map(|_| ()) })
        }));
    }

    /// 在全部 Provider boot/overlay 后执行最终领域配置校验。
    pub fn validate_at_finalize<F>(&mut self, validate: F)
    where
        F: FnOnce() -> BoxFuture<'static, Result<()>> + Send + 'static,
    {
        self.finalizers.push(Box::new(validate));
    }

    /// 检查 register 阶段是否登记了指定服务。
    pub fn is_bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.container.contains::<T>()
    }

    /// 解析已封口的服务绑定。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&mut self) -> Result<Arc<T>> {
        self.container.resolve::<T>().await
    }

    /// 等待当前缓存装饰成功后返回；失败不替换旧缓存。
    pub async fn extend<T, F>(&mut self, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.container.extend::<T, F>(extender).await
    }

    /// 清除未来构造的持久装饰链，不撤销当前缓存。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&mut self) -> Result<()> {
        self.container.forget_extenders::<T>().await
    }

    /// 读取当前已发布配置视图。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        self.config.section(key).map_err(Error::from)
    }

    /// 共享配置仓储句柄，供最终校验读取全部 boot 覆盖后的配置。
    pub fn config_repository(&self) -> ConfigRepository {
        self.config.clone()
    }

    /// 用本次应用引导的环境快照应用模块 Rust 配置清单；缓存命中时跳过函数。
    pub fn code_config(&mut self, manifest: &crate::config::ConfigManifest) -> Result<()> {
        self.config
            .apply_code_manifest(manifest, self.console_command)
    }

    /// 按模块启动顺序递归覆盖一个配置区段。
    pub fn overlay(&mut self, key: &str, value: Value) -> Result<()> {
        self.config.overlay(key, value)
    }

    /// 登记需要在关闭时逆序执行的异步回调或受管任务。
    pub fn resources(&self) -> &ResourceRegistrar {
        &self.resources
    }

    /// 贡献命令；同名命令由后启动的 Provider 替换。
    pub fn command(&mut self, command: impl Command) {
        if let Some(index) = self
            .commands
            .iter()
            .position(|existing| existing.name() == command.name())
        {
            self.commands[index] = Box::new(command);
        } else {
            self.commands.push(Box::new(command));
        }
    }

    /// 取得领域贡献登记器；不能通过它重绑容器服务。
    pub fn extension_mut<T: Default + Send + Sync + 'static>(&mut self) -> Result<&mut T> {
        let entry = self
            .extensions
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()));
        entry
            .downcast_mut::<T>()
            .ok_or_else(|| Error::custom("extension_type_mismatch", "领域贡献类型内部不匹配"))
    }
}

/// 一次完成的 Provider 生命周期；关闭资源须显式等待。
pub struct ProviderRuntime {
    pub(crate) container: Container,
    pub(crate) config: ConfigRepository,
    pub(crate) framework: FrameworkConfig,
    pub(crate) resources: ResourceOwner,
    pub(crate) extensions: Extensions,
    pub(crate) commands: Vec<Box<dyn Command>>,
    pub(crate) providers: registration::ProviderRegistry,
}

impl ProviderRuntime {
    /// 唯一服务容器。
    pub fn container(&self) -> &Container {
        &self.container
    }

    /// 最终配置视图。
    pub fn config(&self) -> &ConfigRepository {
        &self.config
    }

    /// 按具体类型读取领域贡献。
    pub fn extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// 按登记顺序读取命令贡献。
    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.commands.iter().map(AsRef::as_ref)
    }

    /// 拒绝新登记并限时回收受管任务与资源。
    pub async fn shutdown(&self, deadline: Instant) -> Result<()> {
        self.resources.shutdown(deadline).await
    }
}

/// 静态 Provider 清单与唯一启动引擎。
#[derive(Default)]
pub struct ProviderBuilder {
    console_command: Option<String>,
    testing_boot: bool,
    providers: Vec<ProviderEntry>,
    default_providers: Option<(usize, crate::core::DefaultProviders)>,
    modules: ModuleManifest,
    modules_configured: bool,
    removed: HashSet<ProviderId>,
    app_config: Option<AppConfigRegistration>,
    binding_overrides: Vec<BindingOverride>,
}

struct ProviderEntry {
    provider: Box<dyn ServiceProvider>,
    force: bool,
}

pub(crate) fn check_dependencies(
    provider: &dyn ServiceProvider,
    ready: &HashSet<TypeId>,
) -> Result<()> {
    for dependency in provider.dependencies() {
        if !ready.contains(&dependency.type_id) {
            return Err(Error::custom(
                "provider_order_invalid",
                format!(
                    "{} 必须在 {} 之后启动",
                    provider.id().name(),
                    dependency.name()
                ),
            ));
        }
    }
    Ok(())
}

fn ordered_providers(providers: &[ProviderEntry], removed: &HashSet<ProviderId>) -> Result<()> {
    let all: HashSet<_> = providers
        .iter()
        .filter(|entry| !removed.contains(&entry.provider.id()))
        .flat_map(|entry| {
            std::iter::once(entry.provider.id()).chain(entry.provider.nested_providers())
        })
        .filter(|id| !removed.contains(id))
        .map(|id| id.type_id)
        .collect();
    let mut ready = HashSet::new();
    for entry in providers {
        let provider = &entry.provider;
        if removed.contains(&provider.id()) {
            continue;
        }
        if !entry.force && ready.contains(&provider.id().type_id) {
            continue;
        }
        for dependency in provider.dependencies() {
            if !all.contains(&dependency.type_id) {
                return Err(Error::custom(
                    "provider_dependency_missing",
                    format!("{} 缺少依赖 {}", provider.id().name(), dependency.name()),
                ));
            }
            if !ready.contains(&dependency.type_id) {
                return Err(Error::custom(
                    "provider_order_invalid",
                    format!(
                        "{} 必须在 {} 之后启动",
                        provider.id().name(),
                        dependency.name()
                    ),
                ));
            }
        }
        ready.extend(
            provider
                .nested_providers()
                .into_iter()
                .filter(|id| !removed.contains(id))
                .map(|id| id.type_id),
        );
        ready.insert(provider.id().type_id);
    }
    Ok(())
}

struct StartupResources {
    owner: Option<ResourceOwner>,
    container: Option<Container>,
}

impl StartupResources {
    fn new() -> Self {
        Self {
            owner: Some(ResourceOwner::new()),
            container: None,
        }
    }

    fn owner(&self) -> Result<&ResourceOwner> {
        self.owner.as_ref().ok_or_else(|| {
            Error::custom("provider_resources_transferred", "启动资源所有权已经交接")
        })
    }

    fn attach_container(&mut self, container: &Container) {
        self.container = Some(container.clone());
    }

    async fn shutdown(&self, deadline: Instant) -> Result<()> {
        let Some(container) = &self.container else {
            return self.owner()?.shutdown(deadline).await;
        };
        container.seal_root();
        self.owner()?.seal();
        let (root, provider) = tokio::join!(
            container.shutdown_root(deadline),
            self.owner()?.shutdown(deadline)
        );
        match (root, provider) {
            (Err(error), Err(cleanup_error)) => {
                tracing::error!(
                    code = cleanup_error.code(),
                    "provider resource cleanup also failed"
                );
                Err(error)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), result) => result,
        }
    }

    fn into_owner(mut self) -> Result<ResourceOwner> {
        self.container = None;
        self.owner.take().ok_or_else(|| {
            Error::custom("provider_resources_transferred", "启动资源所有权已经交接")
        })
    }
}

impl Drop for StartupResources {
    fn drop(&mut self) {
        let Some(owner) = self.owner.take() else {
            return;
        };
        let container = self.container.take();
        if let Some(container) = &container {
            container.seal_root();
        }
        owner.seal();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let deadline = Instant::now() + Duration::from_secs(5);
                let (root, provider) = if let Some(container) = container {
                    tokio::join!(
                        async move { container.shutdown_root(deadline).await },
                        owner.shutdown(deadline)
                    )
                } else {
                    (Ok(()), owner.shutdown(deadline).await)
                };
                for error in [root.err(), provider.err()].into_iter().flatten() {
                    tracing::error!(
                        code = error.code(),
                        "cancelled provider startup cleanup failed"
                    );
                }
            });
        } else {
            tracing::error!("cancelled provider startup has no Tokio runtime for resource cleanup");
        }
    }
}

impl ProviderBuilder {
    /// 创建空清单。
    pub fn new() -> Self {
        Self::default()
    }

    /// 在当前清单槽位安装框架默认 Provider，启动时从同一配置仓储求值。
    pub fn default_providers(&mut self, defaults: crate::core::DefaultProviders) -> &mut Self {
        self.default_providers = Some((self.providers.len(), defaults));
        self
    }

    /// 指定本次启动实际要执行的控制台命令；普通引导默认无命令上下文。
    ///
    /// 调用方须用同一名称随后执行 [`crate::core::App::run_command`]。
    pub(crate) fn console_command(&mut self, name: impl Into<String>) -> &mut Self {
        self.console_command = Some(name.into());
        self
    }

    /// 仅测试应用启动器可设置；普通 HTTP 启动不能由请求伪造此事实。
    pub(crate) fn testing_boot(&mut self) -> &mut Self {
        self.testing_boot = true;
        self
    }

    /// 添加 Provider；同具体类型重复登记保留第一项。
    pub fn provider(&mut self, provider: impl ServiceProvider) -> &mut Self {
        self.providers.push(ProviderEntry {
            provider: Box::new(provider),
            force: false,
        });
        self
    }

    /// 加入已盒装的应用 Provider，保留具体类型身份。
    pub fn boxed_provider(&mut self, provider: Box<dyn ServiceProvider>) -> &mut Self {
        self.providers.push(ProviderEntry {
            provider,
            force: false,
        });
        self
    }

    /// 显式重注册同类型 Provider；保留首次 boot 槽位。
    pub fn force_provider(&mut self, provider: impl ServiceProvider) -> &mut Self {
        self.boxed_force_provider(Box::new(provider))
    }

    /// 显式重注册已盒装 Provider。
    pub fn boxed_force_provider(&mut self, provider: Box<dyn ServiceProvider>) -> &mut Self {
        self.providers.push(ProviderEntry {
            provider,
            force: true,
        });
        self
    }

    /// 在启动前从清单移除具体 Provider 类型；依赖关系随后重新校验。
    pub fn remove_provider(&mut self, id: ProviderId) -> &mut Self {
        self.providers.retain(|entry| entry.provider.id() != id);
        self.removed.insert(id);
        self
    }

    /// 声明已编译模块和本次启动的启用来源；与应用 Provider 共用一个生命周期。
    pub fn module_manifest(&mut self, manifest: ModuleManifest) -> &mut Self {
        self.modules = manifest;
        self.modules_configured = true;
        self
    }

    /// 仅从本次应用构建移除模块，不改写持久启用状态。
    pub fn remove_module(&mut self, name: impl Into<String>) -> &mut Self {
        self.modules.disable(name);
        self
    }

    /// 全体 Provider 注册后、boot 前覆盖类型化服务绑定。
    ///
    /// 测试 fake 使用与生产相同的容器和启动顺序；回调只运行一次。
    pub fn configure_after_register<F>(&mut self, configure: F) -> &mut Self
    where
        F: FnOnce(Container) -> BoxFuture<'static, Result<()>> + Send + 'static,
    {
        self.binding_overrides.push(Box::new(configure));
        self
    }

    /// 在同一容器中登记应用自身配置，首次解析读取 boot 后最终视图。
    pub fn app_config<T: DeserializeOwned + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.app_config = Some(Box::new(|bindings, config| {
            bindings.singleton::<T, _>(move |_| {
                let config = config.clone();
                Box::pin(async move {
                    config
                        .snapshot()
                        .extract::<T>()
                        .map(Arc::new)
                        .map_err(|error| Error::Config(error.into()))
                })
            });
        }));
        self
    }

    /// 完成全体 register、基础配置、顺序 boot 与最终根解析。
    #[allow(
        clippy::too_many_lines,
        reason = "统一启动顺序保持 register、配置和 boot 生命周期可审阅"
    )]
    pub async fn start(self, sources: impl Into<ConfigSources>) -> Result<ProviderRuntime> {
        let console_command = self.console_command;
        let mut providers = self.providers;
        let defaults = self.default_providers;
        let mut bindings = ContainerBuilder::new();
        bindings.instance(Arc::new(crate::http::MimeTypes::new()));
        let mut config = ConfigRepository::new(sources);
        if let Some((position, defaults)) = defaults {
            let entries = defaults
                .resolve(&config)?
                .into_iter()
                .filter(|provider| !self.removed.contains(&provider.id()))
                .map(|provider| ProviderEntry {
                    provider,
                    force: false,
                });
            providers.splice(position..position, entries);
        }
        let module_seed = Arc::new(Self::append_modules(
            self.modules,
            &self.removed,
            &mut providers,
        )?);
        if self.modules_configured {
            let events = crate::events::EventsServiceProvider::new();
            // 独立默认类型只补空服务，不占用 nested 真实 EventsServiceProvider 的去重身份。
            if !self.removed.contains(&events.id())
                && !providers
                    .iter()
                    .any(|entry| entry.provider.id() == events.id())
            {
                providers.insert(
                    0,
                    ProviderEntry {
                        provider: Box::new(crate::events::DefaultEventsServiceProvider),
                        force: false,
                    },
                );
            }
            bindings.singleton::<ModuleRepository, _>(move |resolver| {
                let seed = Arc::clone(&module_seed);
                Box::pin(async move { Ok(Arc::new(ModuleRepository::from_seed(seed, resolver))) })
            });
        }
        ordered_providers(&providers, &self.removed)?;
        let mut resources = StartupResources::new();
        if let Some(register_config) = self.app_config {
            register_config(&mut bindings, config.clone());
        }
        bindings.context(config.clone(), resources.owner()?.registrar());
        let container = bindings.build();
        resources.attach_container(&container);
        let mut required = Vec::new();
        let mut active = registration::ProviderRegistry::new(self.removed.clone());
        let mut extensions = Extensions::new();
        if self.testing_boot {
            extensions.insert(TypeId::of::<TestingBoot>(), Box::new(TestingBoot));
        }
        let mut commands = Vec::new();
        let mut finalizers = Vec::new();
        let result = async {
            for entry in providers {
                registration::register(
                    &mut active,
                    entry.provider,
                    entry.force,
                    &container,
                    &mut config,
                    &mut required,
                    None,
                )
                .await?;
            }
            for configure in self.binding_overrides {
                configure(container.clone()).await?;
            }
            config.validate_base()?;
            container.initialize_extensions().await?;
            for provider in active.iter() {
                provider
                    .boot(&mut BootContext {
                        console_command: console_command.as_deref(),
                        container: &container,
                        config: &mut config,
                        resources: resources.owner()?.registrar(),
                        required: &mut required,
                        extensions: &mut extensions,
                        commands: &mut commands,
                        finalizers: &mut finalizers,
                    })
                    .await?;
            }
            finalize_startup(&mut config, finalizers, required, &container).await
        }
        .await;
        let framework = match result {
            Ok(framework) => framework,
            Err(error) => {
                let shutdown = resources
                    .shutdown(Instant::now() + Duration::from_secs(5))
                    .await;
                if let Err(cleanup_error) = shutdown {
                    tracing::error!(
                        code = cleanup_error.code(),
                        "provider startup cleanup failed"
                    );
                }
                drop(resources.into_owner()?);
                return Err(error);
            }
        };
        Ok(ProviderRuntime {
            container,
            config,
            framework,
            resources: resources.into_owner()?,
            extensions,
            commands,
            providers: active,
        })
    }

    fn append_modules(
        modules: ModuleManifest,
        removed: &HashSet<ProviderId>,
        providers: &mut Vec<ProviderEntry>,
    ) -> Result<ModuleRepositorySeed> {
        let prepared = modules.prepare_runtime()?;
        for module in prepared.modules {
            for provider in module.providers {
                let id = provider.id();
                if removed.contains(&id) {
                    continue;
                }
                providers.push(ProviderEntry {
                    provider,
                    force: false,
                });
            }
        }
        Ok(prepared.seed)
    }
}

async fn finalize_startup(
    config: &mut ConfigRepository,
    finalizers: Vec<Finalizer>,
    required: Vec<Required>,
    container: &Container,
) -> Result<FrameworkConfig> {
    config.freeze()?;
    for validate in finalizers {
        validate().await?;
    }
    let framework: FrameworkConfig = config
        .snapshot()
        .extract()
        .map_err(|error| Error::Config(ConfigError::from(error)))?;
    framework.app.resolved_timezone()?;
    for resolve in required {
        resolve(container).await?;
    }
    Ok(framework)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::core::{ModuleConfigFiles, ModuleDescriptor, ModuleManifest};

    type ConsoleObservation = (Option<String>, bool);
    type ConsoleObservations = Arc<Mutex<Vec<ConsoleObservation>>>;

    struct ConsoleProbe(ConsoleObservations);

    impl ServiceProvider for ConsoleProbe {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.lock().unwrap().push((
                    app.console_command().map(str::to_owned),
                    app.is_console_command("migrate"),
                ));
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn console_command_is_explicit_and_default_start_has_no_command() -> Result<()> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        for name in [None, Some("migrate"), Some("serve"), Some("migrate custom")] {
            let mut builder = ProviderBuilder::new();
            if let Some(name) = name {
                builder.console_command(name);
            }
            builder.provider(ConsoleProbe(Arc::clone(&seen)));
            let source = Figment::from(figment::providers::Serialized::defaults(
                FrameworkConfig::default(),
            ));
            builder
                .start(source)
                .await?
                .shutdown(Instant::now() + Duration::from_secs(1))
                .await?;
        }
        assert_eq!(
            *seen.lock().unwrap(),
            [
                (None, false),
                (Some("migrate".to_owned()), true),
                (Some("serve".to_owned()), false),
                (Some("migrate custom".to_owned()), false),
            ]
        );
        Ok(())
    }

    struct Base(Arc<Mutex<Vec<&'static str>>>);
    struct Overlay(Arc<Mutex<Vec<&'static str>>>);
    struct Observe(Arc<Mutex<Vec<&'static str>>>);

    impl ServiceProvider for Base {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.lock().unwrap().push("base.register");
                app.instance(Arc::new(41usize)).await?;
                app.config_section(
                    "service",
                    serde_json::json!({ "name": "base", "enabled": true }),
                    SectionSchema::any(),
                    |_| Ok(()),
                )
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(*app.resolve::<usize>().await?, 41);
                self.0.lock().unwrap().push("base.boot");
                Ok(())
            })
        }
    }

    impl ServiceProvider for Overlay {
        fn dependencies(&self) -> Vec<ProviderId> {
            vec![ProviderId::of::<Base>()]
        }

        fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.lock().unwrap().push("overlay.register");
                Ok(())
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(app.section::<Value>("service")?["name"], "base");
                app.overlay("service", serde_json::json!({"name": "module"}))?;
                self.0.lock().unwrap().push("overlay.boot");
                Ok(())
            })
        }
    }

    impl ServiceProvider for Observe {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(app.section::<Value>("service")?["name"], "module");
                self.0.lock().unwrap().push("observe.boot");
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn all_register_before_boot_and_later_boot_sees_overlay() -> Result<()> {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(Base(Arc::clone(&order)))
            .provider(Overlay(Arc::clone(&order)))
            .provider(Observe(Arc::clone(&order)));
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(
            *order.lock().unwrap(),
            [
                "base.register",
                "overlay.register",
                "base.boot",
                "overlay.boot",
                "observe.boot"
            ]
        );
        assert_eq!(
            runtime.config().section::<Value>("service")?["name"],
            "module"
        );
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }

    struct OverrideObserved(Arc<AtomicUsize>);

    impl ServiceProvider for OverrideObserved {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(String::from("production"))).await?;
                app.require::<String>();
                Ok(())
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(&*app.resolve::<String>().await?, "fake");
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn post_register_override_is_visible_to_boot_and_required_resolution() -> Result<()> {
        let boots = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(OverrideObserved(Arc::clone(&boots)));
        builder.configure_after_register(|container| {
            Box::pin(async move {
                assert!(container.contains::<String>());
                container.instance(Arc::new(String::from("fake"))).await?;
                Ok(())
            })
        });
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(boots.load(Ordering::SeqCst), 1);
        assert_eq!(&*runtime.container().resolve::<String>().await?, "fake");
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn removed_provider_does_not_register_and_dependency_is_rechecked() -> Result<()> {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(Base(Arc::clone(&order)));
        builder.provider(Overlay(Arc::clone(&order)));
        builder.remove_provider(ProviderId::of::<Base>());
        let error =
            builder.start(Figment::new()).await.err().ok_or_else(|| {
                Error::custom("expected_provider_error", "移除依赖后必须拒绝启动")
            })?;
        assert_eq!(error.code(), "provider_dependency_missing");
        assert!(order.lock().unwrap().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_order_stops_before_any_register() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder
            .provider(Overlay(Arc::clone(&order)))
            .provider(Base(Arc::clone(&order)));
        let result = builder.start(Figment::new()).await;
        assert!(matches!(result, Err(ref error) if error.code() == "provider_order_invalid"));
        assert!(order.lock().unwrap().is_empty());
    }

    struct LateFactory(Arc<AtomicUsize>);

    impl ServiceProvider for LateFactory {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.config_section(
                    "late",
                    serde_json::json!({"value": "base"}),
                    SectionSchema::any(),
                    |_| Ok(()),
                )?;
                let calls = Arc::clone(&self.0);
                app.singleton::<String, _>(move |resolver| {
                    let calls = Arc::clone(&calls);
                    Box::pin(async move {
                        let config: Value = resolver.section("late")?;
                        resolver.resources()?.on_shutdown(move || {
                            Box::pin(async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                Ok(())
                            })
                        })?;
                        Ok(Arc::new(
                            config["value"].as_str().unwrap_or_default().to_owned(),
                        ))
                    })
                })
                .await?;
                app.require::<String>();
                Ok(())
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("late", serde_json::json!({"value": "overlay"})) })
        }
    }

    #[tokio::test]
    async fn factory_reads_boot_overlay_and_owns_one_shutdown_callback() -> Result<()> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(LateFactory(Arc::clone(&calls)));
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(&*runtime.container().resolve::<String>().await?, "overlay");
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    struct ModuleBefore(Arc<Mutex<Vec<String>>>);
    struct ModuleSupport {
        root: std::path::PathBuf,
        reads: Arc<Mutex<Vec<String>>>,
    }
    struct ModuleAfter(Arc<Mutex<Vec<String>>>);

    impl ServiceProvider for ModuleBefore {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.config_section(
                "sample",
                serde_json::json!({"name":"Default","items":["a","b","c"],"settings":{"value":"default"}}),
                SectionSchema::any(),
                |_| Ok(()),
            )
            })
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let value: Value = app.section("sample")?;
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("before:{}", value["name"]));
                Ok(())
            })
        }
    }

    impl ServiceProvider for ModuleSupport {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let before: Value = app.section("sample")?;
                self.reads
                    .lock()
                    .unwrap()
                    .push(format!("support-before:{}", before["name"]));
                ModuleConfigFiles::new("sample", &self.root)?.overlay(app)?;
                let after: Value = app.section("sample")?;
                self.reads
                    .lock()
                    .unwrap()
                    .push(format!("support-after:{}", after["name"]));
                Ok(())
            })
        }
    }

    impl ServiceProvider for ModuleAfter {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let value: Value = app.section("sample")?;
                assert_eq!(value["name"], "Module");
                assert_eq!(value["items"], serde_json::json!(["x", "b", "c"]));
                assert_eq!(value["settings"]["value"], "settings");
                self.0.lock().unwrap().push("after".to_owned());
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn module_support_overlays_at_its_own_boot_point() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let config = temporary.path().join("config");
        std::fs::create_dir(&config).map_err(Error::internal)?;
        std::fs::write(
            config.join("config.toml"),
            "name = 'Module'\nitems = ['x']\n",
        )
        .map_err(Error::internal)?;
        std::fs::write(config.join("settings.toml"), "value = 'settings'\n")
            .map_err(Error::internal)?;
        let reads = Arc::new(Mutex::new(Vec::new()));
        let module = ModuleDescriptor::new("sample", temporary.path(), 0)
            .provider(ModuleBefore(Arc::clone(&reads)))
            .provider(ModuleSupport {
                root: temporary.path().to_path_buf(),
                reads: Arc::clone(&reads),
            })
            .provider(ModuleAfter(Arc::clone(&reads)));
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(ModuleManifest::new().module(module).enable("sample"));
        let source = Figment::from(figment::providers::Serialized::defaults(
            serde_json::json!({"sample":{"name":"App","items":["a","b","c"],"settings":{"value":"app"}}}),
        ));
        let runtime = builder.start(source).await?;
        assert_eq!(
            *reads.lock().unwrap(),
            [
                "before:\"App\"",
                "support-before:\"App\"",
                "support-after:\"Module\"",
                "after"
            ]
        );
        assert!(runtime.config().version() >= 2);
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn module_dependency_orders_provider_register_and_boot() -> Result<()> {
        struct Producer(Arc<Mutex<Vec<&'static str>>>);
        struct Consumer(Arc<Mutex<Vec<&'static str>>>);

        impl ServiceProvider for Producer {
            fn register<'a>(
                &'a self,
                _app: &'a mut RegisterContext<'_>,
            ) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    self.0
                        .lock()
                        .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                        .push("producer.register");
                    Ok(())
                })
            }

            fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
                let events = Arc::clone(&self.0);
                Box::pin(async move {
                    events
                        .lock()
                        .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                        .push("producer.boot");
                    Ok(())
                })
            }
        }

        impl ServiceProvider for Consumer {
            fn register<'a>(
                &'a self,
                _app: &'a mut RegisterContext<'_>,
            ) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    self.0
                        .lock()
                        .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                        .push("consumer.register");
                    Ok(())
                })
            }

            fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
                let events = Arc::clone(&self.0);
                Box::pin(async move {
                    events
                        .lock()
                        .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                        .push("consumer.boot");
                    Ok(())
                })
            }
        }

        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let producer_root = temporary.path().join("producer");
        let consumer_root = temporary.path().join("consumer");
        std::fs::create_dir(&producer_root).map_err(Error::internal)?;
        std::fs::create_dir(&consumer_root).map_err(Error::internal)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.module_manifest(
            ModuleManifest::new()
                .module(
                    ModuleDescriptor::new("consumer", &consumer_root, -100)
                        .requires("producer")
                        .provider(Consumer(Arc::clone(&events))),
                )
                .module(
                    ModuleDescriptor::new("producer", &producer_root, 100)
                        .provider(Producer(Arc::clone(&events))),
                )
                .enable("consumer")
                .enable("producer"),
        );

        let runtime = builder.start(Figment::new()).await?;
        let observed = {
            let events = events
                .lock()
                .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?;
            events.clone()
        };
        assert_eq!(
            observed,
            vec![
                "producer.register",
                "consumer.register",
                "producer.boot",
                "consumer.boot"
            ]
        );
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_runtime_powers_production_app_router() -> Result<()> {
        use tower::ServiceExt as _;

        let mut builder = ProviderBuilder::new();
        builder.provider(crate::http::HttpServiceProvider);
        let runtime = builder.start(Figment::new()).await?;
        let app = crate::core::App::from_provider_runtime(runtime)?;
        let router = crate::http::build_router(&app)?;
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(response.status(), http::StatusCode::OK);
        app.state()
            .shutdown_resources(Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }

    struct InvalidFinalConfig(Arc<AtomicUsize>);

    impl ServiceProvider for InvalidFinalConfig {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let calls = Arc::clone(&self.0);
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                app.overlay("server", serde_json::json!({"port": "invalid"}))
            })
        }
    }

    #[tokio::test]
    async fn final_framework_config_failure_cleans_boot_resources_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(InvalidFinalConfig(Arc::clone(&calls)));
        let result = builder.start(Figment::new()).await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    struct PendingStartup {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        cleaned: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicUsize>,
    }

    impl ServiceProvider for PendingStartup {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let cleaned = Arc::clone(&self.cleaned);
                let calls = Arc::clone(&self.calls);
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        cleaned.notify_one();
                        Ok(())
                    })
                })?;
                self.entered.notify_one();
                self.release.notified().await;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn cancelled_startup_runs_registered_cleanup() -> Result<()> {
        let entered = Arc::new(tokio::sync::Notify::new());
        let cleaned = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(PendingStartup {
            entered: Arc::clone(&entered),
            release: Arc::new(tokio::sync::Notify::new()),
            cleaned: Arc::clone(&cleaned),
            calls: Arc::clone(&calls),
        });
        let start = tokio::spawn(async move { builder.start(Figment::new()).await });
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .map_err(Error::internal)?;
        start.abort();
        let _ = start.await;
        tokio::time::timeout(Duration::from_secs(1), cleaned.notified())
            .await
            .map_err(Error::internal)?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    struct RegistrationProbe {
        value: usize,
        events: Arc<Mutex<Vec<String>>>,
        fail_register: bool,
        fail_boot: bool,
    }

    impl ServiceProvider for RegistrationProbe {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("register:{}", self.value));
                app.instance(Arc::new(self.value)).await?;
                if self.fail_register {
                    return Err(Error::custom("registration_probe_failed", "登记失败"));
                }
                Ok(())
            })
        }

        fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.events
                    .lock()
                    .unwrap()
                    .push(format!("boot:{}", self.value));
                if self.fail_boot {
                    return Err(Error::custom("registration_probe_boot_failed", "启动失败"));
                }
                Ok(())
            })
        }
    }

    fn probe(value: usize, events: &Arc<Mutex<Vec<String>>>) -> RegistrationProbe {
        RegistrationProbe {
            value,
            events: Arc::clone(events),
            fail_register: false,
            fail_boot: false,
        }
    }

    #[tokio::test]
    async fn force_before_boot_registers_twice_but_boots_last_in_first_slot() -> Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(probe(1, &events));
        builder.provider(probe(99, &events));
        builder.force_provider(probe(2, &events));
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(*runtime.container().resolve::<usize>().await?, 2);
        assert_eq!(
            *events.lock().unwrap(),
            ["register:1", "register:2", "boot:2"]
        );
        runtime
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn live_force_keeps_failed_registration_contributions_and_boot_index() -> Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(probe(1, &events));
        let mut app =
            crate::core::App::from_provider_runtime(builder.start(Figment::new()).await?)?;
        let old = app.state().resolve::<usize>().await?;
        assert_eq!(
            app.register_provider(probe(99, &events)).await?,
            ProviderId::of::<RegistrationProbe>()
        );
        assert_eq!(*app.state().resolve::<usize>().await?, 1);
        let mut failed_register = probe(2, &events);
        failed_register.fail_register = true;
        assert_eq!(
            app.force_register_provider(failed_register)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("registration_probe_failed".to_owned())
        );
        assert_eq!(*app.state().resolve::<usize>().await?, 2);
        assert_eq!(*old, 1);
        assert_eq!(
            app.register_provider(probe(88, &events)).await?,
            ProviderId::of::<RegistrationProbe>()
        );
        let mut failed_boot = probe(3, &events);
        failed_boot.fail_boot = true;
        assert_eq!(
            app.force_register_provider(failed_boot)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("registration_probe_boot_failed".to_owned())
        );
        assert_eq!(*app.state().resolve::<usize>().await?, 3);
        assert_eq!(
            app.register_provider(probe(77, &events)).await?,
            ProviderId::of::<RegistrationProbe>()
        );
        assert_eq!(
            *events.lock().unwrap(),
            ["register:1", "boot:1", "register:2", "register:3", "boot:3"]
        );
        app.shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        assert_eq!(
            app.register_provider(probe(4, &events))
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        Ok(())
    }

    #[tokio::test]
    async fn live_rebinding_error_stops_register_before_later_contributions() -> Result<()> {
        struct FailingRebinding(Arc<AtomicUsize>);
        impl ServiceProvider for FailingRebinding {
            fn register<'a>(
                &'a self,
                app: &'a mut RegisterContext<'_>,
            ) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    self.0.fetch_add(1, Ordering::SeqCst);
                    let _ = app
                        .rebinding::<usize, _>(|_, _| {
                            Box::pin(async {
                                Err(Error::custom("rebinding_probe_failed", "重绑回调失败"))
                            })
                        })
                        .await?;
                    app.instance(Arc::new(2usize)).await?;
                    app.instance(Arc::new(55u64)).await?;
                    Ok(())
                })
            }
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(probe(1, &events));
        let mut app =
            crate::core::App::from_provider_runtime(builder.start(Figment::new()).await?)?;
        let calls = Arc::new(AtomicUsize::new(0));
        for expected in 1..=2 {
            let error = app
                .register_provider(FailingRebinding(Arc::clone(&calls)))
                .await
                .err();
            assert_eq!(
                error.map(|error| error.code().to_owned()),
                Some("rebinding_probe_failed".to_owned())
            );
            assert_eq!(calls.load(Ordering::SeqCst), expected);
            assert!(!app.state().container().contains::<u64>());
        }
        app.shutdown(Instant::now() + Duration::from_secs(1)).await
    }

    #[tokio::test]
    async fn live_force_keeps_old_and_failed_boot_resources_until_shared_shutdown() -> Result<()> {
        struct ResourceProbe(usize, Arc<Mutex<Vec<usize>>>, bool);
        impl ServiceProvider for ResourceProbe {
            fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    let value = self.0;
                    let cleaned = Arc::clone(&self.1);
                    app.resources().on_shutdown(move || {
                        Box::pin(async move {
                            cleaned.lock().unwrap().push(value);
                            Ok(())
                        })
                    })?;
                    if self.2 {
                        return Err(Error::custom("resource_probe_boot_failed", "启动失败"));
                    }
                    Ok(())
                })
            }
        }
        let cleaned = Arc::new(Mutex::new(Vec::new()));
        let mut builder = ProviderBuilder::new();
        builder.provider(ResourceProbe(1, Arc::clone(&cleaned), false));
        let mut app =
            crate::core::App::from_provider_runtime(builder.start(Figment::new()).await?)?;
        app.force_register_provider(ResourceProbe(2, Arc::clone(&cleaned), false))
            .await?;
        assert!(cleaned.lock().unwrap().is_empty());
        let error = app
            .force_register_provider(ResourceProbe(3, Arc::clone(&cleaned), true))
            .await
            .err();
        assert_eq!(
            error.map(|error| error.code().to_owned()),
            Some("resource_probe_boot_failed".to_owned())
        );
        assert!(cleaned.lock().unwrap().is_empty());
        app.shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        assert_eq!(*cleaned.lock().unwrap(), [3, 2, 1]);
        Ok(())
    }

    struct EarlyScopedStartup {
        cleaned: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        release: Option<Arc<tokio::sync::Notify>>,
        fail_register: bool,
    }

    impl ServiceProvider for EarlyScopedStartup {
        fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let cleaned = Arc::clone(&self.cleaned);
                app.scoped::<usize, _>(move |resolver| {
                    let cleaned = Arc::clone(&cleaned);
                    Box::pin(async move {
                        resolver.resources()?.on_shutdown(move || {
                            Box::pin(async move {
                                cleaned.fetch_add(1, Ordering::SeqCst);
                                Ok(())
                            })
                        })?;
                        Ok(Arc::new(7))
                    })
                })
                .await?;
                let ready = app
                    .rebinding::<usize, _>(|_, _| Box::pin(async { Ok(()) }))
                    .await?;
                assert_eq!(ready.as_deref(), Some(&7));
                self.entered.notify_one();
                if let Some(release) = &self.release {
                    release.notified().await;
                }
                if self.fail_register {
                    Err(Error::custom("early_register_failed", "提前解析后登记失败"))
                } else {
                    Ok(())
                }
            })
        }
    }

    #[tokio::test]
    async fn failed_register_cleans_early_root_scoped_resource_once() {
        let cleaned = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(EarlyScopedStartup {
            cleaned: Arc::clone(&cleaned),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: None,
            fail_register: true,
        });
        let error = builder.start(Figment::new()).await.err();
        assert_eq!(
            error.map(|error| error.code().to_owned()),
            Some("early_register_failed".to_owned())
        );
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_register_cleans_early_root_scoped_resource_once() -> Result<()> {
        let cleaned = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut builder = ProviderBuilder::new();
        builder.provider(EarlyScopedStartup {
            cleaned: Arc::clone(&cleaned),
            entered: Arc::clone(&entered),
            release: Some(Arc::new(tokio::sync::Notify::new())),
            fail_register: false,
        });
        let task = tokio::spawn(async move { builder.start(Figment::new()).await });
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .map_err(Error::internal)?;
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while cleaned.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn failed_base_validation_cleans_early_root_scoped_resource_once() {
        struct InvalidConfig;
        impl ServiceProvider for InvalidConfig {
            fn register<'a>(
                &'a self,
                app: &'a mut RegisterContext<'_>,
            ) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.config_section(
                        "invalid",
                        String::from("text"),
                        SectionSchema::Boolean,
                        |_| Ok(()),
                    )
                })
            }
        }
        let cleaned = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder.provider(EarlyScopedStartup {
            cleaned: Arc::clone(&cleaned),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: None,
            fail_register: false,
        });
        builder.provider(InvalidConfig);
        assert!(builder.start(Figment::new()).await.is_err());
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn source_value_wins_default_during_early_make_and_late_force() -> Result<()> {
        struct PortProbe(Arc<AtomicUsize>);
        impl ServiceProvider for PortProbe {
            fn register<'a>(
                &'a self,
                app: &'a mut RegisterContext<'_>,
            ) -> BoxFuture<'a, Result<()>> {
                Box::pin(async move {
                    app.config_section(
                        "probe",
                        serde_json::json!({"port":3000}),
                        SectionSchema::any(),
                        |_| Ok(()),
                    )?;
                    let seen = Arc::clone(&self.0);
                    app.singleton::<u16, _>(move |resolver| {
                        let seen = Arc::clone(&seen);
                        Box::pin(async move {
                            let value: Value = resolver.section("probe")?;
                            let port = value["port"]
                                .as_u64()
                                .ok_or_else(|| Error::custom("probe_port_missing", "端口缺失"))?;
                            let port = u16::try_from(port)
                                .map_err(|_| Error::custom("probe_port_invalid", "端口越界"))?;
                            seen.store(usize::from(port), Ordering::SeqCst);
                            Ok(Arc::new(port))
                        })
                    })
                    .await?;
                    let _ = app
                        .rebinding::<u16, _>(|_, _| Box::pin(async { Ok(()) }))
                        .await?;
                    Ok(())
                })
            }
        }
        let seen = Arc::new(AtomicUsize::new(0));
        let sources = Figment::from(figment::providers::Serialized::defaults(
            serde_json::json!({"probe":{"port":8080}}),
        ));
        let mut builder = ProviderBuilder::new();
        builder.provider(PortProbe(Arc::clone(&seen)));
        let mut app = crate::core::App::from_provider_runtime(builder.start(sources).await?)?;
        assert_eq!(seen.load(Ordering::SeqCst), 8080);
        assert_eq!(*app.state().resolve::<u16>().await?, 8080);
        app.force_register_provider(PortProbe(Arc::clone(&seen)))
            .await?;
        assert_eq!(seen.load(Ordering::SeqCst), 8080);
        assert_eq!(app.section::<Value>("probe")?["port"], 8080);
        app.shutdown(Instant::now() + Duration::from_secs(1)).await
    }
}
