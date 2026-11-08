//! 复用生产 Provider 启动流程的应用测试夹具。

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, Response};
#[cfg(test)]
use figment::Figment;
use tokio::time::Instant;
use tower::ServiceExt;

use crate::config::ConfigSources;
use crate::container::Container;
use crate::core::Error;
use crate::core::Result;
use crate::foundation::Application;
use crate::foundation::ApplicationDefinition;
use crate::foundation::ProviderBuilder;
use crate::foundation::provider::ProviderId;
use crate::support::BoxFuture;

use super::response::TestResponse;

const DEFAULT_MAX_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
type ConfigureProviders = Arc<dyn Fn(&mut ProviderBuilder) + Send + Sync>;
type Configure = Arc<dyn Fn(Container) -> BoxFuture<'static, Result<()>> + Send + Sync>;

/// 显式配置、Provider 选择及可重复执行的测试替换蓝图。
///
/// 不修改进程环境或当前目录；闭包捕获的外部 `Arc` 由调用方管理，重建不会重置该对象。
pub struct TestAppBuilder<A: ApplicationDefinition> {
    sources: ConfigSources,
    removed: Vec<ProviderId>,
    configure_providers: Vec<ConfigureProviders>,
    configure: Vec<Configure>,
    middleware_enabled: bool,
    application: PhantomData<fn() -> A>,
}

impl<A: ApplicationDefinition> TestAppBuilder<A> {
    /// 为此测试蓝图跳过路由和应用全局中间件及 CORS，保留 HTTP 内核的协议保护。
    pub fn without_middleware(&mut self) -> &mut Self {
        self.middleware_enabled = false;
        self
    }

    /// 恢复此测试蓝图的完整中间件链。
    pub fn with_middleware(&mut self) -> &mut Self {
        self.middleware_enabled = true;
        self
    }

    /// 从应用 Provider 清单移除具体类型；不修改框架实现。
    pub fn remove_provider(&mut self, id: ProviderId) -> &mut Self {
        self.removed.push(id);
        self
    }

    /// 通过生产启动器配置外部包；reset 时重复应用同一纯装配闭包。
    pub fn configure_providers<F>(&mut self, configure: F) -> &mut Self
    where
        F: Fn(&mut ProviderBuilder) + Send + Sync + 'static,
    {
        self.configure_providers.push(Arc::new(configure));
        self
    }

