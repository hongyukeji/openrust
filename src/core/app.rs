//! 应用组装入口：[`Application`]、[`App`] 和 [`AppState`]。

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use figment::Figment;
use serde::de::DeserializeOwned;

use crate::config::repository::ConfigRepository;
use crate::config::{ConfigError, FrameworkConfig};
use crate::core::command::Command;
use crate::core::container::{Container, Scope};
use crate::core::error::{Error, Result};
use crate::core::module::ModuleManifest;
use crate::core::provider::{ProviderRuntime, ServiceProvider};
use crate::core::resources::ResourceOwner;

/// 每个项目实现一次，返回统一 Provider 清单。
///
/// ```
/// use openrust::core::{Application, ServiceProvider};
/// use openrust::config::NoConfig;
///
/// struct Server;
/// impl Application for Server {
///     type Config = NoConfig;
///     fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> { Vec::new() }
/// }
/// ```
pub trait Application: Send + Sync + 'static {
    /// 应用专属配置，与框架配置来自相同的分层配置源，并作为服务注册。
    /// 没有应用专属配置时，请使用 [`NoConfig`](crate::config::NoConfig)。
    type Config: DeserializeOwned + Send + Sync + 'static;

    /// 用于引导加载此应用配置的环境变量命名空间。
    ///
    /// 默认值为 `APP_`；应用也可以返回 `SHOP_` 等值。
    fn config_namespace() -> &'static str {
        "APP_"
    }

    /// 按框架、应用、模块顺序返回 Provider 清单；选择依据为启动来源的应用配置。
    fn providers(_config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        Vec::new()
    }

    /// 声明已编译模块及本次启动的启用来源；缺省不挂载业务模块。
    fn modules(_config: &Self::Config) -> ModuleManifest {
        ModuleManifest::new()
    }
}

/// 处理器或命令请求未注册服务时返回的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("service `{0}` is not registered; register it in a ServiceProvider")]
pub struct MissingServiceError(pub &'static str);

type Extensions = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

struct StateInner {
    framework: FrameworkConfig,
    config_repository: ConfigRepository,
    services: Container,
    provider_resources: Arc<ResourceOwner>,
}

struct CommandScopeGuard(Arc<Scope>);

impl Drop for CommandScopeGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// 可低成本克隆、供每个请求和任务共享的句柄，包含配置和服务。
#[derive(Clone)]
pub struct AppState {
    inner: Arc<StateInner>,
}

impl AppState {
    pub(crate) fn begin_close(&self) {
        self.inner.services.seal_root();
        self.inner.provider_resources.seal();
    }

    /// 取得本应用唯一的服务容器句柄。
    pub fn container(&self) -> &Container {
        &self.inner.services
    }

    /// 在当前入口 scope（若属于本应用）或根生命周期异步解析服务。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        if let Some(scope) = self.inner.services.current_entry_scope() {
            scope.resolve::<T>().await
        } else {
            self.inner.services.resolve::<T>().await
        }
    }
    /// 启动时解析的框架配置快照；运行期原始写入不重建其中的服务配置。
    pub fn config(&self) -> &FrameworkConfig {
        &self.inner.framework
    }

    /// 返回与 Provider 和当前应用共享的配置仓储句柄。
    pub fn config_repository(&self) -> ConfigRepository {
        self.inner.config_repository.clone()
    }

    /// 同步读取已就绪的 `T`；异步工厂尚未解析时返回 `None`。
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner.services.get_ready()
    }

    /// 返回已注册的 `T` 类型服务；若未注册则返回包含缺失类型名称的错误。
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        if self.inner.services.is_terminated() {
            return Err(Error::custom("container_closed", "容器已关闭"));
        }
        if let Some(service) = self.get() {
            return Ok(service);
        }
        if self.inner.services.contains::<T>() {
            return Err(Error::custom(
                "service_requires_async_resolution",
                format!("服务 `{}` 须异步解析", type_name::<T>()),
            ));
        }
        Err(Error::internal(MissingServiceError(type_name::<T>())))
    }

    pub(crate) async fn shutdown_resources(&self, deadline: tokio::time::Instant) -> Result<()> {
        let (scoped, application) = tokio::join!(
            self.inner.services.shutdown_root(deadline),
            self.inner.provider_resources.shutdown(deadline)
        );
        match (scoped, application) {
            (Err(error), Err(cleanup_error)) => {
                tracing::error!(
                    code = cleanup_error.code(),
                    "application resource cleanup also failed"
                );
                Err(error)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), result) => result,
        }
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("app", &self.inner.framework.app.name)
            .field("services", &self.inner.services.len())
            .finish()
    }
}

