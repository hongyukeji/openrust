//! 引导启动顺序与关闭信号。
//!
//! [`bootstrap`] 按以下顺序执行：非生产环境加载 `.env`、加载配置、初始化 `tracing`、
//! 提取应用自身的配置、调用 [`Application`] 构建插件、运行每个插件的 `build`、
//! 冻结构建器，最后运行每个插件的 `finish`。

#[cfg(feature = "otel")]
use opentelemetry::trace::TracerProvider as _;
use tracing_subscriber::EnvFilter;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt as _;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::config::{self, ConfigLoader, Environment, LogConfig, LogFormat};
use crate::core::Cancellation;
use crate::core::app::{App, AppBuilder, Application};
use crate::core::command::{ConfigShowCommand, HelpCommand};
use crate::core::error::{Error, Result};
use std::sync::{Arc, Mutex, Weak};
use tokio::{task::JoinSet, time::Instant};

const MAX_BACKGROUND_TASKS: usize = 256;

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
            while tasks.join_next().await.is_some() {}
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
        if tasks.len() >= MAX_BACKGROUND_TASKS {
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
    bootstrap_with::<A>(|_| {}).await
}

/// 与 [`bootstrap`] 类似，但允许调用方在 [`Application::build`] 前添加默认插件。
pub async fn bootstrap_with<A: Application>(defaults: impl FnOnce(&mut AppBuilder)) -> Result<App> {
    let loader = bootstrap_config_loader(A::config_namespace())?;
    let framework = loader.load_framework()?;
    init_tracing(&framework.log);
    tracing::debug!(env = %framework.app.env, config_dir = %loader.config_dir().display(), "configuration loaded");

    let app_config: A::Config = loader.load()?;
    let mut builder = AppBuilder::new(framework, loader.figment());
    builder.insert(app_config);
    builder.command(HelpCommand).command(ConfigShowCommand);
    defaults(&mut builder);
    A::build(&mut builder);
    finalize(builder).await
}

fn bootstrap_config_loader(namespace: &str) -> Result<ConfigLoader> {
    // 先确定运行环境，再加载 `.env` 中的其他键。
    let env_key = format!("{namespace}ENV");
    let env = if std::env::var(&env_key).is_ok_and(|value| !value.trim().is_empty()) {
        Environment::from_env_with_namespace(namespace)?
    } else {
        config::dotenv_environment(namespace)?
    };
    if !env.is_production() {
        config::load_dotenv();
    }
    Ok(ConfigLoader::from_environment_with_namespace(
        env, namespace,
    )?)
}

/// 在组装完成的构建器上运行插件的构建与完成阶段。
///
/// 暴露此入口，使测试无需修改进程环境即可构建应用。
pub async fn finalize(mut builder: AppBuilder) -> Result<App> {
    let mut built = Vec::new();
    while builder.has_pending_plugins() {
        for plugin in builder.take_plugins() {
            tracing::trace!(plugin = plugin.name(), "building");
            plugin.build(&mut builder)?;
            built.push(plugin);
        }
    }
    let app = builder.build();
    for plugin in &built {
        tracing::trace!(plugin = plugin.name(), "finishing");
        plugin.finish(&app).await?;
    }
    Ok(app)
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
    use crate::config::FrameworkConfig;
    use crate::core::plugin::{BoxFuture, Plugin};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    // 每个闭包执行后，Jail 会恢复工作目录和进程环境。
    #[test]
    fn production_bootstrap_ignores_dotenv() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("APP_ENV", "");
            jail.set_env("APP_SERVER__PORT", "");
            jail.clear_env();
            jail.create_file(".env", "APP_ENV=test\nAPP_SERVER__PORT=4999\n")?;
            jail.set_env("APP_ENV", "production");
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment(), Environment::Production);
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                3000
            );
            Ok(())
        });
    }

    #[test]
    fn dotenv_selected_production_does_not_load_other_keys() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.set_env("APP_ENV", "");
            jail.set_env("APP_SERVER__PORT", "");
            jail.clear_env();
            jail.create_file(".env", "APP_ENV=production\nAPP_SERVER__PORT=4999\n")?;
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment(), Environment::Production);
            assert!(std::env::var("APP_ENV").is_err());
            assert!(std::env::var("APP_SERVER__PORT").is_err());
            assert_eq!(
                loader
                    .load_framework()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .server
                    .port,
                3000
            );
            Ok(())
        });
    }

    #[test]
    fn development_bootstrap_loads_dotenv_then_resolves_environment() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            // 在 dotenvy 设置键值前记录两个键，确保 Jail 能恢复其原始值。
            jail.set_env("APP_ENV", "");
            jail.set_env("APP_SERVER__PORT", "");
            jail.clear_env();
            jail.create_file(".env", "APP_ENV=test\nAPP_SERVER__PORT=4999\n")?;
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
    async fn finalize_without_http_exposes_only_builtin_commands() -> Result<()> {
        let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
        builder.command(HelpCommand).command(ConfigShowCommand);
        let app = finalize(builder).await?;
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
    impl Plugin for Second {
        fn build(&self, app: &mut AppBuilder) -> Result<()> {
            app.extension_mut::<Log>()
                .0
                .lock()
                .map(|mut l| l.push("second:build"))
                .ok();
            Ok(())
        }
    }

    struct First;
    impl Plugin for First {
        fn build(&self, app: &mut AppBuilder) -> Result<()> {
            app.extension_mut::<Log>()
                .0
                .lock()
                .map(|mut l| l.push("first:build"))
                .ok();
            app.plugin(Second);
            app.insert(AtomicBool::new(false));
            Ok(())
        }
        fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.state()
                    .try_get::<AtomicBool>()?
                    .store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn plugins_added_during_build_are_processed_and_finished() -> Result<()> {
        let mut builder = AppBuilder::new(
            crate::config::FrameworkConfig::default(),
            figment::Figment::new(),
        );
        builder.plugin(First);
        let app = finalize(builder).await?;
        let log = app
            .extension::<Log>()
            .and_then(|l| l.0.lock().ok().map(|l| l.clone()));
        assert_eq!(log, Some(vec!["first:build", "second:build"]));
        assert!(
            app.state().try_get::<AtomicBool>()?.load(Ordering::SeqCst),
            "finish ran"
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
