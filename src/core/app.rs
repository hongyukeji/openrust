//! 应用组装入口：[`Application`]、[`AppBuilder`]、[`App`] 和 [`AppState`]。

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use figment::Figment;
use serde::de::DeserializeOwned;

use crate::config::{ConfigError, FrameworkConfig};
use crate::core::command::Command;
use crate::core::error::{Error, Result};
use crate::core::lifecycle::BackgroundTasks;
use crate::core::plugin::Plugin;

/// 每个项目实现一次，用于描述应用的组装方式。
///
/// ```
/// use openrust::core::{AppBuilder, Application};
/// use openrust::config::NoConfig;
///
/// struct App;
///
/// impl Application for App {
///     type Config = NoConfig;
///
///     fn build(app: &mut AppBuilder) {
///         // app.plugin(DatabasePlugin).routes(routes::register);
///         let _ = app;
///     }
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

    /// 注册插件、路由、命令和服务。
    fn build(app: &mut AppBuilder);
}

/// 处理器或命令请求未注册服务时返回的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("service `{0}` is not registered; call `AppBuilder::insert` from a Plugin")]
pub struct MissingServiceError(pub &'static str);

#[derive(Default)]
struct Registry {
    items: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Registry {
    fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.items.insert(TypeId::of::<T>(), Arc::new(value));
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.items
            .get(&TypeId::of::<T>())
            .and_then(|item| Arc::clone(item).downcast::<T>().ok())
    }

    fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.items.contains_key(&TypeId::of::<T>())
    }
}

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

/// 可变的应用组装阶段。插件在此添加服务、命令和 crate 专属扩展。
pub struct AppBuilder {
    framework: FrameworkConfig,
    figment: Figment,
    services: Registry,
    plugins: Vec<Box<dyn Plugin>>,
    commands: Vec<Box<dyn Command>>,
    extensions: Extensions,
}

impl AppBuilder {
    /// 使用已加载的框架配置及其来源 [`Figment`] 开始组装。
    pub fn new(framework: FrameworkConfig, figment: Figment) -> Self {
        Self {
            framework,
            figment,
            services: Registry::default(),
            plugins: Vec::new(),
            commands: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    /// 框架配置。
    pub fn config(&self) -> &FrameworkConfig {
        &self.framework
    }

    /// 提取一个配置区段，例如 `section::<DatabaseConfig>("database")`。
    pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        extract_section(&self.figment, key)
    }

    pub(crate) fn sensitive_section<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<T, ConfigError> {
        self.figment.extract_inner(key).map_err(|_| {
            ConfigError::Invalid(format!(
                "invalid configuration in `{key}`; check field types and required values (secret values omitted)"
            ))
        })
    }

    /// 排入插件。某个插件构建期间新增的插件也会继续处理。
    pub fn plugin(&mut self, plugin: impl Plugin) -> &mut Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// 注册或替换服务，之后可通过 [`AppState::get`] 获取。
    ///
    /// 同一具体类型再次调用时，后注册的值会替换先前值。插件需要拒绝冲突时使用
    /// [`try_insert`](Self::try_insert)。
    pub fn insert<T: Send + Sync + 'static>(&mut self, service: T) -> &mut Self {
        self.services.insert(service);
        self
    }

