//! Bootstrap sequence and shutdown signal.
//!
//! [`bootstrap`] performs, in order: load `.env` outside production, load configuration, initialise `tracing`,
//! extract the application's own configuration, let the [`Application`] build plugins,
//! run every plugin's `build`, freeze the builder, then run every plugin's `finish`.

use tracing_subscriber::EnvFilter;

use crate::config::{self, ConfigLoader, Environment, LogConfig, LogFormat};
use crate::core::app::{App, AppBuilder, Application};
use crate::core::command::{ConfigShowCommand, HelpCommand};
use crate::core::error::Result;

/// Boots `A` from the working directory's configuration.
pub async fn bootstrap<A: Application>() -> Result<App> {
    bootstrap_with::<A>(|_| {}).await
}

/// Like [`bootstrap`], but lets the caller add default plugins before [`Application::build`].
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
    // Select the environment before loading any other .env key.
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
// `figment::Jail` closures return `figment::Result`, whose error type is large by design.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use crate::config::FrameworkConfig;
    use crate::core::plugin::{BoxFuture, Plugin};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    // Jail restores the working directory and process environment after each closure.
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
            // Track both keys before dotenvy sets them, so Jail restores their original values.
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
}
