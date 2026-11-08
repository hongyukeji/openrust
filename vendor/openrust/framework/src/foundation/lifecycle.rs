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

#[cfg(test)]
use crate::config::Environment;
use crate::config::{self, ConfigLoader, LogConfig, LogFormat};
use crate::console::command::{
    ConfigCacheCommand, ConfigClearCommand, ConfigShowCommand, EnvironmentCommand, HelpCommand,
    KeyGenerateCommand, ListCommand,
};
use crate::core::Cancellation;
use crate::core::error::{Error, Result};
use crate::foundation::application::Application;
use crate::foundation::application::ApplicationDefinition;
use crate::foundation::provider::BootContext;
use crate::foundation::provider::ProviderBuilder;
use crate::support::ServiceProvider;
use std::sync::{Arc, Mutex, Weak};
use tokio::{task::JoinSet, time::Instant};

mod callback;
pub(crate) use callback::{CallbackDisposal, callback_future};
pub(crate) mod disposal;

pub(crate) const MAX_BACKGROUND_TASKS: usize = 256;

struct CoreCommandsProvider<A> {
    console_environment: Option<String>,
    marker: std::marker::PhantomData<A>,
}

impl<A: ApplicationDefinition> ServiceProvider for CoreCommandsProvider<A> {
    fn boot<'a>(
        &'a self,
        app: &'a mut BootContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(HelpCommand);
            app.command(ListCommand);
            app.command(EnvironmentCommand);
            app.command(ConfigShowCommand);
            app.command(ConfigCacheCommand::<A> {
                console_environment: self.console_environment.clone(),
                marker: std::marker::PhantomData,
            });
            app.command(ConfigClearCommand);
            app.command(KeyGenerateCommand);
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
    resources: Option<super::resources::ResourceRegistrar>,
}

#[derive(Default)]
struct BackgroundTaskState {
    state: Mutex<BackgroundTaskSet>,
    cancellation: Cancellation,
    failures: Arc<std::sync::atomic::AtomicUsize>,
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
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }
    pub(crate) fn failure_counter(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        Arc::clone(&self.inner.failures)
    }
    pub(crate) fn take_failures(&self) -> usize {
        self.inner
            .failures
            .swap(0, std::sync::atomic::Ordering::AcqRel)
    }
    pub(crate) fn handle(&self) -> BackgroundTaskHandle {
        BackgroundTaskHandle {
            inner: Arc::downgrade(&self.inner),
            resources: None,
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
                    drop_isolated(error);
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

/// 与ObservedFuture同一受控poll/Drop规则，普通任务不把用户payload交回Tokio输出。
/// 原owner只有一个失败计数；不新增调度器或任务登记表。
struct ControlledTask<F, Fut: std::future::Future<Output = ()>> {
    task: Option<Box<F>>,
    future: Option<std::pin::Pin<Box<Fut>>>,
    cancellation: Cancellation,
    failures: Arc<std::sync::atomic::AtomicUsize>,
    finished: bool,
    disposal: disposal::TaskTicket,
}
impl<F, Fut: std::future::Future<Output = ()>> ControlledTask<F, Fut> {
    fn finish(&mut self, interrupted: bool) {
        if self.finished {
            return;
        }
        disposal::scope(
            Some(disposal::Credential::Task(self.disposal.clone())),
            || {
                if interrupted {
                    disposal::failure(None);
                }
                disposal::dispose(self.task.take(), None);
                disposal::dispose(self.future.take(), None);
            },
        );
        // 对应同步上下文已恢复后，才结束任务归责并提交原聚合计数。
        if self.disposal.finish() {
            self.failures
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        self.finished = true;
    }
}
impl<F, Fut: std::future::Future<Output = ()>> Drop for ControlledTask<F, Fut> {
    fn drop(&mut self) {
        self.finish(false);
    }
}
impl<F, Fut> std::future::Future for ControlledTask<F, Fut>
where
    F: FnOnce(Cancellation) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        let this = self.get_mut();
        let (polled, _) = disposal::scope(
            Some(disposal::Credential::Task(this.disposal.clone())),
            || {
                if let Some(task) = this.task.take() {
                    let Ok(future) = catch_isolated(|| task(this.cancellation.clone())) else {
                        disposal::failure(None);
                        return Err(());
                    };
                    this.future = Some(Box::pin(future));
                }
                let polled = catch_isolated(|| {
                    this.future
                        .as_mut()
                        .map_or(std::task::Poll::Ready(()), |future| {
                            future.as_mut().poll(context)
                        })
                });
                if polled.is_err() {
                    disposal::failure(None);
                }
                polled
            },
        );
        if let Ok(std::task::Poll::Pending) = polled {
            std::task::Poll::Pending
        } else {
            this.finish(false);
            std::task::Poll::Ready(())
        }
    }
}

/// 接收后直接持有并poll真正用户Fut；框架await的操作数是此安全持有者。
pub(crate) fn controlled_task<F, Fut>(
    task: F,
    cancellation: Cancellation,
    failures: Arc<std::sync::atomic::AtomicUsize>,
) -> impl std::future::Future<Output = ()>
where
    F: FnOnce(Cancellation) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    ControlledTask::<F, Fut> {
        task: Some(Box::new(task)),
        future: None,
        cancellation,
        failures,
        finished: false,
        disposal: disposal::TaskTicket::admitted(),
    }
}