    /// 在全体 register 后、基础校验及 boot 前替换绑定；reset 时重新运行此闭包。
    pub fn configure_after_register<F>(&mut self, configure: F) -> &mut Self
    where
        F: Fn(Container) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.configure.push(Arc::new(configure));
        self
    }

    /// 经与生产相同的 `ProviderBuilder` 生命周期构建测试应用。
    pub async fn build(self) -> Result<TestApp<A>> {
        let app = self.start().await?;
        let mut fixture = TestApp {
            app: Some(app),
            router: None,
            active: AtomicBool::new(false),
            blueprint: self,
        };
        fixture.install_router().await?;
        Ok(fixture)
    }

    /// 以实际命令名引导并执行一次命令，随后限时关闭应用。
    ///
    /// 启动上下文与执行使用同一 `name`；不返回带命令上下文的应用。
    /// 普通 [`Self::build`] 与 [`TestApp::reset`] 始终执行完整启动校验。
    /// 命令失败时仍等待关闭；若命令与关闭均失败，返回命令错误并记录关闭错误码。
    pub async fn run_command(self, name: &str, args: Vec<String>) -> Result<()> {
        let started = crate::foundation::EntryStarted::now();
        let app = self.start_with_command(Some(name)).await?;
        // 复用 TestApp 的 Drop：执行 future 被取消时立即封口，并在运行时兜底回收。
        let mut fixture = TestApp {
            app: Some(app),
            router: None,
            active: AtomicBool::new(false),
            blueprint: self,
        };
        let result = fixture
            .app
            .as_ref()
            .ok_or_else(|| Error::custom("test_app_closed", "测试应用尚未启动"))?
            .run_command_since(name, args, started)
            .await;
        let cleanup = fixture.shutdown(Instant::now() + DEFAULT_TIMEOUT).await;
        match (result, cleanup) {
            (Err(error), Err(cleanup_error)) => {
                tracing::warn!(code = cleanup_error.code(), "测试命令失败后的回收失败");
                Err(error)
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    async fn start(&self) -> Result<Application> {
        self.start_with_command(None).await
    }

    async fn start_with_command(&self, name: Option<&str>) -> Result<Application> {
        let config: A::Config = self
            .sources
            .figment()
            .extract()
            .map_err(|_| Error::custom("test_app_config_invalid", "测试应用配置提取失败"))?;
        let mut builder = ProviderBuilder::for_application::<A>(&config);
        builder.testing_boot();
        if let Some(name) = name {
            builder.console_command(name);
        }
        for id in &self.removed {
            builder.remove_provider(*id);
        }
        for configure in &self.configure_providers {
            configure(&mut builder);
        }
        for configure in &self.configure {
            let configure = Arc::clone(configure);
            builder.configure_after_register(move |container| configure(container));
        }
        Application::from_provider_runtime(builder.start(self.sources.clone()).await?)
    }
}

/// 独立应用测试实例，使用公开 Router 与生产请求 scope。
///
/// `request` 是进程内 Tower 请求，不是 TCP 网络验收。应显式调用 `shutdown` 等待回收；
/// Drop 只能在仍有 Tokio 运行时的情况下提交尽力而为的回收。
pub struct TestApp<A: ApplicationDefinition> {
    app: Option<Application>,
    router: Option<axum::Router>,
    active: AtomicBool,
    blueprint: TestAppBuilder<A>,
}

impl<A: ApplicationDefinition> TestApp<A> {
    /// 以显式配置源建立蓝图；不读取或修改环境变量。
    pub fn builder(sources: impl Into<ConfigSources>) -> TestAppBuilder<A> {
        TestAppBuilder {
            sources: sources.into(),
            removed: Vec::new(),
            configure_providers: Vec::new(),
            configure: Vec::new(),
            middleware_enabled: true,
            application: PhantomData,
        }
    }

    /// 获取仍可用的应用；关闭或重建失败后不暴露旧生命周期。
    pub fn app(&self) -> Result<&Application> {
        self.app
            .as_ref()
            .filter(|_| self.active.load(Ordering::Acquire))
            .ok_or_else(|| Error::custom("test_app_closed", "测试应用已关闭或重建未成功"))
    }

    async fn install_router(&mut self) -> Result<()> {
        let app = self
            .app
            .as_ref()
            .ok_or_else(|| Error::custom("test_app_closed", "测试应用尚未启动"))?;
        match crate::foundation::http::build_router_with_middleware(
            app,
            self.blueprint.middleware_enabled,
        ) {
            Ok(router) => {
                self.router = Some(router);
                self.active.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                app.begin_close();
                if let Err(cleanup) = self.shutdown(Instant::now() + DEFAULT_TIMEOUT).await {
                    tracing::warn!(code = cleanup.code(), "测试路由构建失败后的回收失败");
                }
                Err(error)
            }
        }
    }

    async fn dispatch(&self, request: Request<Body>) -> Result<Response<Body>> {
        self.app()?;
        let router = self
            .router
            .as_ref()
            .ok_or_else(|| Error::custom("test_app_closed", "测试应用路由尚未就绪"))?;
        router
            .clone()
            .oneshot(request)
            .await
            .map_err(|_| Error::custom("test_app_request_failed", "测试应用请求执行失败"))
    }

    /// 保留流式响应，不读取响应体；响应头等待上限为五秒。
    ///
    /// 调用方应消费或丢弃响应体，释放由其持有的请求 scope。
    pub async fn raw_response(&self, request: Request<Body>) -> Result<Response<Body>> {
        tokio::time::timeout(DEFAULT_TIMEOUT, self.dispatch(request))
            .await
            .map_err(|_| Error::custom("test_app_request_timeout", "测试应用请求超过期限"))?
    }

    /// 发送请求并读取响应，默认总预算五秒、响应体上限 1 MiB。
    pub async fn request(&self, request: Request<Body>) -> Result<TestResponse> {
        self.request_with_limits(request, DEFAULT_MAX_BYTES, DEFAULT_TIMEOUT)
            .await
    }

    /// 设置从请求开始到读完响应体的总预算与最大字节数。
    pub async fn request_with_limits(
        &self,
        request: Request<Body>,
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<TestResponse> {
        tokio::time::timeout(timeout, async {
            let response = self.dispatch(request).await?;
            TestResponse::from_response(response, max_bytes, timeout).await
        })
        .await
        .map_err(|_| Error::custom("test_app_request_timeout", "测试应用请求超过期限"))?
    }

    /// 封闭本夹具并等待框架拥有的资源；失败后仍禁止继续请求。
    pub async fn shutdown(&mut self, deadline: Instant) -> Result<()> {
        self.active.store(false, Ordering::Release);
        self.router = None;
        if let Some(app) = &self.app {
            app.shutdown(deadline).await?;
        }
        self.app = None;
        Ok(())
    }

    /// 在指定期限内关闭旧应用后按相同蓝图重建；不是只重置根 scoped 缓存。
    ///
    /// 新应用的启动和路由构建不受这个关闭期限约束。旧关闭或新启动失败时保持不可用；
    /// 外部共享 fixture 不会自动重置。
    pub async fn reset(&mut self, deadline: Instant) -> Result<()> {
        self.shutdown(deadline).await?;
        let app = self.blueprint.start().await?;
        self.app = Some(app);
        self.install_router().await
    }
}

impl<A: ApplicationDefinition> Drop for TestApp<A> {
    fn drop(&mut self) {
        self.router = None;
        if let Some(app) = self.app.take() {
            app.begin_close();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    if let Err(error) = app.shutdown(Instant::now() + DEFAULT_TIMEOUT).await {
                        tracing::warn!(code = error.code(), "测试应用兜底回收失败");
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FrameworkConfig, NoConfig};
    use crate::console::Command;
    use crate::foundation::BootContext;
    use crate::foundation::RegisterContext;
    use crate::http::{HttpServiceProvider, Inject};
    use crate::support::{BoxFuture, ServiceProvider};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Fixture {
        created: AtomicUsize,
        closed: AtomicUsize,
        fail: AtomicBool,
        changed: tokio::sync::Notify,
        cleaning: tokio::sync::Notify,
        cleanup_gate: std::sync::Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    }
    struct Lease;
    struct Label(&'static str);
    struct BusinessProvider;
    impl ServiceProvider for BusinessProvider {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(Label("production"))).await?;
                app.instance(Arc::new(Fixture::default())).await?;
                app.scoped::<Lease, _>(|resolver| {
                    Box::pin(async move {
                        let fixture = resolver.resolve::<Fixture>().await?;
                        fixture.created.fetch_add(1, Ordering::SeqCst);
                        resolver.resources()?.on_shutdown(move || {
                            Box::pin(async move {
                                let gate = fixture
                                    .cleanup_gate
                                    .lock()
                                    .map_err(|_| {
                                        Error::custom("fixture_lock_failed", "夹具锁不可用")
                                    })?
                                    .clone();
                                fixture.cleaning.notify_one();
                                if let Some(gate) = gate {
                                    gate.acquire_owned()
                                        .await
                                        .map_err(Error::internal)?
                                        .forget();
                                }
                                fixture.closed.fetch_add(1, Ordering::SeqCst);
                                fixture.changed.notify_one();
                                if fixture.fail.load(Ordering::SeqCst) {
                                    return Err(Error::custom(
                                        "fixture_cleanup_failed",
                                        "夹具关闭失败",
                                    ));
                                }
                                Ok(())
                            })
                        })?;
                        Ok(Arc::new(Lease))
                    })
                })
                .await?;
                Ok(())
            })
        }
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                // 必须在 boot 已读到测试替换，不能只在最终请求时才生效。
                let label = app.resolve::<Label>().await?;
                app.routes(move |routes| {
                    let label = label.0;
                    routes
                        .get("/api/fixture", move |Inject(_): Inject<Lease>| async move {
                            axum::Json(serde_json::json!({"label": label}))
                        })
                        .get("/api/pending", pending)
                        .get("/api/pending-handler", pending_handler)
                })?;
                Ok(())
            })
        }
    }
    async fn pending(Inject(_): Inject<Lease>) -> axum::response::Response {
        Body::from_stream(futures_util::stream::pending::<
            std::result::Result<axum::body::Bytes, std::convert::Infallible>,
        >())
        .into_response()
    }
    async fn pending_handler(Inject(_): Inject<Lease>) -> StatusCode {
        std::future::pending().await
    }
    struct Example;
    impl ApplicationDefinition for Example {
        type Config = NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(HttpServiceProvider), Box::new(BusinessProvider)]
        }
    }
    fn source() -> Figment {
        let mut config = FrameworkConfig::default();
        config.api.http_status = crate::config::HttpStatusMode::Semantic;
        Figment::from(figment::providers::Serialized::defaults(config))
    }
    fn request(path: &str) -> Result<Request<Body>> {
        Request::builder()
            .uri(path)
            .body(Body::empty())
            .map_err(Error::internal)
    }
    fn blueprint(fixture: &Arc<Fixture>, label: &'static str) -> TestAppBuilder<Example> {
        let fixture = Arc::clone(fixture);
        let mut builder = TestApp::<Example>::builder(source());
        builder.configure_after_register(move |bindings| {
            let fixture = Arc::clone(&fixture);
            Box::pin(async move {
                bindings.instance(fixture).await?;
                bindings.instance(Arc::new(Label(label))).await?;
                Ok(())
            })
        });
        builder
    }

    #[tokio::test]
    async fn overrides_are_visible_in_boot_two_apps_and_full_reset() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let mut first = blueprint(&fixture, "first").build().await?;
        let mut second = blueprint(&Arc::new(Fixture::default()), "second")
            .build()
            .await?;
        first
            .request(request("/api/fixture")?)
            .await?
            .assert_json(&serde_json::json!({"label":"first"}))?;
        second
            .request(request("/api/fixture")?)
            .await?
            .assert_json(&serde_json::json!({"label":"second"}))?;
        first.reset(Instant::now() + DEFAULT_TIMEOUT).await?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        first
            .request(request("/api/fixture")?)
            .await?
            .assert_status(StatusCode::OK)?;
        first.shutdown(Instant::now() + DEFAULT_TIMEOUT).await?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.created.load(Ordering::SeqCst), 2);
        assert!(first.app().is_err());
        second.shutdown(Instant::now() + DEFAULT_TIMEOUT).await?;
        Ok(())
    }

    #[tokio::test]
    async fn removing_business_provider_keeps_framework_health() -> Result<()> {
        let mut builder = TestApp::<Example>::builder(source());
        builder.remove_provider(ProviderId::of::<BusinessProvider>());
        let mut app = builder.build().await?;
        let removed = app.request(request("/api/fixture")?).await?;
        assert_eq!(
            removed.status(),
            StatusCode::NOT_FOUND,
            "removed business route"
        );
        let health = app.request(request("/health")?).await?;
        assert_eq!(health.status(), StatusCode::OK, "framework health route");
        app.shutdown(Instant::now() + DEFAULT_TIMEOUT).await
    }

    #[tokio::test]
    async fn stream_drop_and_bounded_timeout_close_request_resources() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let mut app = blueprint(&fixture, "fake").build().await?;
        let response = app.raw_response(request("/api/pending")?).await?;
        assert_eq!(fixture.created.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 0);
        drop(response);
        tokio::time::timeout(DEFAULT_TIMEOUT, async {
            while fixture.closed.load(Ordering::SeqCst) < 1 {
                fixture.changed.notified().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        let failed = app
            .request_with_limits(request("/api/pending")?, 1024, Duration::from_millis(10))
            .await;
        assert!(failed.is_err());
        app.shutdown(Instant::now() + DEFAULT_TIMEOUT).await?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn pending_handler_timeout_closes_its_request_scope() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let mut app = blueprint(&fixture, "fake").build().await?;
        let result = app
            .request_with_limits(
                request("/api/pending-handler")?,
                1024,
                Duration::from_millis(10),
            )
            .await;
        assert_eq!(
            result.err().map(|error| error.code().to_owned()).as_deref(),
            Some("test_app_request_timeout")
        );
        app.shutdown(Instant::now() + DEFAULT_TIMEOUT).await?;
        assert_eq!(fixture.created.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_awaits_cleanup_while_caller_holds_raw_stream() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        *fixture
            .cleanup_gate
            .lock()
            .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))? =
            Some(Arc::clone(&gate));
        let mut app = blueprint(&fixture, "fake").build().await?;
        let response = app.raw_response(request("/api/pending")?).await?;
        let mut closing = Box::pin(app.shutdown(Instant::now() + DEFAULT_TIMEOUT));
        tokio::select! {
            result = &mut closing => {
                result?;
                return Err(Error::custom("fixture_closed_early", "异步回收尚未放行，应用却已关闭"));
            }
            () = fixture.cleaning.notified() => {}
        }
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 0);
        gate.add_permits(1);
        closing.await?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        drop(response);
        assert!(app.app().is_err());
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn dropping_without_runtime_seals_escaped_application_state() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::internal)?;
        let app = runtime.block_on(TestApp::<Example>::builder(source()).build())?;
        let state = app.app()?.state().clone();
        // 这里没有 entered Tokio runtime；不能依赖 spawn 才封闭容器。
        drop(app);
        let result = runtime.block_on(state.resolve::<Label>());
        assert_eq!(
            result.err().map(|error| error.code().to_owned()).as_deref(),
            Some("container_closed")
        );
        Ok(())
    }

    #[tokio::test]
    async fn dropping_with_runtime_seals_immediately_then_awaits_fallback_cleanup() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let app = blueprint(&fixture, "fake").build().await?;
        let state = app.app()?.state().clone();
        let response = app.raw_response(request("/api/pending")?).await?;
        drop(app);
        assert_eq!(
            state
                .resolve::<Label>()
                .await
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("container_closed")
        );
        tokio::time::timeout(DEFAULT_TIMEOUT, async {
            while fixture.closed.load(Ordering::SeqCst) < 1 {
                fixture.changed.notified().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        drop(response);
        Ok(())
    }

    #[tokio::test]
    async fn failed_rebuild_keeps_test_application_closed() -> Result<()> {
        let fail = Arc::new(AtomicBool::new(false));
        let mut builder = TestApp::<Example>::builder(source());
        let fail_start = Arc::clone(&fail);
        builder.configure_after_register(move |_| {
            let fail_start = Arc::clone(&fail_start);
            Box::pin(async move {
                if fail_start.load(Ordering::SeqCst) {
                    return Err(Error::custom("fixture_start_failed", "夹具启动失败"));
                }
                Ok(())
            })
        });
        let mut app = builder.build().await?;
        fail.store(true, Ordering::SeqCst);
        assert!(app.reset(Instant::now() + DEFAULT_TIMEOUT).await.is_err());
        assert!(app.app().is_err());
        assert!(app.request(request("/health")?).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn failed_cleanup_prevents_reset_from_publishing_new_app() -> Result<()> {
        let fixture = Arc::new(Fixture::default());
        let mut app = blueprint(&fixture, "fake").build().await?;
        let raw = app.raw_response(request("/api/pending")?).await?;
        fixture.fail.store(true, Ordering::SeqCst);
        let reset = app.reset(Instant::now() + DEFAULT_TIMEOUT).await;
        assert!(reset.is_err());
        assert!(app.app().is_err());
        assert_eq!(fixture.created.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        drop(raw);
        Ok(())
    }

    #[derive(Default)]
    struct CommandFixture {
        ready: AtomicBool,
        fail_command: AtomicBool,
        fail_cleanup: AtomicBool,
        runs: AtomicUsize,
        closed: AtomicUsize,
        command_started: tokio::sync::Notify,
        cleanup_started: tokio::sync::Notify,
        cleanup_finished: tokio::sync::Notify,
        command_gate: std::sync::Mutex<Option<Arc<tokio::sync::Semaphore>>>,
        cleanup_gate: std::sync::Mutex<Option<Arc<tokio::sync::Semaphore>>>,
        command_state: std::sync::Mutex<Option<crate::foundation::ApplicationState>>,
    }

    struct MigrateProbe;
    impl Command for MigrateProbe {
        fn name(&self) -> &'static str {
            "migrate"
        }

        fn about(&self) -> &'static str {
            "测试迁移命令"
        }

        fn run<'a>(&'a self, app: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let fixture = app.state().resolve::<CommandFixture>().await?;
                fixture.runs.fetch_add(1, Ordering::SeqCst);
                let gate = fixture
                    .command_gate
                    .lock()
                    .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))?
                    .clone();
                if gate.is_some() {
                    *fixture
                        .command_state
                        .lock()
                        .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))? =
                        Some(app.state().clone());
                }
                fixture.command_started.notify_one();
                if let Some(gate) = gate {
                    gate.acquire_owned()
                        .await
                        .map_err(Error::internal)?
                        .forget();
                }
                if fixture.fail_command.load(Ordering::SeqCst) {
                    return Err(Error::custom("fixture_command_failed", "测试命令失败"));
                }
                fixture.ready.store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    struct CommandProvider;
    impl ServiceProvider for CommandProvider {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                let fixture = app.resolve::<CommandFixture>().await?;
                let closing = Arc::clone(&fixture);
                app.resources().on_shutdown(move || {
                    Box::pin(async move {
                        let gate = closing
                            .cleanup_gate
                            .lock()
                            .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))?
                            .clone();
                        closing.cleanup_started.notify_one();
                        if let Some(gate) = gate {
                            gate.acquire_owned()
                                .await
                                .map_err(Error::internal)?
                                .forget();
                        }
                        closing.closed.fetch_add(1, Ordering::SeqCst);
                        closing.cleanup_finished.notify_one();
                        if closing.fail_cleanup.load(Ordering::SeqCst) {
                            return Err(Error::custom("fixture_cleanup_failed", "测试关闭失败"));
                        }
                        Ok(())
                    })
                })?;
                if !app.is_console_command("migrate") && !fixture.ready.load(Ordering::SeqCst) {
                    return Err(Error::custom("fixture_schema_missing", "测试表尚未迁移"));
                }
                app.command(MigrateProbe);
                Ok(())
            })
        }
    }

    struct CommandExample;
    impl ApplicationDefinition for CommandExample {
        type Config = NoConfig;

        fn default_providers() -> crate::support::DefaultProviders {
            crate::support::DefaultProviders::empty()
        }

        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![Box::new(HttpServiceProvider), Box::new(CommandProvider)]
        }
    }

    fn command_builder(fixture: &Arc<CommandFixture>) -> TestAppBuilder<CommandExample> {
        let shared = Arc::clone(fixture);
        let mut builder = TestApp::<CommandExample>::builder(source());
        builder.configure_after_register(move |bindings| {
            let shared = Arc::clone(&shared);
            Box::pin(async move {
                bindings.instance(shared).await?;
                Ok(())
            })
        });
        builder
    }

    #[tokio::test]
    async fn command_builder_uses_same_name_and_normal_build_checks_schema() -> Result<()> {
        let fixture = Arc::new(CommandFixture::default());
        let error = command_builder(&fixture)
            .build()
            .await
            .err()
            .ok_or_else(|| Error::custom("fixture_expected_failure", "普通启动未拒绝空表"))?;
        assert_eq!(error.code(), "fixture_schema_missing");
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 1);
        let error = command_builder(&fixture)
            .run_command("other", Vec::new())
            .await
            .err()
            .ok_or_else(|| Error::custom("fixture_expected_failure", "其他命令未检查空表"))?;
        assert_eq!(error.code(), "fixture_schema_missing");
        assert_eq!(fixture.runs.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 2);
        command_builder(&fixture)
            .run_command("migrate", Vec::new())
            .await?;
        assert_eq!(fixture.runs.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 3);
        let mut normal = command_builder(&fixture).build().await?;
        normal.shutdown(Instant::now() + DEFAULT_TIMEOUT).await?;
        assert_eq!(fixture.closed.load(Ordering::SeqCst), 4);
        Ok(())
    }

    #[tokio::test]
    async fn command_builder_closes_after_command_and_cleanup_failures() -> Result<()> {
        let failing_command = Arc::new(CommandFixture::default());
        failing_command.fail_command.store(true, Ordering::SeqCst);
        let error = command_builder(&failing_command)
            .run_command("migrate", Vec::new())
            .await
            .err()
            .ok_or_else(|| Error::custom("fixture_expected_failure", "命令未失败"))?;
        assert_eq!(error.code(), "fixture_command_failed");
        assert_eq!(failing_command.closed.load(Ordering::SeqCst), 1);
        assert!(!failing_command.ready.load(Ordering::SeqCst));

        let failing_cleanup = Arc::new(CommandFixture::default());
        failing_cleanup.fail_cleanup.store(true, Ordering::SeqCst);
        let error = command_builder(&failing_cleanup)
            .run_command("migrate", Vec::new())
            .await
            .err()
            .ok_or_else(|| Error::custom("fixture_expected_failure", "关闭未失败"))?;
        assert_eq!(error.code(), "resource_shutdown_failed");
        assert_eq!(failing_cleanup.runs.load(Ordering::SeqCst), 1);
        assert_eq!(failing_cleanup.closed.load(Ordering::SeqCst), 1);

        let failing_both = Arc::new(CommandFixture::default());
        failing_both.fail_command.store(true, Ordering::SeqCst);
        failing_both.fail_cleanup.store(true, Ordering::SeqCst);
        let error = command_builder(&failing_both)
            .run_command("migrate", Vec::new())
            .await
            .err()
            .ok_or_else(|| Error::custom("fixture_expected_failure", "命令和关闭未失败"))?;
        assert_eq!(error.code(), "fixture_command_failed");
        assert_eq!(failing_both.runs.load(Ordering::SeqCst), 1);
        assert_eq!(failing_both.closed.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_command_and_cleanup_close_once() -> Result<()> {
        let running = Arc::new(CommandFixture::default());
        let command_gate = Arc::new(tokio::sync::Semaphore::new(0));
        *running
            .command_gate
            .lock()
            .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))? =
            Some(Arc::clone(&command_gate));
        let mut command = Box::pin(command_builder(&running).run_command("migrate", Vec::new()));
        tokio::time::timeout(DEFAULT_TIMEOUT, async {
            tokio::select! {
                result = &mut command => result,
                () = running.command_started.notified() => Ok(()),
            }
        })
        .await
        .map_err(Error::internal)??;
        drop(command);
        let state = running
            .command_state
            .lock()
            .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))?
            .take()
            .ok_or_else(|| Error::custom("fixture_state_missing", "命令未保存应用状态"))?;
        assert_eq!(
            state
                .resolve::<CommandFixture>()
                .await
                .err()
                .as_ref()
                .map(Error::code),
            Some("container_closed")
        );
        tokio::time::timeout(DEFAULT_TIMEOUT, running.cleanup_finished.notified())
            .await
            .map_err(Error::internal)?;
        assert_eq!(running.closed.load(Ordering::SeqCst), 1);
        assert_eq!(running.runs.load(Ordering::SeqCst), 1);
        assert!(!running.ready.load(Ordering::SeqCst));

        let closing = Arc::new(CommandFixture::default());
        let cleanup_gate = Arc::new(tokio::sync::Semaphore::new(0));
        *closing
            .cleanup_gate
            .lock()
            .map_err(|_| Error::custom("fixture_lock_failed", "夹具锁不可用"))? =
            Some(Arc::clone(&cleanup_gate));
        let mut command = Box::pin(command_builder(&closing).run_command("migrate", Vec::new()));
        tokio::time::timeout(DEFAULT_TIMEOUT, async {
            tokio::select! {
                result = &mut command => result,
                () = closing.cleanup_started.notified() => Ok(()),
            }
        })
        .await
        .map_err(Error::internal)??;
        drop(command);
        cleanup_gate.add_permits(1);
        tokio::time::timeout(DEFAULT_TIMEOUT, closing.cleanup_finished.notified())
            .await
            .map_err(Error::internal)?;
        assert_eq!(closing.runs.load(Ordering::SeqCst), 1);
        assert_eq!(closing.closed.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
