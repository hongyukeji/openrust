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
use crate::core::container::Container;
use crate::core::provider::ProviderId;
use crate::core::{App, Application, BoxFuture, Error, ProviderBuilder, Result};

use super::response::TestResponse;

const DEFAULT_MAX_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
type Configure = Arc<dyn Fn(Container) -> BoxFuture<'static, Result<()>> + Send + Sync>;

/// 显式配置、Provider 选择及可重复执行的测试替换蓝图。
///
/// 不修改进程环境或当前目录；闭包捕获的外部 `Arc` 由调用方管理，重建不会重置该对象。
pub struct TestAppBuilder<A: Application> {
    sources: ConfigSources,
    removed: Vec<ProviderId>,
    removed_modules: Vec<String>,
    configure: Vec<Configure>,
    application: PhantomData<fn() -> A>,
}

impl<A: Application> TestAppBuilder<A> {
    /// 从应用 Provider 清单移除具体类型；不修改框架实现。
    pub fn remove_provider(&mut self, id: ProviderId) -> &mut Self {
        self.removed.push(id);
        self
    }

    /// 仅从本次测试应用启动清单移除已编译模块。
    pub fn remove_module(&mut self, name: impl Into<String>) -> &mut Self {
        self.removed_modules.push(name.into());
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

    async fn start(&self) -> Result<App> {
        let config: A::Config = self
            .sources
            .figment()
            .extract()
            .map_err(|_| Error::custom("test_app_config_invalid", "测试应用配置提取失败"))?;
        let mut builder = ProviderBuilder::for_application::<A>(&config);
        for id in &self.removed {
            builder.remove_provider(*id);
        }
        for name in &self.removed_modules {
            builder.remove_module(name.clone());
        }
        for configure in &self.configure {
            let configure = Arc::clone(configure);
            builder.configure_after_register(move |container| configure(container));
        }
        App::from_provider_runtime(builder.start(self.sources.clone()).await?)
    }
}

/// 独立应用测试实例，使用公开 Router 与生产请求 scope。
///
/// `request` 是进程内 Tower 请求，不是 TCP 网络验收。应显式调用 `shutdown` 等待回收；
/// Drop 只能在仍有 Tokio 运行时的情况下提交尽力而为的回收。
pub struct TestApp<A: Application> {
    app: Option<App>,
    router: Option<axum::Router>,
    active: AtomicBool,
    blueprint: TestAppBuilder<A>,
}

impl<A: Application> TestApp<A> {
    /// 以显式配置源建立蓝图；不读取或修改环境变量。
    pub fn builder(sources: impl Into<ConfigSources>) -> TestAppBuilder<A> {
        TestAppBuilder {
            sources: sources.into(),
            removed: Vec::new(),
            removed_modules: Vec::new(),
            configure: Vec::new(),
            application: PhantomData,
        }
    }

    /// 获取仍可用的应用；关闭或重建失败后不暴露旧生命周期。
    pub fn app(&self) -> Result<&App> {
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
        match crate::routes::build_router(app) {
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

impl<A: Application> Drop for TestApp<A> {
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
    use crate::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
    use crate::http::{HttpServiceProvider, Inject};
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
        ) -> crate::core::BoxFuture<'a, Result<()>> {
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
    impl Application for Example {
        type Config = NoConfig;
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
}
