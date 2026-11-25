#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;
#[path = "../../../bootstrap/packages.rs"]
mod packages;

use std::process::ExitCode;

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Route,
};

struct Probe(CommandDefinition);

impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let failed = input.matches().get_flag("fail");
            if self.0.name() == "telemetry:enqueue" {
                let mut arguments = vec!["private-job-input".into()];
                if failed {
                    arguments.push("--fail".into());
                }
                openrust::support::facades::Artisan::queue("telemetry:probe", &arguments)
                    .on_connection("database")
                    .on_queue("telemetry")
                    .dispatch()
                    .await
                    .map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            if failed
                && input
                    .matches()
                    .get_one::<String>("input")
                    .is_some_and(|value| value == "private-job-input")
            {
                return Err(ConsoleError::new("private-job-error"));
            }
            Ok(i32::from(failed))
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for name in ["telemetry:probe", "telemetry:enqueue"] {
        let command = clap::Command::new(name).arg(clap::Arg::new("input")).arg(
            clap::Arg::new("fail")
                .long("fail")
                .action(clap::ArgAction::SetTrue),
        );
        artisan.add_command(Box::new(Probe(CommandDefinition::new(command))))?;
    }
    Ok(())
}

fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let status = match request.route("id") {
            Some("fail") => StatusCode::INTERNAL_SERVER_ERROR,
            Some("dependency") => {
                for variable in [
                    "NEW_RELIC_TEST_ALLOWED_UPSTREAM",
                    "NEW_RELIC_TEST_OTHER_UPSTREAM",
                ] {
                    let url = std::env::var(variable).expect("local test endpoint");
                    let connected =
                        openrust::support::facades::Http::get(&url)
                            .await
                            .map_err(|source| HttpError::Internal {
                                source: Box::new(source),
                            })?;
                    assert_eq!(connected.status(), StatusCode::NO_CONTENT);
                }
                let db = openrust::support::facades::Db::connection(None)?;
                db.select("SELECT 'private-sql-literal'", &[]).await?;
                assert!(
                    db.select("SELECT * FROM private_missing_table", &[])
                        .await
                        .is_err()
                );
                let mut rows = db.cursor("SELECT 1", &[]);
                while let Some(row) = std::future::poll_fn(|cx| rows.as_mut().poll_next(cx)).await {
                    row?;
                }
                drop(rows);
                let mut transaction = db.begin_transaction().await?;
                transaction.select("SELECT 1", &[]).await?;
                transaction.commit().await?;
                StatusCode::ACCEPTED
            }
            _ => StatusCode::NO_CONTENT,
        };
        Ok(Response::empty(status))
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("NEW_RELIC_TEST_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::infer_base_path(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root)
            .with_config(config::load)
            .with_packages(packages::manifest())
            .with_commands(commands)
            .with_migrations(migrations::entries())
            .with_routing(Routing::default().api(|| Route::get("/probe/{id}", probe)))
            .create()
            .map_err(Into::into)
    })
    .await
}
