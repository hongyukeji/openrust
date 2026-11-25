#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;
#[path = "../../../bootstrap/packages.rs"]
mod packages;

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _, queue::JobHandler},
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Lang, Queue, Route},
    },
};
use openrust_translate::{
    Translate, TranslateManager, Translation, TranslationError, TranslationFuture,
    TranslationRequest, Translator,
};
use serde_json::{Value, json};
use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

static REQUESTS: AtomicUsize = AtomicUsize::new(0);

struct Example(String);

impl Translator for Example {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            REQUESTS.fetch_add(1, Ordering::Relaxed);
            Translation::new(format!(
                "{}:{}:{}",
                self.0,
                request.target(),
                request.text()
            ))
            .with_detected_source(request.source().unwrap_or("zh-Hans"))
        })
    }
}

struct ConsumerProvider;

impl ServiceProvider for ConsumerProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let manager = app.container().make::<TranslateManager>()?;
            let named = app.container().get::<TranslateManager>("translator")?;
            assert!(Arc::ptr_eq(&manager, &named));
            assert!(Arc::ptr_eq(&manager, &Translate::manager()?));
            manager.extend("example", |profile| {
                let label = profile
                    .get("label")
                    .and_then(Value::as_str)
                    .ok_or(TranslationError::InvalidConfiguration("label"))?;
                Ok(Arc::new(Example(label.to_owned())))
            });
            Queue::register("translate.test.probe", Arc::new(Job))?;
            Ok(())
        })
    }
}

async fn sample() -> openrust_translate::TranslationResult<Value> {
    let default = Translate::translate(" 你好 ", "EN-us").await?;
    let driver = Translate::driver("office")?;
    let request = TranslationRequest::new("你好", "fr")?.with_source("zh-Hans")?;
    let named = driver.translate(request).await?;
    Ok(json!({
        "default": default.text(), "named": named.text(),
        "source": named.detected_source(),
    }))
}

fn route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let localization_only = request.route("action") == Some("lang");
    Box::pin(async move {
        let response = if localization_only {
            json!({
                "text": Lang::get("translate.missing", &Default::default(), None)?,
                "requests": REQUESTS.load(Ordering::Relaxed),
            })
        } else {
            sample().await.map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?
        };
        Response::json(StatusCode::OK, &response)
    })
}

struct Job;

impl JobHandler for Job {
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
            let result = sample()
                .await
                .map_err(|error| wrap(std::io::Error::other(error)))?;
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
            if let Some(path) = input.argument("queue-output") {
                let payload = Payload::new("translate.test.probe", path)
                    .map_err(ConsoleError::from_source)?;
                Queue::dispatch(payload, DispatchOptions::default(), None)
                    .await
                    .map_err(ConsoleError::from_source)?;
            } else {
                let value = sample().await.map_err(ConsoleError::from_source)?;
                output
                    .write_raw(&format!("{value}\n"))
                    .map_err(ConsoleError::from_source)?;
            }
            Ok(0)
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(
        clap::Command::new("translate:test").arg(clap::Arg::new("queue-output")),
    ))))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("TRANSLATE_TEST_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::infer_base_path(None)?;
        Application::configure(root)
            .with_config(|path, env| {
                let mut config = config::load(path, env)?;
                if let Some(label) = env.get_utf8("TRANSLATE_TEST_LABEL")? {
                    config.insert(
                        "translate".into(),
                        json!({
                            "drivers": {
                                "personal": {"driver":"example","label":label},
                                "office": {"driver":"example","label":"office"},
                            },
                        }),
                    );
                }
                Ok(config)
            })
            .with_packages(packages::manifest())
            .with_providers(vec![Box::new(ConsumerProvider)])
            .with_commands(commands)
            .with_migrations(migrations::entries())
            .with_routing(
                Routing::default()
                    .api(|| Route::get("/translate/{action}", route))
                    .health("/up"),
            )
            .create()
            .map_err(Into::into)
    })
    .await
}
