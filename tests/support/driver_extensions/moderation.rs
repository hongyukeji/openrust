#[path = "../../../database/migrations/0001_01_01_000002_create_jobs_table.rs"]
mod jobs;

use openrust::{
    config::{ConfigError, Environment},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _, queue::JobHandler},
    database::migrations::MigrationEntry,
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::{ProviderResult, ServiceProvider, facades::Queue},
};
use openrust_content_moderation::{
    Assessment, Coverage, Decision, ImageInput, Modality, Moderation, ModerationDriver,
    ModerationError, ModerationFuture, ModerationManager, ModerationResult, Operation, StoredMedia,
    TaskId, TaskStatus, TextInput, VideoInput,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

const JOB: &str = "moderation.fixture.query";

pub(super) fn configuration(env: &Environment) -> Result<Value, ConfigError> {
    let mut configuration = json!({"default":"comments","drivers":{
        "comments":{"driver":"fixture-text"},
        "video":{"driver":"fixture-media","region":env.get_utf8("MODERATION_FIXTURE_REGION")?.unwrap_or("original")},
    }});
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_ALIYUN")? {
        configuration["drivers"]["aliyun-comments"] = json!({
            "driver":"aliyun", "base_url":endpoint, "text_service":"comment_detection", "text_languages":["zh-Hans"],
            "access_key_id_env":"FIXTURE_ALIYUN_ID", "access_key_secret_env":"FIXTURE_ALIYUN_SECRET",
            "security_token_env":"FIXTURE_ALIYUN_TOKEN", "timeout_ms":350,
        });
    }
    Ok(configuration)
}

pub(super) struct ExternalProvider;

impl ServiceProvider for ExternalProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let manager = app.container().make::<ModerationManager>()?;
            let named = app.container().get::<ModerationManager>("moderation")?;
            assert!(Arc::ptr_eq(&manager, &named));
            manager.extend("fixture-text", |_| Ok(Arc::new(TextOnly)));
            manager.extend("fixture-media", |profile| {
                let region = profile["region"]
                    .as_str()
                    .ok_or(ModerationError::InvalidConfiguration("region"))?;
                Ok(Arc::new(Media(region.to_owned())))
            });
            Queue::register(JOB, Arc::new(Query))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            static ENTRIES: &[MigrationEntry] = &[MigrationEntry::new(
                "database/migrations/0001_01_01_000002_create_jobs_table.rs",
                &jobs::Definition,
            )];
            self.load_migrations_from(app, ENTRIES).await
        })
    }
}

struct TextOnly;
impl ModerationDriver for TextOnly {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            if input.text() != "本地 UTF-8 样本 🙂" {
                return Err(ModerationError::InvalidInput("fixture text"));
            }
            Ok(Assessment::new(
                Decision::Review,
                BTreeMap::from([(Modality::Text, Coverage::Complete)]),
                vec![],
            ))
        })
    }
}

struct Media(String);
impl ModerationDriver for Media {
    fn supports(&self, operation: Operation) -> bool {
        matches!(
            operation,
            Operation::Image | Operation::SubmitVideo | Operation::QueryVideo
        )
    }

    fn image<'a>(&'a self, input: &'a ImageInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            if input.as_stored().is_none() {
                return Err(ModerationError::InvalidInput("fixture object"));
            }
            Ok(Assessment::new(
                Decision::Allow,
                BTreeMap::from([(Modality::Visual, Coverage::Complete)]),
                vec![],
            ))
        })
    }

    fn submit_video<'a>(&'a self, input: &'a VideoInput) -> ModerationFuture<'a, TaskId> {
        Box::pin(async move { TaskId::new(format!("{}:{}", self.0, input.source().version())) })
    }

    fn query_video<'a>(&'a self, id: &'a TaskId) -> ModerationFuture<'a, TaskStatus> {
        Box::pin(async move {
            if id.as_str() != format!("{}:revision-2", self.0) {
                return Err(ModerationError::Protocol);
            }
            Ok(TaskStatus::Succeeded(Assessment::new(
                Decision::Allow,
                BTreeMap::from([
                    (Modality::Visual, Coverage::Complete),
                    (Modality::Speech, Coverage::Partial),
                ]),
                vec![],
            )))
        })
    }
}