    /// 注册服务；若相同具体类型已经注册则返回冲突错误。
    pub fn try_insert<T: Send + Sync + 'static>(&mut self, service: T) -> Result<&mut Self> {
        if self.services.contains::<T>() {
            return Err(Error::custom_with_status(
                http::StatusCode::CONFLICT,
                "duplicate_service_registration",
                format!("service `{}` is already registered", type_name::<T>()),
            ));
        }
        self.services.insert(service);
        Ok(self)
    }

    /// 查找先前插件已注册的服务。
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.services.get()
    }

    /// 获取前序插件注册的服务；服务缺失时返回依赖错误。
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.get().ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "plugin_dependency_missing",
                format!(
                    "required plugin service `{}` is not registered",
                    type_name::<T>()
                ),
            )
        })
    }

    /// 注册或替换运行时命令；同名命令由后注册项替换。
    ///
    /// 插件要拒绝命令名冲突时使用 [`try_command`](Self::try_command)。
    pub fn command(&mut self, command: impl Command) -> &mut Self {
        let name = command.name();
        if let Some(index) = self
            .commands
            .iter()
            .position(|existing| existing.name() == name)
        {
            self.commands[index] = Box::new(command);
        } else {
            self.commands.push(Box::new(command));
        }
        self
    }

    /// 注册运行时命令；无效命令名或同名命令会返回错误。
    pub fn try_command(&mut self, command: impl Command) -> Result<&mut Self> {
        validate_command_name(command.name())?;
        if self
            .commands
            .iter()
            .any(|existing| existing.name() == command.name())
        {
            return Err(Error::custom_with_status(
                http::StatusCode::CONFLICT,
                "duplicate_command_registration",
                format!("command `{}` is already registered", command.name()),
            ));
        }
        self.commands.push(Box::new(command));
        Ok(self)
    }

    /// 可变访问 crate 专属的扩展容器；首次访问时创建。
    ///
    /// 能力 crate 在 `build` 阶段用它收集注册项（路由构建器、迁移、任务），
    /// 并在构建完成后通过 [`App::extension`] 读取。
    pub fn extension_mut<T: Default + Send + Sync + 'static>(&mut self) -> &mut T {
        let entry = self
            .extensions
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()));
        // 此项以 `TypeId::of::<T>()` 为键插入，因此向下转型必定成功。
        entry
            .downcast_mut::<T>()
            .unwrap_or_else(|| unreachable!("extension stored under a mismatched TypeId"))
    }

    pub(crate) fn take_plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        std::mem::take(&mut self.plugins)
    }

    pub(crate) fn has_pending_plugins(&self) -> bool {
        !self.plugins.is_empty()
    }

    /// 冻结构建器。优先使用 [`finalize`](crate::lifecycle::finalize)，它还会运行插件。
    pub fn build(self) -> App {
        App {
            state: AppState {
                inner: Arc::new(StateInner {
                    framework: self.framework,
                    services: self.services,
                    background_tasks: BackgroundTasks::default(),
                }),
            },
            figment: self.figment,
            commands: self.commands,
            extensions: self.extensions,
        }
    }
}

fn validate_command_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.trim() != name
        || name.split_whitespace().collect::<Vec<_>>().join(" ") != name
    {
        return Err(Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_command_name",
            "command names must contain non-empty words separated by single spaces",
        ));
    }
    Ok(())
}

struct StateInner {
    framework: FrameworkConfig,
    services: Registry,
    background_tasks: BackgroundTasks,
}

/// 可低成本克隆、供每个请求和任务共享的句柄，包含配置和服务。
#[derive(Clone)]
pub struct AppState {
    inner: Arc<StateInner>,
}

impl AppState {
    /// 框架配置。
    pub fn config(&self) -> &FrameworkConfig {
        &self.inner.framework
    }