/// raw交付的单次诊断通知，不授予清理资格或给owner重复归账。
#[derive(Clone)]
pub(crate) struct FutureFailure {
    failed: Arc<std::sync::atomic::AtomicBool>,
    observer: Arc<dyn Fn() + Send + Sync>,
}
impl FutureFailure {
    pub(crate) fn new(observer: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            failed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            observer: Arc::new(observer),
        }
    }
    pub(crate) fn note_diagnostic(&self) {
        if !self.failed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            (self.observer)();
        }
    }
}

/// 执行原错和清理未知同时保留，格式仅含稳定码。
pub(crate) struct TaskExecutionFailure {
    pub(crate) primary: Error,
    pub(crate) cleanup: Error,
}
impl std::fmt::Debug for TaskExecutionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskExecutionFailure")
            .field("primary", &self.primary.code())
            .field("cleanup", &self.cleanup.code())
            .finish()
    }
}
impl std::fmt::Display for TaskExecutionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "task failed ({}) with interrupted cleanup ({})",
            self.primary.code(),
            self.cleanup.code()
        )
    }
}
impl std::error::Error for TaskExecutionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}
impl TaskExecutionFailure {
    pub(crate) fn error(primary: Error, cleanup: Error) -> Error {
        Error::http_with_source(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "resource_task_interrupted",
            "执行与完成清理包含异常",
            Self { primary, cleanup },
        )
    }
}

/// 内部登记结果；调用方释放外层资源锁后才可 finish。
pub(crate) struct TaskSubmission<F> {
    result: Result<()>,
    rejected: Option<F>,
    completed: Vec<tokio::task::JoinError>,
}
/// 登记结论与清理未知是两个独立维度，任意回收前已经拆开保存。
pub(crate) struct TaskSubmissionOutcome {
    pub(crate) registration: Result<()>,
    pub(crate) cleanup_interrupted: bool,
}
impl TaskSubmissionOutcome {
    /// 只在锁外交付用户F；已知无人接收不能转成Ok。
    pub(crate) fn handoff<F>(mut self, sender: tokio::sync::oneshot::Sender<F>, task: F) -> Self {
        let abandoned = if self.registration.is_ok() && !self.cleanup_interrupted {
            match sender.send(task) {
                Ok(()) => None,
                Err(task) => {
                    self.registration = Err(Error::custom(
                        "background_task_handoff_failed",
                        "后台任务未收到用户闭包，未执行本次请求",
                    ));
                    Some(task)
                }
            }
        } else {
            drop(sender);
            Some(task)
        };
        self.cleanup_interrupted |= disposal::dispose(abandoned, None).seen;
        self
    }
    pub(crate) fn into_result(self) -> Result<()> {
        match (self.registration, self.cleanup_interrupted) {
            (Err(registration), true) => Err(TaskSubmissionFailure::error(
                registration,
                task_cleanup_interrupted(),
            )),
            (result, false) => result,
            (Ok(()), true) => Err(task_cleanup_interrupted()),
        }
    }
}
pub(crate) fn task_cleanup_interrupted() -> Error {
    Error::custom(
        "background_task_cleanup_interrupted",
        "后台任务回收异常，不能确认正常清理",
    )
}

/// 每个独立值单独析构；捕获到的panic payload也逐次在非unwind状态下释放。
/// 单个用户Drop内部自行双panic/abort或不返回不在可恢复边界内。
pub(crate) fn drop_isolated<T>(value: T) -> bool {
    let mut failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))).err();
    let interrupted = failure.is_some();
    while let Some(payload) = failure {
        failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))).err();
    }
    interrupted
}

/// 框架持有的标准集合按原迭代次序逐项析构；有限独立异常不会展开剩余集合。
pub(crate) fn drop_all_isolated<T>(values: impl IntoIterator<Item = T>) -> usize {
    let mut failures = 0;
    for value in values {
        failures += usize::from(drop_isolated(value));
    }
    failures
}

/// 捕获后先完整处置panic payload，再返回无payload的稳定未知标记。
pub(crate) fn catch_isolated<T>(action: impl FnOnce() -> T) -> std::result::Result<T, ()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            drop_isolated(payload);
            Err(())
        }
    }
}

