//! 引导启动顺序与关闭信号。
//!
//! [`bootstrap`] 加载 `.env` 和配置，按清单完成所有 Provider 的 `register`，
//! 再顺序执行 `boot`、最终配置校验与必需服务解析，最后发布应用。

#[cfg(feature = "otel")]
use opentelemetry::trace::TracerProvider as _;
use tracing_subscriber::EnvFilter;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt as _;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::config::{self, ConfigLoader, Environment, LogConfig, LogFormat};
use crate::core::Cancellation;
use crate::core::app::{App, Application};
use crate::core::command::{ConfigShowCommand, HelpCommand};
use crate::core::error::{Error, Result};
use crate::core::provider::{BootContext, ProviderBuilder, ServiceProvider};
use std::sync::{Arc, Mutex, Weak};
use tokio::{task::JoinSet, time::Instant};

pub(crate) const MAX_BACKGROUND_TASKS: usize = 256;

struct CoreCommandsProvider;

impl ServiceProvider for CoreCommandsProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(HelpCommand);
            app.command(ConfigShowCommand);
            Ok(())
        })
    }
}

/// 应用拥有的合作式后台任务集合，供有界框架刷新任务使用。
#[derive(Default)]
pub(crate) struct BackgroundTasks {
    inner: Arc<BackgroundTaskState>,
}

#[derive(Clone)]
pub(crate) struct BackgroundTaskHandle {
    inner: Weak<BackgroundTaskState>,
}

#[derive(Default)]
struct BackgroundTaskState {
    state: Mutex<BackgroundTaskSet>,
    cancellation: Cancellation,
}

#[derive(Default)]
struct BackgroundTaskSet {
    tasks: Option<JoinSet<()>>,
    closed: bool,
}

impl BackgroundTasks {
    pub(crate) fn cancel(&self) {
        self.inner.cancellation.cancel();
    }
    pub(crate) fn handle(&self) -> BackgroundTaskHandle {
        BackgroundTaskHandle {
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub(crate) fn stop(&self) -> JoinSet<()> {
        self.inner.cancellation.cancel();
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.tasks.take().unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) async fn shutdown_default(&self, timeout: std::time::Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        Self::join_until(self.stop(), deadline).await
    }

    pub(crate) async fn join_until(mut tasks: JoinSet<()>, deadline: Instant) -> Result<()> {
        let joined = tokio::time::timeout_at(deadline, async {
            let mut failed = false;
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result {
                    let _ = error;
                    tracing::error!("application background task failed");
                    failed = true;
                }
            }
            if failed {
                Err(Error::custom(
                    "background_task_failed",
                    "an application background task failed",
                ))
            } else {
                Ok(())
            }
        })
        .await;
        if let Ok(result) = joined {
            result
        } else {
            tasks.abort_all();
            Err(Error::custom(
                "background_task_shutdown_timeout",
                "application background tasks did not stop before the shutdown deadline",
            ))
        }
    }
}

impl BackgroundTaskHandle {
    pub(crate) fn spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawn_with_limit(MAX_BACKGROUND_TASKS, task)
    }

    /// 给父资源所有者的单一子清理驱动保留一个内部任务槽。
    pub(crate) fn spawn_cleanup_driver<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawn_with_limit(MAX_BACKGROUND_TASKS + 1, task)
    }

    fn spawn_with_limit<F, Fut>(&self, limit: usize, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let inner = self.inner.upgrade().ok_or_else(|| {
            Error::custom(
                "background_tasks_stopped",
                "application background tasks have stopped",
            )
        })?;
        let mut state = inner.state.lock().map_err(|_| {
            Error::custom(
                "background_tasks_unavailable",
                "background task registry is unavailable",
            )
        })?;
        if state.closed {
            return Err(Error::custom(
                "background_tasks_stopping",
                "application background tasks are stopping",
            ));
        }
        let tasks = state.tasks.get_or_insert_with(JoinSet::new);
        while let Some(result) = tasks.try_join_next() {
            if let Err(error) = result {
                let _ = error;
                tracing::error!("application background task failed");
            }
        }
        if tasks.len() >= limit {
            return Err(Error::custom(
                "background_task_capacity",
                "application background task capacity is exhausted",
            ));
        }
        let cancellation = inner.cancellation.clone();
        tasks.spawn(async move { task(cancellation).await });
        Ok(())
    }
}

