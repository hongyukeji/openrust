//! 统一 `ServiceProvider` 的注册、配置与启动生命周期。

use std::any::{Any, TypeId, type_name};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use figment::Figment;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::time::Instant;

use crate::config::repository::{ConfigRepository, SectionSchema};
use crate::config::{ConfigError, FrameworkConfig};

use super::command::Command;
use super::container::{Container, ContainerBuilder, Resolver};
use super::module::ModuleManifest;
use super::resources::{ResourceOwner, ResourceRegistrar};
use super::{BoxFuture, Error, Result};

type Extensions = HashMap<TypeId, Box<dyn Any + Send + Sync>>;
type Required = Box<dyn Fn(&Container) -> BoxFuture<'_, Result<()>> + Send + Sync>;
type Finalizer = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;
type AppConfigRegistration = Box<dyn FnOnce(&mut ContainerBuilder, ConfigRepository) + Send>;
type BindingOverride = Box<dyn FnOnce(&mut ContainerBuilder) -> Result<()> + Send>;

/// Provider 的具体 Rust 类型身份；错误消息只公开类型名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId {
    type_id: TypeId,
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

    /// 仅登记绑定、配置 schema 与默认值，不解析服务或贡献路由。
    fn register(&self, _app: &mut RegisterContext<'_>) -> Result<()> {
        Ok(())
    }

    /// 所有 Provider 注册且基础配置验证后依序启动。
    fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Provider 注册阶段可访问的受限接口。
pub struct RegisterContext<'a> {
    bindings: &'a mut ContainerBuilder,
    config: &'a mut ConfigRepository,
    required: &'a mut Vec<Required>,
    owner: ProviderId,
}

impl RegisterContext<'_> {
    /// 判断已有 Provider 是否已登记服务，供默认绑定遵循应用覆盖顺序。
    pub fn is_bound<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.bindings.contains::<T>()
    }

    /// 判断原始来源是否声明区段，供可选领域 Provider 选择是否安装。
    pub fn source_has_section(&self, key: &str) -> Result<bool> {
        self.config.source_has_section(key).map_err(Error::from)
    }

    /// 每次解析构造新实例。
    pub fn bind<T, F>(&mut self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bindings.bind(factory);
    }

    /// 在应用内共享成功构造的实例。
    pub fn singleton<T, F>(&mut self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bindings.singleton(factory);
    }

    /// 在一个请求、任务或命令 scope 内共享实例。
    pub fn scoped<T, F>(&mut self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.bindings.scoped(factory);
    }

    /// 登记已存在的共享实例。
    pub fn instance<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) {
        self.bindings.instance(value);
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
        self.config
            .register_section(self.owner.type_id, key, defaults, schema, validate)
    }

    /// 排入 Laravel 包配置的浅层默认合并。
    pub fn merge_defaults(&mut self, key: &'static str, defaults: Value) -> Result<()> {
        self.config.merge_defaults(key, defaults)
    }

    /// 排入 Laravel 包配置的递归默认合并。
    pub fn replace_defaults_recursively(
        &mut self,
        key: &'static str,
        defaults: Value,
    ) -> Result<()> {
        self.config.replace_defaults_recursively(key, defaults)
    }
}

/// Provider 启动阶段的解析、模块覆盖和领域贡献入口。
pub struct BootContext<'a> {
    container: &'a Container,
    config: &'a mut ConfigRepository,
    resources: ResourceRegistrar,
    required: &'a mut Vec<Required>,
    extensions: &'a mut Extensions,
    commands: &'a mut Vec<Box<dyn Command>>,
    finalizers: &'a mut Vec<Finalizer>,
}

impl BootContext<'_> {
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

    /// 读取当前已发布配置视图。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        self.config.section(key).map_err(Error::from)
    }

    /// 共享配置仓储句柄，供最终校验读取全部 boot 覆盖后的配置。
    pub fn config_repository(&self) -> ConfigRepository {
        self.config.clone()
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
    providers: Vec<Box<dyn ServiceProvider>>,
    modules: ModuleManifest,
    removed: HashSet<ProviderId>,
    app_config: Option<AppConfigRegistration>,
    binding_overrides: Vec<BindingOverride>,
}

fn ordered_providers(
    providers: Vec<Box<dyn ServiceProvider>>,
) -> Result<Vec<Box<dyn ServiceProvider>>> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for provider in providers {
        if seen.insert(provider.id().type_id) {
            ordered.push(provider);
        }
    }
    let all: HashSet<_> = ordered
        .iter()
        .map(|provider| provider.id().type_id)
        .collect();
    let mut ready = HashSet::new();
    for provider in &ordered {
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
        ready.insert(provider.id().type_id);
    }
    Ok(ordered)
}

struct StartupResources(Option<ResourceOwner>);

impl StartupResources {
    fn new() -> Self {
        Self(Some(ResourceOwner::new()))
    }

    fn owner(&self) -> Result<&ResourceOwner> {
        self.0.as_ref().ok_or_else(|| {
            Error::custom("provider_resources_transferred", "启动资源所有权已经交接")
        })
    }

    fn into_owner(mut self) -> Result<ResourceOwner> {
        self.0.take().ok_or_else(|| {
            Error::custom("provider_resources_transferred", "启动资源所有权已经交接")
        })
    }
}