impl<F> TaskSubmission<F> {
    /// 无外层锁；不让一个值的unwind携带其余Vec元素进入析构。
    pub(crate) fn finish(self) -> TaskSubmissionOutcome {
        let Self {
            result: registration,
            rejected,
            mut completed,
        } = self;
        let completed_count = completed.len();
        let mut cleanup_interrupted = disposal::dispose(rejected, None).seen;
        while let Some(error) = completed.pop() {
            cleanup_interrupted |= disposal::dispose(error, None).seen;
        }
        if completed_count != 0 {
            tracing::error!(
                failures = completed_count,
                "application background task failed"
            );
        }
        TaskSubmissionOutcome {
            registration,
            cleanup_interrupted,
        }
    }
}

/// 拒绝登记与其清理未知的双诊断；Display/Debug只公开稳定码。
pub(crate) struct TaskSubmissionFailure {
    pub(crate) registration: Error,
    pub(crate) cleanup: Error,
}
impl std::fmt::Debug for TaskSubmissionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskSubmissionFailure")
            .field("registration", &self.registration.code())
            .field("cleanup", &self.cleanup.code())
            .finish()
    }
}
impl std::fmt::Display for TaskSubmissionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "task registration failed ({}) and cleanup failed ({})",
            self.registration.code(),
            self.cleanup.code()
        )
    }
}
impl std::error::Error for TaskSubmissionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cleanup)
    }
}
impl TaskSubmissionFailure {
    pub(crate) fn error(registration: Error, cleanup: Error) -> Error {
        Error::http_with_source(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "resource_task_interrupted",
            "任务拒绝后的清理异常，不能确认完成",
            Self {
                registration,
                cleanup,
            },
        )
    }
}

#[cfg(test)]
std::thread_local! { static TEST_TASK_LIMIT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_test_task_limit<T>(limit: usize, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_TASK_LIMIT.set(self.0);
        }
    }
    let _restore = Restore(TEST_TASK_LIMIT.replace(Some(limit)));
    action()
}

impl BackgroundTaskHandle {
    pub(crate) fn with_resources(mut self, resources: super::resources::ResourceRegistrar) -> Self {
        self.resources = Some(resources);
        self
    }

    pub(crate) fn spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut cleanup = self
            .resources
            .as_ref()
            .and_then(super::resources::ResourceRegistrar::begin_cleanup);
        let credential = cleanup
            .as_ref()
            .map(|cleanup| disposal::Credential::Cleanup(cleanup.disposal_ticket()));
        let failures = self.inner.upgrade().map_or_else(
            || Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            |inner| Arc::clone(&inner.failures),
        );
        let (sender, receiver) = tokio::sync::oneshot::channel::<F>();
        let submission = self.submit(move |cancel| async move {
            if let Ok(task) = receiver.await {
                controlled_task(task, cancel, failures).await;
            }
        });
        let (outcome, _) =
            disposal::scope(credential, || submission.finish().handoff(sender, task));
        if let Some(cleanup) = &mut cleanup {
            cleanup.finish(0);
        }
        outcome.into_result()
    }

    pub(crate) fn submit<F, Fut>(&self, task: F) -> TaskSubmission<F>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        #[cfg(test)]
        let limit = TEST_TASK_LIMIT.get().unwrap_or(MAX_BACKGROUND_TASKS);
        #[cfg(not(test))]
        let limit = MAX_BACKGROUND_TASKS;
        self.submit_with_limit(limit, task)
    }

    /// 父资源所有者的子清理驱动仍使用原保留槽，不新增dispatcher。
    pub(crate) fn submit_cleanup_driver<F, Fut>(&self, task: F) -> TaskSubmission<F>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        #[cfg(test)]
        let limit = TEST_TASK_LIMIT.get().unwrap_or(MAX_BACKGROUND_TASKS) + 1;
        #[cfg(not(test))]
        let limit = MAX_BACKGROUND_TASKS + 1;
        self.submit_with_limit(limit, task)
    }

    fn submit_with_limit<F, Fut>(&self, limit: usize, task: F) -> TaskSubmission<F>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut submission = TaskSubmission {
            result: Ok(()),
            rejected: Some(task),
            completed: Vec::new(),
        };
        submission.result = (|| {
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
            tokio::runtime::Handle::try_current().map_err(|_| {
                Error::custom("resource_runtime_missing", "受管任务需要 Tokio 运行时")
            })?;
            let tasks = state.tasks.get_or_insert_with(JoinSet::new);
            while let Some(result) = tasks.try_join_next() {
                if let Err(error) = result {
                    submission.completed.push(error);
                }
            }
            if tasks.len() >= limit {
                return Err(Error::custom(
                    "background_task_capacity",
                    "application background task capacity is exhausted",
                ));
            }
            let task = submission.rejected.take().ok_or_else(|| {
                Error::custom(
                    "background_tasks_unavailable",
                    "background task submission is unavailable",
                )
            })?;
            let cancellation = inner.cancellation.clone();
            tasks.spawn(ControlledTask::<F, Fut> {
                task: Some(Box::new(task)),
                future: None,
                cancellation,
                failures: Arc::clone(&inner.failures),
                finished: false,
                disposal: disposal::TaskTicket::admitted(),
            });
            Ok(())
        })();
        submission
    }

    /// 仅旧JoinError回收边界的fixture；生产准入全部使用ControlledTask。
    #[cfg(test)]
    pub(crate) fn test_legacy_spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| Error::custom("test_owner_missing", "缺少测试任务owner"))?;
        let mut state = inner
            .state
            .lock()
            .map_err(|_| Error::custom("test_tasks_unavailable", "测试任务锁不可用"))?;
        if state.closed {
            return Err(Error::custom("test_tasks_closed", "测试任务已关闭"));
        }
        let cancellation = inner.cancellation.clone();
        state
            .tasks
            .get_or_insert_with(JoinSet::new)
            .spawn(async move { task(cancellation).await });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_lock_available(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.state.try_lock().is_ok())
    }
}