/// 完整组装后的应用：共享配置仓储、启动快照、命令和 crate 扩展。
pub struct App {
    state: AppState,
    figment: Figment,
    commands: Vec<Box<dyn Command>>,
    extensions: Extensions,
}

impl App {
    /// 同步封闭新解析和资源登记；异步回收仍须调用 `shutdown`。
    pub(crate) fn begin_close(&self) {
        self.state.begin_close();
    }

    /// 在共同期限内取消并回收应用受管资源；关闭后不能再次解析服务。
    pub async fn shutdown(&self, deadline: tokio::time::Instant) -> Result<()> {
        self.state.shutdown_resources(deadline).await
    }

    /// 将统一 Provider 生命周期的结果装入生产 App，使用同一个 Container。
    pub fn from_provider_runtime(runtime: ProviderRuntime) -> Result<Self> {
        let figment = runtime.config.snapshot();
        Ok(Self {
            state: AppState {
                inner: Arc::new(StateInner {
                    framework: runtime.framework,
                    config_repository: runtime.config,
                    services: runtime.container,
                    provider_resources: Arc::new(runtime.resources),
                }),
            },
            figment,
            commands: runtime.commands,
            extensions: runtime.extensions,
        })
    }

    /// 提供给 HTTP 处理器、任务和命令的共享状态。
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// 启动时解析的框架配置快照；运行期原始写入不重建已有服务。
    pub fn config(&self) -> &FrameworkConfig {
        self.state.config()
    }

    /// 返回与 Provider 和应用状态共享的配置仓储句柄。
    pub fn config_repository(&self) -> ConfigRepository {
        self.state.config_repository()
    }

