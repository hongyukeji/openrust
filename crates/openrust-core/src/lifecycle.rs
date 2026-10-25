//! Bootstrap sequence and shutdown signal.
//!
//! [`bootstrap`] performs, in order: load `.env`, load configuration, initialise `tracing`,
//! extract the application's own configuration, let the [`Application`] build plugins,
//! run every plugin's `build`, freeze the builder, then run every plugin's `finish`.

use tracing_subscriber::EnvFilter;

use crate::app::{App, AppBuilder, Application};
use crate::command::{ConfigShowCommand, HelpCommand};
use crate::config::{self, ConfigLoader, LogConfig, LogFormat};
use crate::error::Result;

/// Boots `A` from the working directory's configuration.
pub async fn bootstrap<A: Application>() -> Result<App> {
    bootstrap_with::<A>(|_| {}).await
}

/// Like [`bootstrap`], but lets the caller add default plugins before [`Application::build`].
///
/// The facade crate uses this to install the HTTP plugin so every application serves by default.
pub async fn bootstrap_with<A: Application>(defaults: impl FnOnce(&mut AppBuilder)) -> Result<App> {
    config::load_dotenv();
    let loader = ConfigLoader::from_env()?;
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

/// Runs plugin building and finishing on an assembled builder.
///
/// Exposed so tests can build applications without touching the environment.
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

/// Installs the global `tracing` subscriber. A second call (tests, embedded use) is a no-op.
pub fn init_tracing(log: &LogConfig) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&log.level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let installed = match log.format {
        LogFormat::Json => builder.json().try_init(),
        LogFormat::Text => builder.try_init(),
    };
    // Err only means a subscriber is already installed, which is fine.
    let _ = installed;
}

/// Resolves on `SIGINT` (Ctrl-C) or, on Unix, `SIGTERM`.
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
mod tests {
    use super::*;
    use crate::plugin::{BoxFuture, Plugin};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

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
}