/// 从当前工作目录的配置引导启动 `A`。
pub async fn bootstrap<A: ApplicationDefinition>() -> Result<Application> {
    bootstrap_with_command::<A>(None, None, None).await
}

/// 测试现有命令引导路径；生产控制台入口使用 [`bootstrap_for_console`]。
#[cfg(test)]
pub(crate) async fn bootstrap_for_command<A: ApplicationDefinition>(
    name: &str,
) -> Result<Application> {
    bootstrap_with_command::<A>(Some(name), None, None).await
}

/// 使用显式 console 环境文件选择启动，不读取进程 argv。
pub async fn bootstrap_for_console<A: ApplicationDefinition>(
    name: &str,
    console_environment: Option<&str>,
) -> Result<Application> {
    bootstrap_with_command::<A>(Some(name), None, console_environment).await
}

pub(crate) async fn bootstrap_for_cache<A: ApplicationDefinition>(
    path: std::path::PathBuf,
    console_environment: Option<&str>,
) -> Result<Application> {
    bootstrap_with_command::<A>(Some("config:cache"), Some(path), console_environment).await
}

async fn bootstrap_with_command<A: ApplicationDefinition>(
    name: Option<&str>,
    forced_path: Option<std::path::PathBuf>,
    console_environment: Option<&str>,
) -> Result<Application> {
    let namespace = A::config_namespace();
    let manifest = A::config();
    let config::ApplicationSources {
        sources,
        cache_path,
        label,
        key_environment,
    } = if let Some(path) = forced_path {
        if manifest.has_typed() {
            return Err(config::ConfigError::Invalid(
                "已编译配置含运行期 Rust 类型，不能创建配置缓存".to_owned(),
            )
            .into());
        }
        let external = config::EnvironmentSnapshot::capture();
        let app_dir = std::env::current_dir()
            .map_err(|_| config::ConfigError::Invalid("无法定位应用根目录".into()))?;
        let dotenv_path =
            config::selected_dotenv_path(&app_dir, namespace, &external, console_environment);
        let environment = external.with_dotenv_at(&dotenv_path)?;
        let env = config::selected_environment(namespace, &environment)?;
        let loader = ConfigLoader::from_snapshot(env, namespace, environment)?;
        let key_variable = format!("{namespace}KEY");
        config::ApplicationSources {
            sources: loader.code_sources(&manifest)?,
            cache_path: path,
            label: "configured",
            key_environment: config::KeyGenerationEnvironment {
                dotenv_path,
                external_key_present: external.present(&key_variable),
                key_variable,
            },
        }
    } else {
        let app_dir = std::env::current_dir()
            .map_err(|_| config::ConfigError::Invalid("无法定位应用根目录".into()))?;
        ConfigLoader::resolve_application_sources(
            &app_dir,
            namespace,
            console_environment,
            Some(&manifest),
        )?
    };
    let app_config: A::Config = sources
        .figment()
        .extract()
        .map_err(crate::config::ConfigError::from)?;
    let mut builder =
        ProviderBuilder::for_application_with_console::<A>(&app_config, console_environment);
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
    let mut app = Application::from_provider_runtime(runtime)?;
    app.set_config_cache_path(cache_path);
    app.set_key_generation_environment(key_environment);
    init_tracing(&app.config().log);
    tracing::debug!(env = %app.config().app.env, config_source = label, "provider configuration loaded");
    Ok(app)
}

impl ProviderBuilder {
    /// 使用与生产引导相同的命令、应用配置绑定和 Provider 清单创建启动器。
    ///
    /// 测试可在调用 [`Self::start`] 前移除 Provider 或覆盖服务绑定。
    pub fn for_application<A: ApplicationDefinition>(config: &A::Config) -> Self {
        Self::for_application_with_console::<A>(config, None)
    }

