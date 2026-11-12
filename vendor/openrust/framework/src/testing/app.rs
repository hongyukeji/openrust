use std::{error::Error, path::Path};

use tempfile::TempDir;

use crate::{
    contracts::foundation::Application as _,
    foundation::{Application, BootstrapError, configuration::ApplicationBuilder},
};

/// An application with its own configuration, container, and temporary root.
pub struct TestApp {
    app: Application,
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
            _directory: directory,
        })
    }

    /// Runs the normal application bootstrap without opening a listener.
    pub async fn boot(&mut self) -> Result<(), BootstrapError> {
        self.app.boot().await
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