async fn sample() -> ModerationResult<Value> {
    let text = Moderation::text(&TextInput::new("本地 UTF-8 样本 🙂")?).await?;
    let media = Moderation::driver("video")?;
    let object = StoredMedia::new("private", "images/post.png", "revision-1")?;
    let image = media.image(&ImageInput::stored(object)).await?;
    Ok(json!({
        "text_review":text.decision(&[Modality::Text]) == Some(Decision::Review),
        "image_allow":image.decision(&[Modality::Visual]) == Some(Decision::Allow),
        "ocr_missing":image.decision(&[Modality::Visual, Modality::Ocr]).is_none(),
        "no_video_on_text":!Moderation::default_driver()?.supports(Operation::SubmitVideo),
    }))
}

pub(super) fn route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let result = sample().await.map_err(|error| HttpError::Internal {
            source: Box::new(error),
        })?;
        Response::json(StatusCode::OK, result)
    })
}

async fn aliyun(text: &str, language: Option<&str>) -> ModerationResult<Value> {
    let mut input = TextInput::new(text)?;
    if let Some(language) = language {
        input = input.with_language(language)?;
    }
    let result = Moderation::driver("aliyun-comments")?.text(&input).await?;
    let labels: Vec<_> = result
        .findings()
        .iter()
        .map(|finding| json!({"category":finding.category,"provider":finding.provider_label}))
        .collect();
    Ok(json!({
        "decision":result.decision(&[Modality::Text]).map(|value| format!("{value:?}")),
        "request_id":result.request_id(), "labels":labels,
    }))
}

pub(super) fn aliyun_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let raw = request.body_bytes().await?;
        let text = std::str::from_utf8(&raw).map_err(|_| HttpError::InvalidRequest)?;
        match aliyun(text, request.header("x-fixture-language")?).await {
            Ok(value) => Response::json(StatusCode::OK, value),
            Err(error) => {
                let status = match error {
                    ModerationError::InvalidInput(_) | ModerationError::Rejected => {
                        StatusCode::BAD_REQUEST
                    }
                    ModerationError::Unauthorized => StatusCode::UNAUTHORIZED,
                    ModerationError::Throttled => StatusCode::TOO_MANY_REQUESTS,
                    ModerationError::Timeout => StatusCode::GATEWAY_TIMEOUT,
                    _ => StatusCode::BAD_GATEWAY,
                };
                Response::json(status, json!({"error":format!("{error:?}")}))
            }
        }
    })
}

struct Query;
impl JobHandler for Query {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let id: TaskId = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let media = Moderation::driver("video").map_err(queue_error)?;
            let TaskStatus::Succeeded(result) =
                media.query_video(&id).await.map_err(queue_error)?
            else {
                return Err(QueueError::InvalidPayload);
            };
            let path = std::env::var_os("MODERATION_FIXTURE_RESULT")
                .map(PathBuf::from)
                .ok_or(QueueError::InvalidPayload)?;
            let record = json!({"id":id,"speech_partial":result.coverage(Modality::Speech) == Coverage::Partial,
                "allow":result.decision(&[Modality::Visual, Modality::Speech]).is_some()});
            std::fs::write(path, record.to_string()).map_err(queue_error)
        })
    }
}

fn queue_error(source: impl std::error::Error + Send + Sync + 'static) -> QueueError {
    QueueError::Execution {
        source: Arc::new(source),
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
            let value = if input
                .option("queue")
                .is_some_and(|value| value.as_bool() == Some(true))
            {
                let object = StoredMedia::new("private", "videos/post.mp4", "revision-2")
                    .map_err(ConsoleError::from_source)?;
                let id = Moderation::driver("video")
                    .map_err(ConsoleError::from_source)?
                    .submit_video(&VideoInput::new(object))
                    .await
                    .map_err(ConsoleError::from_source)?;
                let payload = Payload::new(JOB, &id).map_err(ConsoleError::from_source)?;
                Queue::dispatch(payload, DispatchOptions::default(), None)
                    .await
                    .map_err(ConsoleError::from_source)?;
                json!(id)
            } else {
                sample().await.map_err(ConsoleError::from_source)?
            };
            output
                .write_raw(&format!("{value}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

pub(super) fn register_command(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(
        clap::Command::new("drivers:moderation").arg(
            clap::Arg::new("queue")
                .long("queue")
                .action(clap::ArgAction::SetTrue),
        ),
    ))))?;
    artisan.add_command(Box::new(AliyunProbe(CommandDefinition::new(
        clap::Command::new("drivers:moderation-aliyun").arg(clap::Arg::new("text").required(true)),
    ))))
}

struct AliyunProbe(CommandDefinition);

impl Command for AliyunProbe {
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
            let text = input.argument("text").expect("fixture command input");
            let result = aliyun(text.as_str().expect("fixture text"), None)
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