    /// 返回已注册的 `T` 类型服务；若未注册则返回 `None`。
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner.services.get()
    }

    /// 返回已注册的 `T` 类型服务；若未注册则返回包含缺失类型名称的错误。
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.get()
            .ok_or_else(|| Error::internal(MissingServiceError(type_name::<T>())))
    }

    pub(crate) fn background_tasks(&self) -> &BackgroundTasks {
        &self.inner.background_tasks
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("app", &self.inner.framework.app.name)
            .field("services", &self.inner.services.items.len())
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

    /// 使用 `args` 运行名为 `name` 的命令；命令结束后会取消并在停机期限内 join 应用后台刷新任务。
    pub async fn run_command(&self, name: &str, args: Vec<String>) -> Result<()> {
        let result = self.run_command_inner(name, args).await;
        let shutdown = self
            .state
            .background_tasks()
            .shutdown_default(std::time::Duration::from_secs(
                self.config().server.shutdown_timeout_secs,
            ))
            .await;
        match (result, shutdown) {
            (Err(error), Err(shutdown_error)) => {
                tracing::error!(
                    code = shutdown_error.code(),
                    "background task shutdown also failed"
                );
                Err(error)
            }
            (Err(error), Ok(())) => Err(error),
            (Ok(()), result) => result,
        }
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
        let i18n = self.state().try_get::<crate::i18n::I18n>().ok();
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
    use crate::core::{ExecutionContext, plugin::BoxFuture};

    #[derive(Debug, PartialEq)]
    struct Counter(u32);

    #[derive(Default)]
    struct Hooks(Vec<&'static str>);

    #[test]
    fn services_are_typed_and_shared() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.insert(Counter(7));
        let app = builder.build();
        assert_eq!(app.state().get::<Counter>().map(|c| c.0), Some(7));
    }

    #[test]
    fn get_returns_none_for_missing_service() {
        let app = AppBuilder::new(FrameworkConfig::default(), Figment::new()).build();
        assert!(app.state().get::<String>().is_none());
    }

    #[test]
    fn try_get_reports_missing_type_name() {
        let app = AppBuilder::new(FrameworkConfig::default(), Figment::new()).build();
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
    }

    #[test]
    fn extensions_accumulate_across_registrations() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.extension_mut::<Hooks>().0.push("a");
        builder.extension_mut::<Hooks>().0.push("b");
        let app = builder.build();
        assert_eq!(
            app.extension::<Hooks>().map(|h| h.0.clone()),
            Some(vec!["a", "b"])
        );
    }

    #[tokio::test]
    async fn unknown_command_is_reported() {
        let app = AppBuilder::new(FrameworkConfig::default(), Figment::new()).build();
        let error = app.run_command("nope", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `nope`; run `help` to list commands")
        );
    }

    #[tokio::test]
    async fn command_exit_cancels_and_joins_application_background_tasks() -> Result<()> {
        let app = AppBuilder::new(FrameworkConfig::default(), Figment::new()).build();
        let started = Arc::new(tokio::sync::Notify::new());
        let stopped = Arc::new(tokio::sync::Notify::new());
        let started_signal = Arc::clone(&started);
        let stopped_signal = Arc::clone(&stopped);
        app.state()
            .background_tasks()
            .handle()
            .spawn(move |cancellation| async move {
                started_signal.notify_one();
                cancellation.cancelled().await;
                stopped_signal.notify_one();
            })?;
        started.notified().await;
        let error = app.run_command("missing", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        stopped.notified().await;
        Ok(())
    }

    #[tokio::test]
    async fn serve_is_unknown_without_http_plugin() -> Result<()> {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.command(crate::core::command::HelpCommand);
        let app = builder.build();
        let error = app.run_command("serve", Vec::new()).await.err();
        assert_eq!(error.as_ref().map(Error::code), Some("unknown_command"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `serve`; run `help` to list commands")
        );
        Ok(())
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

    #[tokio::test]
    async fn nested_command_consumes_its_name_words() -> Result<()> {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.command(NestedCommand);
        let app = builder.build();
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
    async fn unknown_nested_command_names_input_and_available_command() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.command(NestedCommand);
        let app = builder.build();
        let error = app
            .run_command("config", vec!["foo".into(), "bar".into()])
            .await
            .err();
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unknown command `config foo bar`; available: config show")
        );
    }

    #[tokio::test]
    async fn config_show_rejects_extra_argument() {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
        builder.command(crate::core::command::ConfigShowCommand);
        let app = builder.build();
        let error = app
            .run_command("config", vec!["show".into(), "--typo".into()])
            .await
            .err();
        assert_eq!(error.as_ref().map(Error::code), Some("unexpected_argument"));
        assert_eq!(
            error.as_ref().map(ToString::to_string).as_deref(),
            Some("unexpected argument `--typo` for `config show`; run `help` to list commands")
        );
    }

    #[test]
    fn sensitive_section_error_does_not_echo_invalid_secret_value() {
        let secret = "config-secret-should-not-leak";
        let figment = Figment::new().merge(("auth.jwt.keys", secret));
        let builder = AppBuilder::new(FrameworkConfig::default(), figment);
        let error = builder
            .sensitive_section::<crate::config::JwtConfig>("auth.jwt")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("auth.jwt"));
        assert!(!error.contains(secret));
    }
}
