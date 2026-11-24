use std::{error::Error, ffi::OsString, path::Path, sync::Arc, time::Duration};

use tempfile::TempDir;
use tokio::task::JoinHandle;

use crate::{
    console::Output,
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    database::{
        Connection, DatabaseResult, test_events, test_mysql::MySqlTestDatabase,
        test_query_counts::TestQueryCounts,
    },
    events::{Dispatcher, EventFake},
    foundation::{Application, BootstrapError, configuration::ApplicationBuilder, http::Kernel},
    http::{Body, Request},
    support::{
        TestClock,
        facades::{Facade, application_scope},
    },
};

use super::{
    FacadeSpy, TestCommand, TestResponse, database::RefreshState,
    database_migrations::MigrationRun, database_truncation::TruncationState,
};

/// 测试工具捕获正文的资源上限，不影响实际监听器的传输配置。
#[derive(Clone, Copy, Debug)]
pub struct ResponseCaptureOptions {
    pub max_bytes: usize,
    pub timeout: Duration,
}

impl Default for ResponseCaptureOptions {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
}

/// An application with its own configuration, container, and temporary root.
pub struct TestApp {
    app: Application,
    original_exception_handler: Option<Arc<dyn crate::contracts::debug::ExceptionHandler>>,
    pub(super) clock: Arc<TestClock>,
    http: Option<Kernel>,
    response_capture: ResponseCaptureOptions,
    pub(super) mysql: Vec<MySqlTestDatabase>,
    pub(super) refreshed: Option<RefreshState>,
    pub(super) migration_run: Option<MigrationRun>,
    pub(super) truncation: Option<TruncationState>,
    pub(super) query_counts: Arc<TestQueryCounts>,
    pub(super) parallel_databases: std::collections::BTreeSet<String>,
    pub(super) parallel_database: bool,
    #[cfg(feature = "sanctum")]
    pub(crate) sanctum_identity: Option<crate::sanctum::TestIdentity>,
    // Release the application's services before deleting their local files.
    _directory: TempDir,
}

impl TestApp {
    /// Creates a test application with the normal application builder.
    ///
    /// The default configuration sets `app.env` to `testing`. The callback can
    /// replace the configuration loader or register the providers and routes
    /// required by the test.
    pub fn create(
        configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let directory = tempfile::Builder::new()
            .prefix("openrust-test-")
            .tempdir()?;
        let builder = Application::configure(directory.path().canonicalize()?)
            .with_config(|_, _| Ok([("app".into(), serde_json::json!({"env": "testing"}))].into()));
        let app = configure(builder).create()?;
        app.use_testing_environment();
        let query_counts = Arc::new(TestQueryCounts::default());
        app.container().instance(query_counts.clone())?;
        let clock = Arc::new(TestClock::default());
        app.container().instance(clock.clone())?;
        Ok(Self {
            app,
            original_exception_handler: None,
            clock,
            http: None,
            response_capture: ResponseCaptureOptions::default(),
            mysql: Vec::new(),
            refreshed: None,
            migration_run: None,
            truncation: None,
            query_counts,
            parallel_databases: Default::default(),
            parallel_database: false,
            #[cfg(feature = "sanctum")]
            sanctum_identity: None,
            _directory: directory,
        })
    }

    /// 为此测试应用配置正文捕获上限；后续请求立即使用新配置。
    pub fn with_response_capture(&mut self, options: ResponseCaptureOptions) -> &mut Self {
        self.response_capture = options;
        self
    }

    /// Runs the normal application bootstrap without opening a listener.
    pub async fn boot(&mut self) -> Result<(), BootstrapError> {
        self.app.boot().await
    }

