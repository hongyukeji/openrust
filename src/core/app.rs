//! 应用组装入口：[`Application`]、[`App`] 和 [`AppState`]。

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use figment::Figment;
use serde::de::DeserializeOwned;

use crate::config::{ConfigError, FrameworkConfig};
use crate::core::command::Command;
use crate::core::container::Container;
use crate::core::error::{Error, Result};
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
}

/// 处理器或命令请求未注册服务时返回的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("service `{0}` is not registered; register it in a ServiceProvider")]
pub struct MissingServiceError(pub &'static str);

type Extensions = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

fn extract_section<T: DeserializeOwned>(figment: &Figment, key: &str) -> Result<T, ConfigError> {
    figment.extract_inner(key).map_err(|source| {
        let error = ConfigError::from(source);
        match error {
            ConfigError::Source(source) if source.missing() => ConfigError::Source(source),
            ConfigError::Source(source) => {
                let detail = source.to_string();
                let detail = if detail == "configuration" {
                    format!("invalid configuration in `{key}`")
                } else if detail.starts_with(&format!("{key}.")) || detail == key {
                    format!("invalid configuration: {detail}")
                } else {
                    format!("invalid configuration in `{key}.{detail}`")
                };
                ConfigError::Invalid(format!("{detail} (secret values omitted)"))
            }
            other => other,
        }
    })
}

struct StateInner {
    framework: FrameworkConfig,
    services: Container,
    provider_resources: Arc<ResourceOwner>,
}

/// 可低成本克隆、供每个请求和任务共享的句柄，包含配置和服务。
#[derive(Clone)]
pub struct AppState {
    inner: Arc<StateInner>,
}

impl AppState {
    /// 取得本应用唯一的服务容器句柄。
    pub fn container(&self) -> &Container {
        &self.inner.services
    }

    /// 在根生命周期异步解析服务，包含工厂绑定。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.inner.services.resolve::<T>().await
    }
    /// 框架配置。
    pub fn config(&self) -> &FrameworkConfig {
        &self.inner.framework
    }

    /// 同步读取已就绪的 `T`；异步工厂尚未解析时返回 `None`。
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner.services.get_ready()
    }

    /// 返回已注册的 `T` 类型服务；若未注册则返回包含缺失类型名称的错误。
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
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

/// 完整组装后的应用：不可变状态、命令和 crate 扩展。
pub struct App {
    state: AppState,
    figment: Figment,
    commands: Vec<Box<dyn Command>>,
    extensions: Extensions,
}

impl App {
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

    /// 框架配置。
    pub fn config(&self) -> &FrameworkConfig {
        self.state.config()
    }

    /// 合并后的配置来源，供 `config show` 等命令使用。
    pub fn figment(&self) -> &Figment {
        &self.figment
    }

    /// 提取一个配置区段。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        extract_section(&self.figment, key)
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
        let execution =
            crate::core::ExecutionContext::new(metadata, None, crate::core::Cancellation::new());
        crate::core::with_execution_context(execution, async move {
            if let (Some(i18n), Some(locale)) = (i18n, locale) {
                i18n.with_locale(locale, command.run(self, remaining)).await
            } else {
                command.run(self, remaining).await
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BootContext, BoxFuture, ExecutionContext, ProviderBuilder, RegisterContext};
    use figment::providers::Serialized;

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
            Some("scope_closed".to_owned())
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