/// 从当前工作目录的配置引导启动 `A`。
pub async fn bootstrap<A: Application>() -> Result<App> {
    bootstrap_with_command::<A>(None).await
}

/// 为实际指定的控制台命令引导启动 `A`，并向 Provider 暴露命令名称。
///
/// 调用方须在启动成功后用相同名称执行 [`App::run_command`]。
pub(crate) async fn bootstrap_for_command<A: Application>(name: &str) -> Result<App> {
    bootstrap_with_command::<A>(Some(name)).await
}

async fn bootstrap_with_command<A: Application>(name: Option<&str>) -> Result<App> {
    let loader = bootstrap_config_loader(A::config_namespace())?;
    let sources = loader.sources()?;
    let app_config: A::Config = sources
        .figment()
        .extract()
        .map_err(crate::config::ConfigError::from)?;
    let mut builder = ProviderBuilder::for_application::<A>(&app_config);
    if let Some(name) = name {
        builder.console_command(name);
    }
    let runtime = builder.start(sources).await?;
    if let Err(error) = runtime.container().resolve::<A::Config>().await {
        if let Err(cleanup_error) = runtime
            .shutdown(Instant::now() + std::time::Duration::from_secs(5))
            .await
        {
            tracing::error!(
                code = cleanup_error.code(),
                "application config failure cleanup failed"
            );
        }
        return Err(error);
    }
    let app = App::from_provider_runtime(runtime)?;
    init_tracing(&app.config().log);
    tracing::debug!(env = %app.config().app.env, config_source = loader.config_dir_source(), "provider configuration loaded");
    Ok(app)
}

impl ProviderBuilder {
    /// 使用与生产引导相同的命令、应用配置绑定和 Provider 清单创建启动器。
    ///
    /// 测试可在调用 [`Self::start`] 前移除 Provider 或覆盖服务绑定。
    pub fn for_application<A: Application>(config: &A::Config) -> Self {
        let mut builder = Self::new();
        builder.app_config::<A::Config>();
        builder.provider(CoreCommandsProvider);
        for provider in A::providers(config) {
            builder.boxed_provider(provider);
        }
        builder.module_manifest(A::modules(config));
        builder
    }
}

fn bootstrap_config_loader(namespace: &str) -> Result<ConfigLoader> {
    // 先确定运行环境，再加载 `.env` 中的其他键。
    let env_key = format!("{namespace}ENV");
    let env = if std::env::var(&env_key).is_ok_and(|value| !value.trim().is_empty()) {
        Environment::from_env_with_namespace(namespace)?
    } else {
        config::dotenv_environment(namespace)?
    };
    config::load_dotenv_for_bootstrap()?;
    Ok(ConfigLoader::from_environment_with_namespace(
        env, namespace,
    )?)
}

/// 安装全局 `tracing` subscriber。第二次调用（例如测试或嵌入式使用）不会执行任何操作。
pub fn init_tracing(log: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&log.level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    #[cfg(feature = "otel")]
    let installed = if let Ok(Some(provider)) = otel_provider() {
        let tracer = provider.tracer("openrust");
        let registry = tracing_subscriber::registry()
            .with(filter)
            .with(tracing_opentelemetry::layer().with_tracer(tracer));
        match log.format {
            LogFormat::Json => registry
                .with(tracing_subscriber::fmt::layer().json())
                .try_init()
                .map_err(|error| error.to_string()),
            LogFormat::Text => registry
                .with(tracing_subscriber::fmt::layer())
                .try_init()
                .map_err(|error| error.to_string()),
        }
    } else {
        let builder = tracing_subscriber::fmt().with_env_filter(filter);
        match log.format {
            LogFormat::Json => builder.json().try_init().map_err(|error| error.to_string()),
            LogFormat::Text => builder.try_init().map_err(|error| error.to_string()),
        }
    };
    #[cfg(not(feature = "otel"))]
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    #[cfg(not(feature = "otel"))]
    let installed = match log.format {
        LogFormat::Json => builder.json().try_init(),
        LogFormat::Text => builder.try_init(),
    };
    // 此错误仅表示 subscriber 已安装，可安全忽略。
    let _ = installed;
    #[cfg(feature = "otel")]
    if OTEL_PROVIDER.get().is_some_and(Result::is_err) {
        tracing::warn!("OpenTelemetry exporter could not be initialized; span export is disabled");
    }
}

