//! 应用命令的环境权威、注册及参数拒绝回归。

use super::*;
use crate::{
    config::{Environment, FrameworkConfig},
    core::{BootContext, ProviderBuilder, ServiceProvider},
};
use serde_json::json;
use tokio::time::{Duration, Instant};

struct Commands;
impl ServiceProvider for Commands {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(EnvironmentCommand);
            Ok(())
        })
    }
}

async fn application(environment: &str) -> Result<App> {
    let mut config = FrameworkConfig::default();
    config.app.env = Environment::from(environment.to_owned());
    let mut builder = ProviderBuilder::new();
    builder.provider(Commands);
    App::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(config),
            ))
            .await?,
    )
}

#[tokio::test]
async fn exact_started_environment_and_official_message_survive_repository_changes() -> Result<()> {
    let app = application("Mixed-Staging").await?;
    let outcome: Result<()> = async {
        assert_eq!(EnvironmentCommand.name(), "env");
        assert_eq!(
            EnvironmentCommand.about(),
            "Display the current framework environment"
        );
        assert!(!EnvironmentCommand.accepts_args());
        assert_eq!(
            message(&app),
            "The application environment is [Mixed-Staging]."
        );
        app.config_repository()
            .set("app.env", json!("later-repository-value"))?;
        assert_eq!(
            message(&app),
            "The application environment is [Mixed-Staging]."
        );
        assert_eq!(
            app.commands()
                .map(|command| command.name())
                .collect::<Vec<_>>(),
            ["env"]
        );
        app.run_command("env", Vec::new()).await
    }
    .await;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    outcome
}

#[tokio::test]
async fn application_dispatch_rejects_extra_env_arguments() -> Result<()> {
    let app = application("production").await?;
    let outcome: Result<()> = async {
        for argument in ["unexpected", "--unknown"] {
            assert!(matches!(
                app.run_command("env", vec![argument.into()]).await,
                Err(error) if error.code() == "unexpected_argument"
            ));
        }
        assert_eq!(
            message(&app),
            "The application environment is [production]."
        );
        Ok(())
    }
    .await;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    outcome
}