impl Drop for StartupResources {
    fn drop(&mut self) {
        let Some(owner) = self.0.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = owner
                    .shutdown(Instant::now() + Duration::from_secs(5))
                    .await
                {
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

    /// 添加 Provider；同具体类型重复登记保留第一项。
    pub fn provider(&mut self, provider: impl ServiceProvider) -> &mut Self {
        self.providers.push(Box::new(provider));
        self
    }

    /// 加入已盒装的应用 Provider，保留具体类型身份。
    pub fn boxed_provider(&mut self, provider: Box<dyn ServiceProvider>) -> &mut Self {
        self.providers.push(provider);
        self
    }

    /// 在启动前从清单移除具体 Provider 类型；依赖关系随后重新校验。
    pub fn remove_provider(&mut self, id: ProviderId) -> &mut Self {
        self.providers.retain(|provider| provider.id() != id);
        self.removed.insert(id);
        self
    }

    /// 声明已编译模块和本次启动的启用来源；与应用 Provider 共用一个生命周期。
    pub fn module_manifest(&mut self, manifest: ModuleManifest) -> &mut Self {
        self.modules = manifest;
        self
    }

    /// 仅从本次应用构建移除模块，不改写持久启用状态。
    pub fn remove_module(&mut self, name: impl Into<String>) -> &mut Self {
        self.modules.disable(name);
        self
    }

    /// 全体 Provider 注册后、容器封口前覆盖类型化服务绑定。
    ///
    /// 测试 fake 使用与生产相同的容器和启动顺序；回调只运行一次，不能解析服务。
    pub fn configure_after_register<F>(&mut self, configure: F) -> &mut Self
    where
        F: FnOnce(&mut ContainerBuilder) -> Result<()> + Send + 'static,
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
    pub async fn start(self, sources: Figment) -> Result<ProviderRuntime> {
        let mut providers = self.providers;
        Self::append_modules(self.modules, &self.removed, &mut providers)?;
        let providers = ordered_providers(providers)?;
        let mut bindings = ContainerBuilder::new();
        let mut config = ConfigRepository::new(sources);
        if let Some(register_config) = self.app_config {
            register_config(&mut bindings, config.clone());
        }
        let mut required = Vec::new();
        for provider in &providers {
            provider.register(&mut RegisterContext {
                bindings: &mut bindings,
                config: &mut config,
                required: &mut required,
                owner: provider.id(),
            })?;
        }
        for configure in self.binding_overrides {
            configure(&mut bindings)?;
        }
        let resources = StartupResources::new();
        bindings.context(config.clone(), resources.owner()?.registrar());
        config.validate_base()?;
        let container = bindings.build();
        let mut extensions = Extensions::new();
        let mut commands = Vec::new();
        let mut finalizers = Vec::new();
        let result = async {
            for provider in &providers {
                provider
                    .boot(&mut BootContext {
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
            config.freeze()?;
            for validate in finalizers {
                validate().await?;
            }
            let framework: FrameworkConfig = config
                .snapshot()
                .extract()
                .map_err(|error| Error::Config(ConfigError::from(error)))?;
            for resolve in required {
                resolve(&container).await?;
            }
            Ok(framework)
        }
        .await;
        let framework = match result {
            Ok(framework) => framework,
            Err(error) => {
                let shutdown = resources
                    .owner()?
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
        })
    }

    fn append_modules(
        modules: ModuleManifest,
        removed: &HashSet<ProviderId>,
        providers: &mut Vec<Box<dyn ServiceProvider>>,
    ) -> Result<()> {
        let mut owners: HashMap<TypeId, &'static str> = HashMap::new();
        let mut existing: HashSet<TypeId> = providers
            .iter()
            .map(|provider| provider.id().type_id)
            .collect();
        for module in modules.prepare()? {
            let module_name = module.name;
            for provider in module.providers {
                let id = provider.id();
                if removed.contains(&id) {
                    continue;
                }
                if !existing.insert(id.type_id) {
                    let previous = owners.get(&id.type_id).copied().unwrap_or("应用清单");
                    return Err(Error::custom(
                        "module_provider_conflict",
                        format!(
                            "Provider {} 同时属于 {previous} 与 {module_name}",
                            id.name()
                        ),
                    ));
                }
                owners.insert(id.type_id, module_name);
                providers.push(provider);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::core::{ModuleConfigFiles, ModuleDescriptor, ModuleManifest};

    struct Base(Arc<Mutex<Vec<&'static str>>>);
    struct Overlay(Arc<Mutex<Vec<&'static str>>>);
    struct Observe(Arc<Mutex<Vec<&'static str>>>);

    impl ServiceProvider for Base {
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            self.0.lock().unwrap().push("base.register");
            app.instance(Arc::new(41usize));
            app.config_section(
                "service",
                serde_json::json!({ "name": "base", "enabled": true }),
                SectionSchema::any(),
                |_| Ok(()),
            )
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

        fn register(&self, _app: &mut RegisterContext<'_>) -> Result<()> {
            self.0.lock().unwrap().push("overlay.register");
            Ok(())
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
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            app.instance(Arc::new(String::from("production")));
            app.require::<String>();
            Ok(())
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
        builder.configure_after_register(|bindings| {
            assert!(bindings.contains::<String>());
            bindings.instance(Arc::new(String::from("fake")));
            Ok(())
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
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
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
            });
            app.require::<String>();
            Ok(())
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
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            app.config_section(
                "sample",
                serde_json::json!({"name":"Default","items":["a","b","c"],"settings":{"value":"default"}}),
                SectionSchema::any(),
                |_| Ok(()),
            )
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
            fn register(&self, _app: &mut RegisterContext<'_>) -> Result<()> {
                self.0
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("producer.register");
                Ok(())
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
            fn register(&self, _app: &mut RegisterContext<'_>) -> Result<()> {
                self.0
                    .lock()
                    .map_err(|_| Error::custom("test_event_lock", "测试事件锁已中毒"))?
                    .push("consumer.register");
                Ok(())
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
}