#[cfg(feature = "otel")]
static OTEL_PROVIDER: std::sync::OnceLock<
    std::result::Result<Option<opentelemetry_sdk::trace::SdkTracerProvider>, String>,
> = std::sync::OnceLock::new();

#[cfg(feature = "otel")]
fn otel_provider()
-> &'static std::result::Result<Option<opentelemetry_sdk::trace::SdkTracerProvider>, String> {
    OTEL_PROVIDER.get_or_init(|| {
        if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_none()
            && std::env::var_os("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").is_none()
        {
            return Ok(None);
        }
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .map_err(|_| "OTLP span exporter configuration is invalid".to_owned())?;
        Ok(Some(
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .build(),
        ))
    })
}

/// 将已排队的 `OTel` spans 刷新到 collector；无 exporter 时不执行操作。
#[cfg_attr(not(feature = "otel"), allow(clippy::unused_async))]
pub async fn flush_telemetry() {
    #[cfg(feature = "otel")]
    if let Some(Ok(Some(provider))) = OTEL_PROVIDER.get() {
        let provider = provider.clone();
        if !matches!(
            tokio::task::spawn_blocking(move || provider.force_flush()).await,
            Ok(Ok(()))
        ) {
            tracing::warn!("OpenTelemetry span flush failed");
        }
    }
}