    /// Dispatches a request through the application's HTTP kernel.
    pub async fn call(
        &mut self,
        request: ::http::Request<Body>,
    ) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
        let kernel = self
            .http
            .get_or_insert_with(|| Kernel::new(self.app.share()));
        kernel.bootstrap().await?;
        let (mut parts, body) = request.into_parts();
        // 测试入口同样通过应用 URL 生成器补全相对地址，保留子路径和根配置。
        if parts.uri.scheme().is_none() {
            use crate::contracts::routing::UrlGenerator as _;
            let generator = self
                .app
                .container()
                .get::<crate::routing::UrlGenerator>("url")?;
            let base = self.app.config()?.get("app.url");
            let base: ::http::Uri = base
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .unwrap_or("http://localhost")
                .parse()?;
            let generator = generator
                .with_uri(Some(base.clone()))
                .with_base_url(base.path().trim_end_matches('/').to_owned());
            let path = parts.uri.to_string();
            let target = generator.to(path.strip_prefix('/').unwrap_or(&path), &[], None)?;
            parts.uri = target.trim_matches('/').parse()?;
        }
        let mut request = Request::from_parts(parts, body);
        #[cfg(feature = "sanctum")]
        if let Some(identity) = &self.sanctum_identity {
            request.parts_mut().extensions.insert(identity.clone());
        }
        let (parts, body) =
            super::exception_handler_fake::catch_reported(kernel.handle(&mut request))
                .await?
                .into_parts();
        let response_time: std::time::SystemTime =
            self.clock.now().unwrap_or_else(jiff::Timestamp::now).into();
        let response_kind = parts.extensions.get::<crate::http::ResponseKind>().copied();
        let original_view = parts
            .extensions
            .get::<Arc<crate::view::RenderedView>>()
            .cloned();
        let urls = self
            .app
            .container()
            .get::<crate::routing::UrlGenerator>("url")?
            .for_request(&request);
        let body = tokio::time::timeout(
            self.response_capture.timeout,
            axum::body::to_bytes(body, self.response_capture.max_bytes),
        )
        .await;
        use crate::contracts::session::Session as _;
        let session = request
            .session()
            .ok()
            .map(|session| serde_json::to_value(session.all()).expect("Session 可序列化"))
            .unwrap_or_default();
        kernel.terminate(request.parts(), &parts).await;
        let encrypter = self
            .app
            .container()
            .make::<dyn crate::contracts::encryption::Encrypter>()
            .ok();
        Ok(TestResponse::new(parts.status, parts.headers, body??)
            .with_original(
                response_kind,
                original_view,
                urls,
                self.clock.clone(),
                response_time,
            )
            .with_session(session)
            .with_encrypter(encrypter))
    }

    /// Dispatches a GET request through the application's HTTP kernel.
    pub async fn get(&mut self, uri: &str) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
        self.call(::http::Request::get(uri).body(Body::empty())?)
            .await
    }

    /// Runs Artisan through its console kernel and captures the command streams.
    pub async fn artisan(
        &mut self,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> TestCommand {
        match self.try_artisan(arguments).await {
            Ok(command) => command,
            Err(error) => std::panic::panic_any(error),
        }
    }

    /// 以 Result 捕获显式 throw_on_report，避免测试调用方依赖 panic。
    pub async fn try_artisan(
        &mut self,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<TestCommand, crate::contracts::debug::SharedException> {
        let arguments: Vec<_> = arguments.into_iter().map(Into::into).collect();
        super::exception_handler_fake::catch_reported(run_artisan(self.app.share(), arguments))
            .await
    }

    pub(super) fn spawn_migration_command(
        &self,
        connection: Connection,
        muted: Vec<Connection>,
        arguments: Vec<OsString>,
    ) -> JoinHandle<DatabaseResult<TestCommand>> {
        let app = self.app.share();
        tokio::spawn(test_events::without_connections(muted, async move {
            // Open the owned pool before Artisan can consider missing-database creation.
            connection.select("SELECT 1", &[]).await?;
            Ok(run_artisan(app, arguments).await)
        }))
    }

    /// Boots the application and replaces a container service for subsequent resolutions.
    pub async fn instance<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        instance: Arc<T>,
    ) -> Result<Arc<T>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self.app.container().instance(instance)?)
    }

    /// Installs a recording replacement for the facade in this test application.
    pub async fn spy<F: Facade, A: Send + Sync + 'static>(
        &mut self,
        replacement: impl FnOnce(FacadeSpy<A>) -> Arc<F::Service>,
    ) -> Result<FacadeSpy<A>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(application_scope(self.app.container_handle(), async { F::spy(replacement) }).await?)
    }

    /// Boots the application and records the selected events. An empty slice selects all.
    pub async fn fake_events(
        &mut self,
        events: &[&str],
    ) -> Result<Arc<EventFake>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self.app.make::<Dispatcher>().await?.fake(events))
    }

    /// 注册当前应用的异常替身；空选择列表记录所有可报告异常。
    pub async fn fake_exceptions(
        &mut self,
        exceptions: &[super::ExceptionType],
    ) -> Result<Arc<super::ExceptionHandlerFake>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(application_scope(self.app.container_handle(), async {
            crate::support::facades::Exceptions::fake_with(exceptions.to_vec())
        })
        .await?)
    }

    /// Returns the application created for this test.
    pub fn app(&self) -> &Application {
        &self.app
    }

    /// 关闭HTTP异常处理，例外类型仍委托原处理器渲染；不丢弃已安装的Fake记录。
    pub async fn without_exception_handling(
        &mut self,
        except: &[super::ExceptionType],
    ) -> Result<&mut Self, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        if self.original_exception_handler.is_none() {
            self.original_exception_handler = Some(
                application_scope(self.app.container_handle(), async {
                    crate::support::facades::Exceptions::handler()
                })
                .await?,
            );
        }
        let handler: Arc<dyn crate::contracts::debug::ExceptionHandler> = Arc::new(
            super::without_exception_handling::WithoutExceptionHandling::new(
                self.original_exception_handler.as_ref().unwrap().clone(),
                except.to_vec(),
            ),
        );
        self.set_exception_test_handler(handler).await?;
        Ok(self)
    }
    async fn set_exception_test_handler(
        &self,
        handler: Arc<dyn crate::contracts::debug::ExceptionHandler>,
    ) -> Result<(), crate::contracts::container::BindingResolutionError> {
        application_scope(self.app.container_handle(), async {
            use crate::support::facades::Exceptions;
            let current = Exceptions::get_facade_root()?;
            let erased: Arc<dyn std::any::Any + Send + Sync> = current;
            match erased.downcast::<super::ExceptionHandlerFake>() {
                Ok(fake) => {
                    fake.set_handler(handler);
                    Ok(())
                }
                Err(_) => Exceptions::swap(handler),
            }
        })
        .await
    }
    pub async fn with_exception_handling(
        &mut self,
    ) -> Result<&mut Self, Box<dyn Error + Send + Sync>> {
        if let Some(handler) = &self.original_exception_handler {
            self.set_exception_test_handler(handler.clone()).await?;
        }
        Ok(self)
    }
    pub async fn handle_exceptions(
        &mut self,
        except: &[super::ExceptionType],
    ) -> Result<&mut Self, Box<dyn Error + Send + Sync>> {
        self.without_exception_handling(except).await
    }
    pub async fn handle_validation_exceptions(
        &mut self,
    ) -> Result<&mut Self, Box<dyn Error + Send + Sync>> {
        self.handle_exceptions(&[super::ExceptionType::including_sources::<
            crate::validation::ValidationException,
        >()])
        .await
    }

    /// Returns the fixture's application root.
    pub fn root(&self) -> &Path {
        self.app.base_path()
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Err(error) = self.query_counts.finish()
        {
            panic!("{error}");
        }
    }
}

pub(super) async fn run_artisan(app: Application, arguments: Vec<OsString>) -> TestCommand {
    let mut output = Output::default();
    let status = app.run_console(&arguments, &mut output).await;
    TestCommand {
        status,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}
