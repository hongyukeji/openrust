#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;

mod language_lines {
    include!(
        "../../../vendor/openrust/localization-database/stubs/create_language_lines_table.stub"
    );
}

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        localization::Loader,
        queue::JobHandler,
    },
    database::{DatabaseManager, Value, migrations::MigrationEntry},
    foundation::{
        Application, configuration::Routing, console::VendorPublishCommand, run_application,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    localization::FileLoader,
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Db, Lang, Queue, Route},
    },
};
use openrust_localization_database::{DatabaseLoader, LocalizationDatabaseServiceProvider};
use serde_json::json;
use std::{process::ExitCode, sync::Arc};

static LOCALE_MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "database/migrations/2026_11_25_000000_create_language_lines_table.rs",
    &language_lines::Definition,
)];

struct FixtureProvider;

impl ServiceProvider for FixtureProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singleton_named::<dyn Loader, _>("lang.loader", |container| {
                    let files = container.make::<FileLoader>()?;
                    let connection = container
                        .make::<DatabaseManager>()?
                        .connection(None)
                        .map_err(|source| BindingResolutionError::FactoryFailed {
                            service: "lang.loader",
                            source: Box::new(source),
                        })?;
                    Ok(Arc::new(
                        DatabaseLoader::new(connection).with_fallback(files),
                    ))
                })?;
            Queue::register("localization.test.probe", Arc::new(ProbeJob))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        VendorPublishCommand::dont_update_migration_dates(app);
        self.load_migrations_from(app, LOCALE_MIGRATIONS)
    }
}

fn messages() -> openrust::localization::TranslationResult<serde_json::Value> {
    Ok(json!({
        "title": Lang::get("messages.title", &Default::default(), Some("fr"))?,
        "only_file": Lang::get("messages.only_file", &Default::default(), Some("fr"))?,
        "fallback": Lang::get("messages.fallback", &Default::default(), Some("fr"))?,
        "empty": Lang::get("messages.empty", &Default::default(), Some("fr"))?,
        "literal": Lang::get("Fixture literal.", &Default::default(), Some("fr"))?,
    }))
}

fn http(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let refresh = request.route("action") == Some("refresh");
    Box::pin(async move {
        if refresh {
            Lang::refresh_default_source().await?;
        }
        Response::json(StatusCode::OK, messages()?)
    })
}

struct ProbeJob;

impl JobHandler for ProbeJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let wrap = |source: std::io::Error| QueueError::Execution {
                source: Arc::new(source),
            };
            let path = payload.data().as_str().ok_or(QueueError::InvalidPayload)?;
            let result = messages().map_err(|error| wrap(std::io::Error::other(error)))?;
            std::fs::write(path, result.to_string()).map_err(wrap)
        })
    }
}

struct Probe(CommandDefinition);

impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let failure = |message| ConsoleError::new(format!("fixture: {message}"));
            let action = input.argument("action");
            let action = action.as_ref().and_then(serde_json::Value::as_str);
            match action {
                Some("seed" | "clean" | "invalid") => {
                    let connection = Db::connection(None).map_err(|e| failure(e.to_string()))?;
                    connection
                        .table("language_lines")
                        .delete()
                        .await
                        .map_err(|e| failure(e.to_string()))?;
                    if action == Some("clean") {
                        return Ok(0);
                    }
                    let value = input.argument("value");
                    let title = value
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .expect("seed title");
                    for (group, key, text) in [
                        ("messages", "title", json!({"fr":title})),
                        ("messages", "empty", json!({"fr":""})),
                        (
                            "*",
                            "Fixture literal.",
                            if action == Some("invalid") {
                                json!({"fr":17})
                            } else {
                                json!({"fr":"Phrase SQL."})
                            },
                        ),
                    ] {
                        connection
                            .table("language_lines")
                            .insert(&[[
                                ("group".into(), Value::from(group)),
                                ("key".into(), Value::from(key)),
                                ("text".into(), Value::Json(text)),
                            ]
                            .into_iter()
                            .collect()])
                            .await
                            .map_err(|e| failure(e.to_string()))?;
                    }
                }
                Some("queue") => {
                    let value = input.argument("value");
                    let target = value
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .expect("queue output");
                    let payload = Payload::new("localization.test.probe", target)
                        .map_err(|e| failure(e.to_string()))?;
                    Queue::dispatch(payload, DispatchOptions::default(), None)
                        .await
                        .map_err(|e| failure(e.to_string()))?;
                }
                _ => output
                    .write_raw(&format!(
                        "{}\n",
                        messages().map_err(|e| failure(e.to_string()))?
                    ))
                    .map_err(ConsoleError::from_source)?,
            }
            Ok(0)
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(
        clap::Command::new("localization:test")
            .arg(clap::Arg::new("action"))
            .arg(clap::Arg::new("value")),
    ))))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("L10N_FIXTURE_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::infer_base_path(None)?;
        Application::configure(root)
            .with_config(|path, env| {
                let mut config = config::load(path, env)?;
                if env.get_utf8("L10N_ENABLED")? == Some("1") {
                    config.insert(
                        "lang".into(),
                        json!({"resources":[
                            {"locale":"fr","group":"messages"},
                            {"locale":"fr","group":"*"},
                        ]}),
                    );
                }
                Ok(config)
            })
            .with_providers(vec![
                Box::new(LocalizationDatabaseServiceProvider),
                Box::new(FixtureProvider),
            ])
            .with_commands(commands)
            .with_migrations(migrations::entries())
            .with_routing(Routing::default().api(|| Route::get("/localization/{action}", http)))
            .create()
            .map_err(Into::into)
    })
    .await
}