/// 收到 `SIGINT`（Ctrl-C）或 Unix 上的 `SIGTERM` 时完成。
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::warn!(%error, "cannot listen for SIGTERM"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
// `figment::Jail` 闭包必须返回 `figment::Result`；其错误类型按设计较大。
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    // 每个闭包执行后，Jail 会恢复工作目录和进程环境。
    #[test]
    fn production_bootstrap_loads_dotenv_without_overriding_process_environment() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("APP_ENV", "");
            jail.set_env("SERVER_PORT", "");
            jail.set_env("APP_PORT", "");
            jail.set_env("PORT", "");
            jail.set_env("APP_URL", "");
            jail.clear_env();
            jail.create_file(
                ".env",
                "APP_ENV=test\nSERVER_PORT=4999\nAPP_PORT=4888\nPORT=4777\nAPP_URL=\"http://localhost:${SERVER_PORT}\"\n",
            )?;
            jail.set_env("APP_ENV", "production");
            jail.set_env("SERVER_PORT", "4001");
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment(), Environment::Production);
            let config = loader
                .load_framework()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(config.app.url, "http://localhost:4001");
            assert_eq!(config.server.port, 4001);
            Ok(())
        });
    }

    #[test]
    fn dotenv_selected_production_loads_other_keys() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("APP_ENV", "");
            jail.set_env("SERVER_PORT", "");
            jail.set_env("APP_URL", "");
            jail.clear_env();
            jail.create_file(
                ".env",
                "APP_ENV=production\nSERVER_PORT=4999\nAPP_URL=\"http://localhost:${SERVER_PORT}\"\n",
            )?;
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment(), Environment::Production);
            let config = loader
                .load_framework()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(config.app.url, "http://localhost:4999");
            assert_eq!(std::env::var("APP_ENV").as_deref(), Ok("production"));
            assert_eq!(std::env::var("SERVER_PORT").as_deref(), Ok("4999"));
            assert_eq!(config.server.port, 4999);
            Ok(())
        });
    }

    #[test]
    fn malformed_dotenv_fails_without_exposing_its_contents() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                ".env",
                "APP_ENV=production\nSECRET_VALUE=do-not-leak\nINVALID LINE",
            )?;
            let Err(error) = bootstrap_config_loader("APP_") else {
                return Err(figment::Error::from("格式错误的 .env 不应启动成功"));
            };
            let message = error.to_string();
            assert!(message.contains(".env"));
            assert!(!message.contains("do-not-leak"));
            assert!(!message.contains("INVALID LINE"));
            Ok(())
        });
    }

    #[test]
    fn development_bootstrap_loads_dotenv_then_resolves_environment() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            // 在 dotenvy 设置键值前记录两个键，确保 Jail 能恢复其原始值。
            jail.set_env("APP_ENV", "");
            jail.set_env("SERVER_PORT", "");
            jail.clear_env();
            jail.create_file(".env", "APP_ENV=test\nSERVER_PORT=4999\n")?;
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment(), Environment::Test);
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                4999
            );
            Ok(())
        });
    }

    #[tokio::test]
    async fn provider_boot_without_http_exposes_only_builtin_commands() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(CoreCommandsProvider);
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        ));
        let app = App::from_provider_runtime(builder.start(sources).await?)?;
        let names: Vec<&str> = app
            .commands()
            .map(crate::core::command::Command::name)
            .collect();
        assert_eq!(names, ["help", "config show"]);
        assert_eq!(
            app.run_command("serve", Vec::new())
                .await
                .err()
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("unknown command `serve`; run `help` to list commands")
        );
        Ok(())
    }

    #[derive(Default)]
    struct Log(Mutex<Vec<&'static str>>);

    struct Second;
    impl ServiceProvider for Second {
        fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
            app.instance(Arc::new(AtomicBool::new(false)));
            Ok(())
        }
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::core::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.extension_mut::<Log>()?
                    .0
                    .lock()
                    .map(|mut l| l.push("second:boot"))
                    .ok();
                Ok(())
            })
        }
    }

    struct First;
    impl ServiceProvider for First {
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::core::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.extension_mut::<Log>()?
                    .0
                    .lock()
                    .map(|mut l| l.push("first:boot"))
                    .ok();
                app.resolve::<AtomicBool>()
                    .await?
                    .store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn all_register_precedes_ordered_provider_boot() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(First).provider(Second);
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        ));
        let app = App::from_provider_runtime(builder.start(sources).await?)?;
        let log = app
            .extension::<Log>()
            .and_then(|l| l.0.lock().ok().map(|l| l.clone()));
        assert_eq!(log, Some(vec!["first:boot", "second:boot"]));
        assert!(
            app.state()
                .resolve::<AtomicBool>()
                .await?
                .load(Ordering::SeqCst)
        );
        Ok(())
    }

    #[tokio::test]
    async fn app_background_tasks_are_cancelled_joined_and_closed() -> Result<()> {
        let tasks = BackgroundTasks::default();
        let handle = tasks.handle();
        let started = Arc::new(tokio::sync::Notify::new());
        let stopped = Arc::new(tokio::sync::Notify::new());
        let task_started = Arc::clone(&started);
        let task_stopped = Arc::clone(&stopped);
        handle.spawn(move |cancellation| async move {
            task_started.notify_one();
            cancellation.cancelled().await;
            task_stopped.notify_one();
        })?;
        started.notified().await;
        tasks
            .shutdown_default(std::time::Duration::from_secs(1))
            .await?;
        stopped.notified().await;
        assert!(handle.spawn(|_| async {}).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn app_background_tasks_abort_after_shutdown_deadline() -> Result<()> {
        let tasks = BackgroundTasks::default();
        let handle = tasks.handle();
        let started = Arc::new(tokio::sync::Notify::new());
        let task_started = Arc::clone(&started);
        handle.spawn(move |_| async move {
            task_started.notify_one();
            std::future::pending::<()>().await;
        })?;
        started.notified().await;
        let result = BackgroundTasks::join_until(
            tasks.stop(),
            Instant::now() + std::time::Duration::from_millis(10),
        )
        .await;
        assert_eq!(
            result.err().as_ref().map(Error::code),
            Some("background_task_shutdown_timeout")
        );
        Ok(())
    }
}
