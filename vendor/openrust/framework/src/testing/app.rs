use std::{error::Error, ffi::OsString, path::Path, sync::Arc, time::Duration};

use tempfile::TempDir;

use crate::{
    console::Output,
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    events::{Dispatcher, EventFake},
    foundation::{Application, BootstrapError, configuration::ApplicationBuilder, http::Kernel},
    http::{Body, Request},
};

use super::{TestCommand, TestResponse};

const RESPONSE_LIMIT: usize = 1024 * 1024;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// An application with its own configuration, container, and temporary root.
pub struct TestApp {
    app: Application,
    http: Option<Kernel>,
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
            .withConfig(|_, _| Ok([("app".into(), serde_json::json!({"env": "testing"}))].into()));
        let app = configure(builder).create()?;
        Ok(Self {
            app,
            http: None,
            _directory: directory,
        })
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
        let (parts, body) = request.into_parts();
        let mut request = Request::from_parts(parts, body);
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        let body =
            tokio::time::timeout(RESPONSE_TIMEOUT, axum::body::to_bytes(body, RESPONSE_LIMIT))
                .await;
        kernel.terminate(request.parts(), &parts).await;
        Ok(TestResponse::new(parts.status, parts.headers, body??))
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
        let arguments: Vec<_> = arguments.into_iter().map(Into::into).collect();
        let mut output = Output::default();
        let status = self.app.share().run_console(&arguments, &mut output).await;
        TestCommand {
            status,
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }

    /// Boots the application and replaces a container service for subsequent resolutions.
    pub async fn instance<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        instance: Arc<T>,
    ) -> Result<Arc<T>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self.app.container().instance(instance)?)
    }

    /// Boots the application and records the selected events. An empty slice selects all.
    #[allow(non_snake_case, reason = "Matches the event testing API naming")]
    pub async fn fakeEvents(
        &mut self,
        events: &[&str],
    ) -> Result<Arc<EventFake>, Box<dyn Error + Send + Sync>> {
        self.boot().await?;
        Ok(self.app.make::<Dispatcher>().await?.fake(events))
    }

    /// Returns the application created for this test.
    pub fn app(&self) -> &Application {
        &self.app
    }

    /// Returns the fixture's application root.
    pub fn root(&self) -> &Path {
        self.app.base_path()
    }
}