    fn for_application_with_console<A: ApplicationDefinition>(
        config: &A::Config,
        console_environment: Option<&str>,
    ) -> Self {
        let mut builder = Self::new();
        builder.app_config::<A::Config>();
        builder.bootstrap_provider(CoreCommandsProvider::<A> {
            console_environment: console_environment.map(str::to_owned),
            marker: std::marker::PhantomData,
        });
        builder.default_providers(A::default_providers());
        for provider in A::package_providers(config) {
            builder.boxed_provider(provider);
        }
        for provider in A::additional_providers() {
            builder.boxed_provider(provider);
        }
        if A::with_bootstrap_providers() {
            for provider in A::providers(config) {
                builder.boxed_provider(provider);
            }
        }
        A::configure_providers(config, &mut builder);
        builder
    }
}

#[cfg(test)]
fn bootstrap_config_loader(namespace: &str) -> Result<ConfigLoader> {
    let environment = config::EnvironmentSnapshot::capture().with_dotenv()?;
    let env = config::selected_environment(namespace, &environment)?;
    Ok(ConfigLoader::from_snapshot(env, namespace, environment)?)
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

    struct CommandFixture;
    impl ApplicationDefinition for CommandFixture {
        type Config = crate::config::NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }

        fn config() -> crate::config::ConfigManifest {
            crate::config::ConfigManifest::new().with_toml()
        }

        fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(CommandFixtureProvider)]
        }
    }

    struct CommandFixtureProvider;
    impl ServiceProvider for CommandFixtureProvider {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.merge_defaults("package", serde_json::json!({ "items": ["default"] }))
            })
        }

        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.overlay("example", serde_json::json!({ "greeting": "boot" }))?;
                Ok(())
            })
        }
    }

    struct ShopFixture;
    impl ApplicationDefinition for ShopFixture {
        type Config = crate::config::NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }

        fn config() -> crate::config::ConfigManifest {
            crate::config::ConfigManifest::new().with_toml()
        }

        fn config_namespace() -> &'static str {
            "SHOP_"
        }
    }

    struct CodeModuleFixture;
    impl ApplicationDefinition for CodeModuleFixture {
        type Config = crate::config::NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }

        fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(CodeModuleProvider)]
        }
    }

    struct CodeModuleProvider;
    impl ServiceProvider for CodeModuleProvider {
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.code_config(
                    &crate::config::ConfigManifest::new().section("example", |ctx| {
                        Ok(serde_json::json!({"greeting": ctx.env_or("GREETING_TOKEN", "missing")}))
                    }),
                )
            })
        }
    }

    struct TypedCodeModuleFixture;
    impl ApplicationDefinition for TypedCodeModuleFixture {
        type Config = crate::config::NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }

        fn providers(_: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(TypedCodeModuleProvider)]
        }
    }

    struct TypedCodeModuleProvider;
    impl ServiceProvider for TypedCodeModuleProvider {
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.code_config(
                    &crate::config::ConfigManifest::new()
                        .section("example", |_| Ok(serde_json::json!({"greeting": "hello"})))
                        .typed("example.runtime_policy", |_| Ok(17_u32)),
                )
            })
        }
    }

    #[test]
    fn module_typed_config_and_raw_namespace_coexist_but_refuse_cache_command() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<TypedCodeModuleFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    app.section::<String>("example.greeting")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    "hello"
                );
                assert_eq!(
                    app.config_repository()
                        .typed::<u32>("example.runtime_policy")
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .as_deref(),
                    Some(&17)
                );
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let error = bootstrap_for_console::<TypedCodeModuleFixture>("config:cache", None)
                    .await
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                assert!(error.contains("runtime Rust configuration"));
                Ok(())
            })
        });
    }

    #[test]
    fn module_code_uses_startup_snapshot_and_cache_keeps_final_value() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(".env", "GREETING_TOKEN=from-module-code\n")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap_for_console::<CodeModuleFixture>("config:cache", None)
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    app.section::<String>("example.greeting")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    "from-module-code"
                );
                app.run_command("config:cache", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                std::fs::write(".env", "broken dotenv line")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let cached = bootstrap::<CodeModuleFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    cached
                        .section::<String>("example.greeting")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    "from-module-code"
                );
                cached
                    .shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                Ok(())
            })
        });
    }

    #[test]
    fn custom_namespace_cache_hit_skips_dotenv_and_file_configuration() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.set_env("SHOP_CONFIG_CACHE", "bootstrap/cache/shop.json");
            jail.set_env("SHOP_ENV", "invalid");
            jail.set_env("SHOP_KEY", "");
            jail.create_file(".env", "invalid dotenv line")?;
            jail.create_file(".env.local", "SHOP_NAME=ignored\n")?;
            jail.create_dir("bootstrap")?;
            jail.create_dir("bootstrap/cache")?;
            let mut value = serde_json::to_value(crate::config::FrameworkConfig::default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
            value["app"]["name"] = serde_json::json!("shop-cached");
            jail.create_file(
                "bootstrap/cache/shop.json",
                &serde_json::to_string(&value)
                    .map_err(|error| figment::Error::from(error.to_string()))?,
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap_for_console::<ShopFixture>("key:generate", Some("local"))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(app.config().app.name, "shop-cached");
                let (path, variable, external) = app
                    .key_generation_environment()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    path.file_name().and_then(std::ffi::OsStr::to_str),
                    Some(".env")
                );
                assert_eq!(variable, "SHOP_KEY");
                assert!(external);
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                Ok(())
            })
        });
    }

    #[test]
    fn clear_removes_only_selected_cache_when_two_paths_exist() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(".env", "APP_CONFIG_CACHE=bootstrap/cache/alternate.json\n")?;
            jail.create_dir("bootstrap")?;
            jail.create_dir("bootstrap/cache")?;
            let value = serde_json::to_string(&crate::config::FrameworkConfig::default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
            jail.create_file("bootstrap/cache/config.json", &value)?;
            jail.create_file("bootstrap/cache/alternate.json", &value)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(
                    app.config_cache_path()
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .ends_with("config.json")
                );
                app.run_command("config:cache", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(std::path::Path::new("bootstrap/cache/alternate.json").exists());
                app.run_command("config:clear", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(!std::path::Path::new("bootstrap/cache/config.json").exists());
                assert!(std::path::Path::new("bootstrap/cache/alternate.json").exists());

                let alternate = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(
                    alternate
                        .config_cache_path()
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .ends_with("alternate.json")
                );
                alternate
                    .run_command("config:clear", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                alternate
                    .shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(!std::path::Path::new("bootstrap/cache/alternate.json").exists());
                Ok(())
            })
        });
    }

    #[test]
    fn maintenance_commands_reject_corrupt_cache_without_deleting_it() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_dir("bootstrap")?;
            jail.create_dir("bootstrap/cache")?;
            jail.create_file("bootstrap/cache/config.json", "not json")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                for name in ["config:clear", "config:cache"] {
                    let error = bootstrap_for_command::<CommandFixture>(name)
                        .await
                        .err()
                        .ok_or_else(|| figment::Error::from("损坏缓存不得启动维护命令"))?;
                    assert!(error.to_string().contains("缓存文件格式无效"));
                    assert_eq!(
                        std::fs::read("bootstrap/cache/config.json")
                            .map_err(|error| figment::Error::from(error.to_string()))?,
                        b"not json"
                    );
                }
                Ok(())
            })
        });
    }

    #[test]
    fn clear_command_uses_cached_boot_then_removes_only_selected_file() {
        let _environment_guard = crate::lock_test_env();
        for broken_dotenv in [true, false] {
            figment::Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_dir("bootstrap")?;
                jail.create_dir("bootstrap/cache")?;
                jail.create_dir("config")?;
                jail.create_file(
                    ".env",
                    if broken_dotenv {
                        "broken dotenv line"
                    } else {
                        "APP_NAME=fresh\n"
                    },
                )?;
                jail.create_file(
                    "config/app.toml",
                    if broken_dotenv {
                        "[app]\nname = 'fresh'\n"
                    } else {
                        "broken toml ["
                    },
                )?;
                let mut value = serde_json::to_value(crate::config::FrameworkConfig::default())
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                value["app"]["name"] = serde_json::json!("cached-before-clear");
                let compiled = serde_json::to_string(&value)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                jail.create_file("bootstrap/cache/config.json", &compiled)?;
                jail.create_file("bootstrap/cache/alternate.json", &compiled)?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                runtime.block_on(async {
                    let app = bootstrap_for_command::<CommandFixture>("config:clear")
                        .await
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                    assert!(app.config_repository().is_cached());
                    assert_eq!(app.config().app.name, "cached-before-clear");
                    assert!(std::path::Path::new("bootstrap/cache/config.json").exists());
                    app.run_command("config:clear", Vec::new())
                        .await
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                    assert!(!std::path::Path::new("bootstrap/cache/config.json").exists());
                    assert!(std::path::Path::new("bootstrap/cache/alternate.json").exists());
                    assert_eq!(app.config().app.name, "cached-before-clear");
                    app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                        .await
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                    assert!(bootstrap::<CommandFixture>().await.is_err());
                    Ok(())
                })
            });
        }
    }

    #[test]
    fn cache_command_uses_cached_boot_then_fresh_failure_clears_old_file() {
        let _environment_guard = crate::lock_test_env();
        for broken_dotenv in [true, false] {
            figment::Jail::expect_with(|jail| {
                jail.clear_env();
                jail.create_dir("bootstrap")?;
                jail.create_dir("bootstrap/cache")?;
                jail.create_dir("config")?;
                jail.create_file(
                    ".env",
                    if broken_dotenv {
                        "broken dotenv line"
                    } else {
                        "APP_NAME=fresh\n"
                    },
                )?;
                jail.create_file(
                    "config/app.toml",
                    if broken_dotenv {
                        "[app]\nname = 'fresh'\n"
                    } else {
                        "broken toml ["
                    },
                )?;
                let mut value = serde_json::to_value(crate::config::FrameworkConfig::default())
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                value["app"]["name"] = serde_json::json!("cached-before-rebuild");
                jail.create_file(
                    "bootstrap/cache/config.json",
                    &serde_json::to_string(&value)
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                )?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                runtime.block_on(async {
                    let app = bootstrap_for_command::<CommandFixture>("config:cache")
                        .await
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                    assert!(app.config_repository().is_cached());
                    assert_eq!(app.config().app.name, "cached-before-rebuild");
                    assert!(std::path::Path::new("bootstrap/cache/config.json").exists());
                    assert!(app.run_command("config:cache", Vec::new()).await.is_err());
                    assert!(!std::path::Path::new("bootstrap/cache/config.json").exists());
                    assert_eq!(app.config().app.name, "cached-before-rebuild");
                    app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                        .await
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                    Ok(())
                })
            });
        }
    }

    #[test]
    fn absolute_cache_path_does_not_clear_default_file() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            let root =
                std::env::current_dir().map_err(|error| figment::Error::from(error.to_string()))?;
            let selected = root.join("alternate.json");
            jail.set_env("APP_CONFIG_CACHE", selected.to_string_lossy().as_ref());
            let value = serde_json::to_string(&crate::config::FrameworkConfig::default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
            jail.create_dir("bootstrap")?;
            jail.create_dir("bootstrap/cache")?;
            jail.create_file("bootstrap/cache/config.json", &value)?;
            jail.create_file("alternate.json", &value)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    app.config_cache_path()
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    selected.as_path()
                );
                app.run_command("config:clear", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(!selected.exists());
                assert!(root.join("bootstrap/cache/config.json").exists());
                Ok(())
            })
        });
    }

    #[test]
    fn cache_command_uses_fresh_application_and_next_boot_reads_compiled_value() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_dir("config")?;
            jail.create_file("config/app.toml", "[app]\nname = 'before'\n")?;
            jail.create_file(".env", "APP_NAME=before\n")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let original = bootstrap_for_command::<CommandFixture>("config:cache")
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(original.config().app.name, "before");
                std::fs::write("config/app.toml", "[app]\nname = 'after'\n")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                std::fs::write(".env", "APP_NAME=after\n")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                original
                    .run_command("config:cache", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(original.config().app.name, "before");
                assert!(std::env::var("APP_NAME").is_err());
                original
                    .shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                std::fs::write("config/app.toml", "broken toml [")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                std::fs::write(".env", "broken dotenv line")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let cached = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(cached.config().app.name, "after");
                assert_eq!(
                    cached
                        .config_repository()
                        .get("package.items")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    Some(serde_json::json!(["default"]))
                );
                assert_eq!(
                    cached
                        .config_repository()
                        .get("example.greeting")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    Some(serde_json::json!("boot"))
                );
                cached
                    .run_command("config:clear", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                cached
                    .shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(bootstrap::<CommandFixture>().await.is_err());
                Ok(())
            })
        });
    }

    #[test]
    fn failed_fresh_boot_leaves_previous_cache_cleared() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_dir("config")?;
            jail.create_file("config/app.toml", "[app]\nname = 'first'\n")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let path = app
                    .config_cache_path()
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .to_path_buf();
                crate::config::compiled::write(
                    &path,
                    &app.config_repository()
                        .all()
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                )
                .map_err(|error| figment::Error::from(error.to_string()))?;
                std::fs::write("config/app.toml", "broken toml [")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(app.run_command("config:cache", Vec::new()).await.is_err());
                assert!(!path.exists());
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                Ok(())
            })
        });
    }

    #[test]
    fn dotenv_cache_path_is_selected_after_environment_loading() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                ".env",
                "APP_CONFIG_CACHE=bootstrap/cache/alternate.json\nAPP_CONFIG_DIR=missing-dir\nAPP_ENV=invalid\n",
            )?;
            jail.create_dir("bootstrap")?;
            jail.create_dir("bootstrap/cache")?;
            let mut value = serde_json::to_value(crate::config::FrameworkConfig::default())
                .map_err(|error| figment::Error::from(error.to_string()))?;
            value["app"]["name"] = serde_json::json!("alternate-cache");
            value["app"]["env"] = serde_json::json!("Cached-Staging");
            jail.create_file(
                "bootstrap/cache/alternate.json",
                &serde_json::to_string(&value)
                    .map_err(|error| figment::Error::from(error.to_string()))?,
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(app.config().app.name, "alternate-cache");
                assert_eq!(app.environment().as_str(), "Cached-Staging");
                let (path, variable, external) = app
                    .key_generation_environment()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    path.file_name().and_then(std::ffi::OsStr::to_str),
                    Some(".env")
                );
                assert_eq!(variable, "APP_KEY");
                assert!(!external);
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                Ok(())
            })
        });
    }

    #[test]
    fn console_config_cache_fresh_bootstrap_keeps_selected_environment_file() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(".env", "APP_NAME=plain\n")?;
            jail.create_file(
                ".env.local",
                "APP_NAME=selected\nAPP_ENV=testing\nNOTE=\"before\nAPP_KEY=\nafter\"\nAPP_KEY=\n",
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap_for_console::<CommandFixture>("config:cache", Some("local"))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(app.config().app.name, "selected");
                let (dotenv, variable, external) = app
                    .key_generation_environment()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    dotenv.file_name().and_then(std::ffi::OsStr::to_str),
                    Some(".env.local")
                );
                assert_eq!(variable, "APP_KEY");
                assert!(!external);
                app.run_command("key:generate", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let key = app
                    .section::<String>("app.key")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(key.starts_with("base64:"));
                let selected = std::fs::read_to_string(dotenv)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert!(
                    selected
                        .lines()
                        .any(|line| line.strip_prefix("APP_KEY=") == Some(key.as_str()))
                );
                assert!(selected.contains("NOTE=\"before\nAPP_KEY=\nafter\"\n"));
                let root = std::fs::read_to_string(".env")
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(root, "APP_NAME=plain\n");
                app.run_command("config:cache", Vec::new())
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let path = app
                    .config_cache_path()
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let cached = crate::config::compiled::load(path)
                    .map_err(|error| figment::Error::from(error.to_string()))?
                    .ok_or_else(|| figment::Error::from("缺少配置缓存"))?;
                assert_eq!(
                    cached.figment().extract_inner::<String>("app.name")?,
                    "selected"
                );
                app.shutdown(Instant::now() + std::time::Duration::from_secs(5))
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                Ok(())
            })
        });
    }

    #[test]
    fn cache_miss_preserves_custom_environment_value() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                ".env",
                "APP_CONFIG_CACHE=bootstrap/cache/missing.json\nAPP_ENV=invalid\n",
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            runtime.block_on(async {
                let app = bootstrap::<CommandFixture>()
                    .await
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(app.environment().as_str(), "invalid");
                assert!(!app.is_local());
                assert!(!app.running_unit_tests());
                Ok(())
            })
        });
    }

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
            assert_eq!(
                loader.environment_override(),
                Some(&Environment::Production)
            );
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
            assert_eq!(
                loader.environment_override(),
                Some(&Environment::Production)
            );
            let config = loader
                .load_framework()
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(config.app.url, "http://localhost:4999");
            assert!(std::env::var("APP_ENV").is_err());
            assert!(std::env::var("SERVER_PORT").is_err());
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
            // 外部环境保持独立，`.env` 只供本次加载器使用。
            jail.set_env("APP_ENV", "");
            jail.set_env("SERVER_PORT", "");
            jail.clear_env();
            jail.create_file(".env", "APP_ENV=test\nSERVER_PORT=4999\n")?;
            let loader = bootstrap_config_loader("APP_")
                .map_err(|error| figment::Error::from(error.to_string()))?;
            assert_eq!(loader.environment_override(), Some(&Environment::Test));
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
        builder.provider(CoreCommandsProvider::<CommandFixture> {
            console_environment: None,
            marker: std::marker::PhantomData,
        });
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            crate::config::FrameworkConfig::default(),
        ));
        let app = Application::from_provider_runtime(builder.start(sources).await?)?;
        let names: Vec<&str> = app.commands().map(|command| command.name()).collect();
        assert_eq!(
            names,
            [
                "help",
                "list",
                "env",
                "config show",
                "config:cache",
                "config:clear",
                "key:generate"
            ]
        );
        app.run_command("env", Vec::new()).await?;
        for argument in ["unexpected", "--unknown"] {
            assert!(matches!(
                app.run_command("env", vec![argument.into()]).await,
                Err(error) if error.code() == "unexpected_argument"
            ));
        }
        assert_eq!(
            app.run_command("serve", Vec::new())
                .await
                .err()
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("unknown command `serve`; run `list` to list commands")
        );
        Ok(())
    }

    #[derive(Default)]
    struct Log(Mutex<Vec<&'static str>>);

    struct Second;
    impl ServiceProvider for Second {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(AtomicBool::new(false))).await?;
                Ok(())
            })
        }
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
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
        ) -> crate::support::BoxFuture<'a, Result<()>> {
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
        let app = Application::from_provider_runtime(builder.start(sources).await?)?;
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