    /// 启动时合并的配置来源快照，供 `config show` 等命令使用。
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    /// 从当前配置仓储提取一个区段；运行期原始写入随后可见。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        self.state.inner.config_repository.section(key)
    }

    /// 按注册顺序排列的命令。
    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.commands.iter().map(AsRef::as_ref)
    }

    /// 注册阶段填充的 crate 专属扩展容器。
    pub fn extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())
            .and_then(|item| item.downcast_ref::<T>())
    }

    /// 使用 `args` 运行名为 `name` 的命令；调用方可继续复用应用，最后显式关闭。
    pub async fn run_command(&self, name: &str, args: Vec<String>) -> Result<()> {
        self.run_command_inner(name, args).await
    }

    async fn run_command_inner(&self, name: &str, args: Vec<String>) -> Result<()> {
        let available: Vec<&str> = self
            .commands()
            .filter(|command| command.name().split_whitespace().next() == Some(name))
            .map(Command::name)
            .collect();
        let command = self
            .commands()
            .filter(|command| {
                let mut words = command.name().split_whitespace();
                words.next() == Some(name)
                    && words
                        .clone()
                        .zip(args.iter())
                        .all(|(word, arg)| word == arg)
                    && words.count() <= args.len()
            })
            .max_by_key(|command| command.name().split_whitespace().count())
            .ok_or_else(|| {
                let input = std::iter::once(name)
                    .chain(args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let hint = if available.is_empty() {
                    "run `help` to list commands".to_owned()
                } else {
                    format!("available: {}", available.join(", "))
                };
                Error::custom_with_status(
                    http::StatusCode::BAD_REQUEST,
                    "unknown_command",
                    format!("unknown command `{input}`; {hint}"),
                )
            })?;
        if let Some(checks) = self.extension::<super::provider::DeferredBootChecks>() {
            for check in &checks.0 {
                if command.type_id() != check.permitted_command {
                    (check.validate)().await?;
                }
            }
        }
        let consumed = command.name().split_whitespace().count() - 1;
        let remaining: Vec<String> = args.into_iter().skip(consumed).collect();
        if !command.accepts_args()
            && let Some(argument) = remaining.first()
        {
            return Err(Error::custom_with_status(
                http::StatusCode::BAD_REQUEST,
                "unexpected_argument",
                format!(
                    "unexpected argument `{argument}` for `{}`; run `help` to list commands",
                    command.name()
                ),
            ));
        }
        let scope = Arc::new(self.state().container().scope()?);
        let _scope_guard = CommandScopeGuard(Arc::clone(&scope));
        let result = scope
            .run(async {
                let i18n = if self.state().container().contains::<crate::i18n::I18n>() {
                    Some(self.state().resolve::<crate::i18n::I18n>().await?)
                } else {
                    None
                };
                let mut metadata = crate::core::ExecutionMetadata::default();
                let locale = i18n.as_ref().map(|service| service.select(None));
                if let Some(locale) = &locale {
                    metadata = metadata.with_locale(locale.clone());
                }
                let execution = crate::core::ExecutionContext::new(
                    metadata,
                    None,
                    crate::core::Cancellation::new(),
                );
                crate::core::with_execution_context(execution, async move {
                    if let (Some(i18n), Some(locale)) = (i18n, locale) {
                        i18n.with_locale(locale, command.run(self, remaining)).await
                    } else {
                        command.run(self, remaining).await
                    }
                })
                .await
            })
            .await;
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(self.config().server.shutdown_timeout_secs);
        let cleanup = scope.shutdown(deadline).await;
        match (result, cleanup) {
            (Err(error), Err(cleanup_error)) => {
                tracing::error!(
                    code = cleanup_error.code(),
                    "command scope cleanup also failed"
                );
                Err(error)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), cleanup) => cleanup,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BootContext, BoxFuture, ExecutionContext, ProviderBuilder, RegisterContext};
    use figment::providers::Serialized;
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, PartialEq)]
    struct Counter(u32);

    #[derive(Default)]
    struct Hooks(Vec<&'static str>);

    struct Empty;
    impl ServiceProvider for Empty {}

    struct CounterProvider;
    impl ServiceProvider for CounterProvider {
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            app.instance(Arc::new(Counter(7)));
            Ok(())
        }
    }

    struct HooksProvider;
    impl ServiceProvider for HooksProvider {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.extension_mut::<Hooks>()?.0.push("a");
                app.extension_mut::<Hooks>()?.0.push("b");
                Ok(())
            })
        }
    }

    struct NestedCommand;
    impl Command for NestedCommand {
        fn name(&self) -> &'static str {
            "config show"
        }
        fn about(&self) -> &'static str {
            "Test nested command"
        }
        fn accepts_args(&self) -> bool {
            true
        }
        fn run<'a>(&'a self, _app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(args, ["--json"]);
                assert!(ExecutionContext::current().is_some());
                Ok(())
            })
        }
    }

    enum TestCommand {
        Nested,
        Help,
        ConfigShow,
    }
    impl ServiceProvider for TestCommand {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                match self {
                    Self::Nested => app.command(NestedCommand),
                    Self::Help => app.command(crate::core::command::HelpCommand),
                    Self::ConfigShow => app.command(crate::core::command::ConfigShowCommand),
                }
                Ok(())
            })
        }
    }

    struct TaskProvider {
        started: Arc<tokio::sync::Notify>,
        stopped: Arc<tokio::sync::Notify>,
    }

    struct ScopedCommandProvider {
        created: Arc<AtomicUsize>,
        cleaned: Arc<AtomicUsize>,
        seen: Arc<Mutex<Vec<usize>>>,
        pending_scope: Option<tokio::sync::mpsc::UnboundedSender<Arc<Scope>>>,
        cleaned_notify: Option<Arc<tokio::sync::Notify>>,
    }

    struct ScopedCommand {
        seen: Arc<Mutex<Vec<usize>>>,
        pending_scope: Option<tokio::sync::mpsc::UnboundedSender<Arc<Scope>>>,
    }

    impl Command for ScopedCommand {
        fn name(&self) -> &'static str {
            "scope check"
        }

        fn about(&self) -> &'static str {
            "Check command scope"
        }

        fn accepts_args(&self) -> bool {
            true
        }

        fn run<'a>(&'a self, app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let first = app.state().resolve::<usize>().await?;
                let second = app.state().resolve::<usize>().await?;
                assert!(Arc::ptr_eq(&first, &second));
                self.seen
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(*first);
                if args.first().map(String::as_str) == Some("pending") {
                    let scope = app
                        .state()
                        .container()
                        .current_entry_scope()
                        .ok_or_else(|| {
                            Error::custom("command_scope_missing", "command scope missing")
                        })?;
                    if let Some(sender) = &self.pending_scope {
                        let _ = sender.send(scope);
                    }
                    std::future::pending::<()>().await;
                }
                if args.first().map(String::as_str) == Some("fail") {
                    return Err(Error::custom("command_failed", "intentional test failure"));
                }
                Ok(())
            })
        }
    }

    impl ServiceProvider for ScopedCommandProvider {
        fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
            let created = Arc::clone(&self.created);
            let cleaned = Arc::clone(&self.cleaned);
            let cleaned_notify = self.cleaned_notify.clone();
            app.scoped::<usize, _>(move |resolver| {
                let created = Arc::clone(&created);
                let cleaned = Arc::clone(&cleaned);
                let cleaned_notify = cleaned_notify.clone();
                Box::pin(async move {
                    let id = created.fetch_add(1, Ordering::SeqCst) + 1;
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            cleaned.fetch_add(1, Ordering::SeqCst);
                            if let Some(notify) = cleaned_notify {
                                notify.notify_one();
                            }
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(id))
                })
            });
            Ok(())
        }

        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.command(ScopedCommand {
                    seen: Arc::clone(&self.seen),
                    pending_scope: self.pending_scope.clone(),
                });
                Ok(())
            })
        }
    }
    impl ServiceProvider for TaskProvider {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let started = Arc::clone(&self.started);
                let stopped = Arc::clone(&self.stopped);
                app.resources().spawn(move |cancellation| async move {
                    started.notify_one();
                    cancellation.cancelled().await;
                    stopped.notify_one();
                })?;
                Ok(())
            })
        }
    }

    async fn start(provider: impl ServiceProvider) -> Result<App> {
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
        App::from_provider_runtime(builder.start(source).await?)
    }

    #[tokio::test]
    async fn services_are_typed_and_shared() -> Result<()> {
        let app = start(CounterProvider).await?;
        assert_eq!(app.state().get::<Counter>().map(|c| c.0), Some(7));
        Ok(())
    }

    #[tokio::test]
    async fn app_config_repository_is_live_shared_and_isolated_from_other_apps() -> Result<()> {
        let app = start(Empty).await?;
        let other = start(Empty).await?;
        let state = app.state().clone();
        let startup_name = app.config().app.name.clone();
        let startup_figment = app.figment().clone();
        app.config_repository().set("runtime.flag", json!(true))?;
        state
            .config_repository()
            .set("app.name", json!("runtime-name"))?;
        assert!(app.section::<bool>("runtime.flag")?);
        assert_eq!(app.section::<String>("app.name")?, "runtime-name");
        assert_eq!(app.config().app.name, startup_name);
        assert_eq!(
            startup_figment
                .extract_inner::<String>("app.name")
                .map_err(ConfigError::from)?,
            startup_name
        );
        assert!(other.config_repository().get("runtime.flag")?.is_none());
        assert_ne!(other.section::<String>("app.name")?, "runtime-name");
        Ok(())
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_service() -> Result<()> {
        let app = start(Empty).await?;
        assert!(app.state().get::<String>().is_none());
        Ok(())
    }

    #[tokio::test]
    async fn try_get_reports_missing_type_name() -> Result<()> {
        let app = start(Empty).await?;
        let error = app
            .state()
            .try_get::<String>()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("alloc::string::String"),
            "message was: {error}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn extensions_accumulate_across_registrations() -> Result<()> {
        let app = start(HooksProvider).await?;
        assert_eq!(
            app.extension::<Hooks>().map(|h| h.0.clone()),
            Some(vec!["a", "b"])
        );
        Ok(())
    }

    #[tokio::test]
    async fn unknown_command_is_reported() -> Result<()> {
        let app = start(Empty).await?;
        let error = app.run_command("nope", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `nope`; run `help` to list commands")
        );
        Ok(())
    }

    struct AllowedMigration(Arc<AtomicUsize>);
    struct ShadowMigration(Arc<AtomicUsize>);
    struct CheckedServe(Arc<AtomicUsize>);

    impl Command for AllowedMigration {
        fn name(&self) -> &'static str {
            "migrate"
        }
        fn about(&self) -> &'static str {
            "Allowed migration"
        }
        fn accepts_args(&self) -> bool {
            true
        }
        fn run<'a>(&'a self, _: &'a App, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    impl Command for ShadowMigration {
        fn name(&self) -> &'static str {
            "migrate custom"
        }
        fn about(&self) -> &'static str {
            "Shadow migration"
        }
        fn run<'a>(&'a self, _: &'a App, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    impl Command for CheckedServe {
        fn name(&self) -> &'static str {
            "serve"
        }
        fn about(&self) -> &'static str {
            "Serve"
        }
        fn run<'a>(&'a self, _: &'a App, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    struct DeferredCommandProbe {
        checks: Arc<AtomicUsize>,
        runs: Arc<AtomicUsize>,
    }
    impl ServiceProvider for DeferredCommandProbe {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                assert_eq!(app.console_command(), Some("migrate"));
                app.command(AllowedMigration(Arc::clone(&self.runs)));
                app.command(ShadowMigration(Arc::clone(&self.runs)));
                app.command(CheckedServe(Arc::clone(&self.runs)));
                let checks = Arc::clone(&self.checks);
                app.defer_boot_check_for_command(
                    std::any::TypeId::of::<AllowedMigration>(),
                    move || {
                        let checks = Arc::clone(&checks);
                        Box::pin(async move {
                            checks.fetch_add(1, Ordering::SeqCst);
                            Err(Error::custom("missing_schema", "schema is missing"))
                        })
                    },
                )
            })
        }
    }

    #[tokio::test]
    async fn deferred_boot_check_follows_selected_command_identity_and_reused_app() -> Result<()> {
        let checks = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicUsize::new(0));
        let mut builder = ProviderBuilder::new();
        builder
            .console_command("migrate")
            .provider(DeferredCommandProbe {
                checks: Arc::clone(&checks),
                runs: Arc::clone(&runs),
            });
        let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
        let app = App::from_provider_runtime(builder.start(source).await?)?;
        app.run_command("migrate", Vec::new()).await?;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(checks.load(Ordering::SeqCst), 0);
        for (name, args) in [
            ("migrate", vec!["custom".to_owned()]),
            ("serve", Vec::new()),
        ] {
            let error = app.run_command(name, args).await.err();
            assert_eq!(error.as_ref().map(Error::code), Some("missing_schema"));
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        assert_eq!(
            app.run_command("unknown", Vec::new())
                .await
                .err()
                .as_ref()
                .map(Error::code),
            Some("unknown_command")
        );
        Ok(())
    }

    #[tokio::test]
    async fn commands_reuse_resources_until_explicit_application_shutdown() -> Result<()> {
        let started = Arc::new(tokio::sync::Notify::new());
        let stopped = Arc::new(tokio::sync::Notify::new());
        let app = start(TaskProvider {
            started: Arc::clone(&started),
            stopped: Arc::clone(&stopped),
        })
        .await?;
        started.notified().await;
        let error = app.run_command("missing", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        let error = app.run_command("missing", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), stopped.notified())
                .await
                .is_err()
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        stopped.notified().await;
        Ok(())
    }

    #[tokio::test]
    async fn consecutive_commands_use_distinct_scopes_and_close_each_one() -> Result<()> {
        let created = Arc::new(AtomicUsize::new(0));
        let cleaned = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = start(ScopedCommandProvider {
            created: Arc::clone(&created),
            cleaned: Arc::clone(&cleaned),
            seen: Arc::clone(&seen),
            pending_scope: None,
            cleaned_notify: None,
        })
        .await?;
        app.run_command("scope", vec!["check".into()]).await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        app.run_command("scope", vec!["check".into()]).await?;
        assert_eq!(created.load(Ordering::SeqCst), 2);
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
        let failure = app
            .run_command("scope", vec!["check".into(), "fail".into()])
            .await
            .err();
        assert_eq!(failure.as_ref().map(Error::code), Some("command_failed"));
        assert_eq!(created.load(Ordering::SeqCst), 3);
        assert_eq!(cleaned.load(Ordering::SeqCst), 3);
        assert_eq!(
            *seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![1, 2, 3]
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_and_timed_out_commands_close_their_scopes() -> Result<()> {
        let created = Arc::new(AtomicUsize::new(0));
        let cleaned = Arc::new(AtomicUsize::new(0));
        let cleaned_notify = Arc::new(tokio::sync::Notify::new());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (pending_scope, mut scopes) = tokio::sync::mpsc::unbounded_channel();
        let app = Arc::new(
            start(ScopedCommandProvider {
                created: Arc::clone(&created),
                cleaned: Arc::clone(&cleaned),
                seen: Arc::clone(&seen),
                pending_scope: Some(pending_scope),
                cleaned_notify: Some(Arc::clone(&cleaned_notify)),
            })
            .await?,
        );

        let cancelled = tokio::spawn({
            let app = Arc::clone(&app);
            async move {
                app.run_command("scope", vec!["check".into(), "pending".into()])
                    .await
            }
        });
        let first_scope = scopes
            .recv()
            .await
            .ok_or_else(|| Error::custom("scope_missing", "scope missing"))?;
        let cleaned_once = cleaned_notify.notified();
        tokio::pin!(cleaned_once);
        cleaned_once.as_mut().enable();
        cancelled.abort();
        let cancelled_result = cancelled.await;
        assert!(
            cancelled_result
                .as_ref()
                .is_err_and(tokio::task::JoinError::is_cancelled)
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), cleaned_once)
            .await
            .map_err(Error::internal)?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        assert_eq!(
            first_scope
                .resolve::<usize>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );

        let timed_out = tokio::spawn({
            let app = Arc::clone(&app);
            async move {
                tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    app.run_command("scope", vec!["check".into(), "pending".into()]),
                )
                .await
            }
        });
        let second_scope = scopes
            .recv()
            .await
            .ok_or_else(|| Error::custom("scope_missing", "scope missing"))?;
        let cleaned_twice = cleaned_notify.notified();
        tokio::pin!(cleaned_twice);
        cleaned_twice.as_mut().enable();
        assert!(timed_out.await.map_err(Error::internal)?.is_err());
        tokio::time::timeout(std::time::Duration::from_secs(1), cleaned_twice)
            .await
            .map_err(Error::internal)?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
        assert_eq!(
            second_scope
                .resolve::<usize>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );

        app.run_command("scope", vec!["check".into()]).await?;
        assert_eq!(created.load(Ordering::SeqCst), 3);
        assert_eq!(cleaned.load(Ordering::SeqCst), 3);
        assert_eq!(
            *seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![1, 2, 3]
        );
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn managed_scope_task_sees_same_service_and_explicit_metadata() -> Result<()> {
        let created = Arc::new(AtomicUsize::new(0));
        let cleaned = Arc::new(AtomicUsize::new(0));
        let app = start(ScopedCommandProvider {
            created: Arc::clone(&created),
            cleaned: Arc::clone(&cleaned),
            seen: Arc::new(Mutex::new(Vec::new())),
            pending_scope: None,
            cleaned_notify: None,
        })
        .await?;
        let scope = Arc::new(app.state().container().scope()?);
        let first = scope.run(app.state().resolve::<usize>()).await?;
        let (sent, received) = tokio::sync::oneshot::channel();
        let state = app.state().clone();
        let metadata = crate::core::ExecutionMetadata::default().with_request_id("safe-id")?;
        scope.spawn(metadata, None, move |cancel| {
            let constructed_in_scope = state.container().current_entry_scope().is_some();
            let constructor_request_id = ExecutionContext::current()
                .and_then(|context| context.metadata().request_id().map(str::to_owned));
            async move {
                let result = async {
                    let value = state.resolve::<usize>().await?;
                    let request_id = ExecutionContext::current()
                        .and_then(|context| context.metadata().request_id().map(str::to_owned));
                    Ok::<_, Error>((
                        *value,
                        request_id,
                        constructor_request_id,
                        constructed_in_scope,
                    ))
                }
                .await;
                let _ = sent.send(result);
                cancel.cancelled().await;
            }
        })?;
        let (child, request_id, constructor_request_id, constructed_in_scope) =
            received.await.map_err(Error::internal)??;
        assert_eq!(child, *first);
        assert_eq!(request_id.as_deref(), Some("safe-id"));
        assert_eq!(constructor_request_id.as_deref(), Some("safe-id"));
        assert!(constructed_in_scope);
        let root = tokio::spawn({
            let state = app.state().clone();
            async move {
                assert!(ExecutionContext::current().is_none());
                state.resolve::<usize>().await
            }
        })
        .await
        .map_err(Error::internal)??;
        assert_ne!(*root, *first);
        scope
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn application_shutdown_seals_root_scope_without_publishing_new_generation() -> Result<()>
    {
        struct ScopedProvider;
        impl ServiceProvider for ScopedProvider {
            fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
                app.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(7)) }));
                Ok(())
            }
        }
        let app = start(ScopedProvider).await?;
        assert_eq!(*app.state().resolve::<usize>().await?, 7);
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        assert_eq!(
            app.state()
                .resolve::<usize>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        assert_eq!(
            app.state()
                .container()
                .scope()
                .err()
                .map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        Ok(())
    }

    #[tokio::test]
    async fn serve_is_unknown_without_http_provider() -> Result<()> {
        let app = start(TestCommand::Help).await?;
        let error = app.run_command("serve", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `serve`; run `help` to list commands")
        );
        Ok(())
    }

    #[tokio::test]
    async fn nested_command_consumes_its_name_words() -> Result<()> {
        let app = start(TestCommand::Nested).await?;
        app.run_command("config", vec!["show".into(), "--json".into()])
            .await?;
        assert_eq!(
            app.run_command("config", vec!["other".into()])
                .await
                .err()
                .as_ref()
                .map(Error::code),
            Some("unknown_command")
        );
        Ok(())
    }

    #[tokio::test]
    async fn unknown_nested_command_names_input_and_available_command() -> Result<()> {
        let app = start(TestCommand::Nested).await?;
        let error = app
            .run_command("config", vec!["foo".into(), "bar".into()])
            .await
            .err();
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `config foo bar`; available: config show")
        );
        Ok(())
    }

    #[tokio::test]
    async fn config_show_rejects_extra_argument() -> Result<()> {
        let app = start(TestCommand::ConfigShow).await?;
        let error = app
            .run_command("config", vec!["show".into(), "--typo".into()])
            .await
            .err();
        assert_eq!(error.as_ref().map(Error::code), Some("unexpected_argument"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unexpected argument `--typo` for `config show`; run `help` to list commands")
        );
        Ok(())
    }

    #[test]
    fn sensitive_section_error_does_not_echo_invalid_secret_value() {
        let secret = "config-secret-should-not-leak";
        let figment = Figment::new().merge(("auth.jwt.keys", secret));
        let config = crate::config::repository::ConfigRepository::new(figment);
        let error = config
            .section::<crate::config::JwtConfig>("auth.jwt")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("auth.jwt"));
        assert!(!error.contains(secret));
    }
}
