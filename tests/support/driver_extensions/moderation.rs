#[path = "../../../database/migrations/0001_01_01_000002_create_jobs_table.rs"]
mod jobs;

use openrust::{
    config::{ConfigError, Environment},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{
        container::Container as _,
        events::{Event as EventPayload, Listener},
        foundation::Application as _,
        queue::JobHandler,
    },
    database::Db,
    database::migrations::MigrationEntry,
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client::events::RequestSending},
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::{ProviderResult, ServiceProvider, facades::Queue},
};
use openrust_content_moderation::{
    Assessment, Coverage, Decision, ImageInput, Modality, Moderation, ModerationDriver,
    ModerationError, ModerationFuture, ModerationManager, ModerationResult, ModerationStore,
    Operation, ProfileRevision, StoredMedia, TaskId, TaskStatus, TextInput, VideoInput,
    VideoReview, VideoReviewRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

const JOB: &str = "moderation.fixture.query";
const ALIYUN_JOB: &str = "moderation.fixture.aliyun-query";
const TENCENT_JOB: &str = "moderation.fixture.tencent-query";
const SIGHTENGINE_JOB: &str = "moderation.fixture.sightengine-query";
const HIVE_VIDEO_JOB: &str = "moderation.fixture.hive-video-query";
const REVIEW_JOB: &str = "moderation.fixture.read-review";
const SUBMIT_REVIEW_JOB: &str = "moderation.fixture.submit-review";
const QUERY_REVIEW_JOB: &str = "moderation.fixture.query-review";

pub(super) fn schedule(
    schedule: &mut openrust::console::scheduling::Schedule,
) -> Result<(), ConsoleError> {
    if let Ok(connection) = std::env::var("MODERATION_FIXTURE_RECOVERY_QUEUE") {
        schedule
            .command(
                "moderation:recover",
                [
                    "app".into(),
                    "tenant".into(),
                    format!("--connection={connection}").into(),
                    "--queue=moderation-recovery".into(),
                    "--limit=1".into(),
                ],
            )
            .every_minute();
    }
    Ok(())
}

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
        if let Some(origin) = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")? {
            configuration["drivers"]["aliyun-images"] = json!({
                "driver":"aliyun", "base_url":endpoint, "image_service":"baselineCheck",
                "media_disk":"s3", "media_origin":origin,
                "access_key_id_env":"FIXTURE_ALIYUN_ID", "access_key_secret_env":"FIXTURE_ALIYUN_SECRET",
                "security_token_env":"FIXTURE_ALIYUN_TOKEN", "timeout_ms":3000,
            });
        }
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_ALIYUN_VIDEO")? {
        let media = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")?;
        configuration["drivers"]["aliyun-videos"] = json!({
            "driver":"aliyun", "base_url":endpoint,
            "video_service":env.get_utf8("MODERATION_FIXTURE_VIDEO_SERVICE")?.unwrap_or("videoDetection"),
            "region":env.get_utf8("MODERATION_FIXTURE_VIDEO_REGION")?.unwrap_or("cn-shanghai"),
            "media_disk":"s3", "media_origin":media,
            "access_key_id_env":"FIXTURE_ALIYUN_ID", "access_key_secret_env":"FIXTURE_ALIYUN_SECRET",
            "security_token_env":"FIXTURE_ALIYUN_TOKEN", "timeout_ms":500,
        });
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_TENCENT")? {
        configuration["drivers"]["tencent-default"] = json!({
            "driver":"tencent", "base_url":endpoint, "region":"ap-guangzhou", "biz_type":"comments",
            "secret_id_env":"FIXTURE_TENCENT_ID", "secret_key_env":"FIXTURE_TENCENT_SECRET",
            "security_token_env":"FIXTURE_TENCENT_TOKEN", "timeout_ms":350,
        });
        let mut english = configuration["drivers"]["tencent-default"].clone();
        english["english_text"] = json!(true);
        configuration["drivers"]["tencent-english"] = english;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_TENCENT_IMAGE")? {
        let mut images = json!({
            "driver":"tencent", "image_base_url":endpoint, "region":"ap-guangzhou",
            "image_biz_type":"images", "secret_id_env":"FIXTURE_TENCENT_ID",
            "secret_key_env":"FIXTURE_TENCENT_SECRET", "security_token_env":"FIXTURE_TENCENT_TOKEN",
            "timeout_ms":3000,
        });
        if let Some(origin) = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")? {
            images["media_disk"] = json!("s3");
            images["media_origin"] = json!(origin);
        }
        configuration["drivers"]["tencent-images"] = images;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_TENCENT_VIDEO")? {
        let media = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")?;
        configuration["drivers"]["tencent-videos"] = json!({
            "driver":"tencent", "video_base_url":endpoint, "region":"ap-guangzhou",
            "video_biz_type":env.get_utf8("MODERATION_FIXTURE_TENCENT_BIZ")?.unwrap_or("video_profile"),
            "media_disk":"s3", "media_origin":media,
            "secret_id_env":"FIXTURE_TENCENT_ID","secret_key_env":"FIXTURE_TENCENT_SECRET",
            "security_token_env":"FIXTURE_TENCENT_TOKEN","timeout_ms":500,
        });
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_SIGHTENGINE")? {
        let mut text = json!({
            "driver":"sightengine", "base_url":endpoint, "api_user_env":"FIXTURE_SIGHTENGINE_USER",
            "api_secret_env":"FIXTURE_SIGHTENGINE_SECRET", "timeout_ms":350,
        });
        let mut rules = text.clone();
        rules["text_mode"] = json!("rules");
        rules["text_languages"] = json!(["en", "zh"]);
        rules["text_categories"] = json!(["profanity", "personal"]);
        configuration["drivers"]["sightengine-rules"] = rules;
        text["text_mode"] = json!("ml");
        text["text_languages"] = json!(["en"]);
        text["text_models"] = json!(["general", "self-harm"]);
        text["text_review_score"] = json!(0.8);
        configuration["drivers"]["sightengine-ml"] = text.clone();
        text["text_languages"] = json!(["en", "fr"]);
        configuration["drivers"]["sightengine-bad-ml"] = text;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_SIGHTENGINE_IMAGE")? {
        let mut image = json!({
            "driver":"sightengine", "base_url":endpoint, "api_user_env":"FIXTURE_SIGHTENGINE_USER",
            "api_secret_env":"FIXTURE_SIGHTENGINE_SECRET", "timeout_ms":3000,
            "image_models":["violence","nudity-2.1","text-content-2.0"],
            "image_review_score":0.8, "image_text_categories":["email","insult"],
        });
        if let Some(origin) = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")? {
            image["media_disk"] = json!("s3");
            image["media_origin"] = json!(origin);
        }
        configuration["drivers"]["sightengine-images"] = image;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_SIGHTENGINE_VIDEO")? {
        let timeout = env
            .get_utf8("MODERATION_FIXTURE_SIGHTENGINE_VIDEO_TIMEOUT")?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2500);
        let mut profile = json!({
            "driver":"sightengine", "base_url":endpoint, "api_user_env":"FIXTURE_SIGHTENGINE_USER",
            "api_secret_env":"FIXTURE_SIGHTENGINE_SECRET", "timeout_ms":timeout,
            "media_disk":"s3", "media_origin":env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")?,
            "video_models":["violence","text-content-2.0"], "video_text_categories":["email"],
            "video_review_score":0.8, "video_max_duration":900,
        });
        if let Some(directory) = env.get_utf8("MODERATION_CREDENTIAL_DIRECTORY")? {
            configuration["credential_directory"] = json!(directory);
            let fields = profile.as_object_mut().expect("点播审核配置对象");
            fields.remove("api_user_env");
            fields.remove("api_secret_env");
            profile["credential_ref"] = json!(
                env.get_utf8("MODERATION_FIXTURE_CREDENTIAL_REF")?
                    .unwrap_or("sightengine")
            );
            profile["credential_revision"] = json!(
                env.get_utf8("MODERATION_FIXTURE_CREDENTIAL_REVISION")?
                    .unwrap_or("r1")
            );
            if let Some(old_endpoint) = env.get_utf8("MODERATION_FIXTURE_PIN_ENDPOINT")? {
                let mut original = profile.clone();
                original["base_url"] = json!(old_endpoint);
                original["account"] = json!("original-account");
                original["credential_revision"] = json!("r1");
                original["policy_revision"] = json!("policy-1");
                original["required_modalities"] = json!(["visual", "speech", "ocr"]);
                for (key, variable) in [
                    (
                        "query_interval_seconds",
                        "MODERATION_FIXTURE_QUERY_INTERVAL",
                    ),
                    ("query_max_attempts", "MODERATION_FIXTURE_QUERY_ATTEMPTS"),
                ] {
                    if let Some(value) = env.get_utf8(variable)? {
                        original[key] = json!(value.parse::<u16>().expect("验收查单参数"));
                    }
                }
                if env.get_utf8("MODERATION_FIXTURE_QUERY_EXTERNAL")? == Some("1") {
                    original["driver"] = json!("fixture-media");
                    original["region"] = json!("original");
                    original["required_modalities"] = json!(["visual"]);
                }
                let change = env.get_utf8("MODERATION_FIXTURE_PIN_CHANGE")?;
                match change {
                    Some("account") => original["account"] = json!("different-account"),
                    Some("policy") => original["policy_revision"] = json!("policy-replacement"),
                    Some("endpoint") => original["base_url"] = json!("http://127.0.0.1:1"),
                    Some("credential") => original["credential_revision"] = json!("r2"),
                    _ => (),
                }
                if change != Some("missing") {
                    configuration["profile_revisions"]["sightengine-videos"]["original"] = original;
                }
                configuration["default"] = json!("video");
            }
        }
        configuration["drivers"]["sightengine-videos"] = profile;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_HIVE")? {
        configuration["drivers"]["hive-text"] = json!({
            "driver":"hive", "base_url":endpoint, "token_env":"FIXTURE_HIVE_TOKEN",
            "text_project":48172, "text_classes":["hate","spam","violence"],
            "text_languages":["es"], "text_review_severity":2, "timeout_ms":350,
        });
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_HIVE_IMAGE")? {
        let mut profiles = json!({
            "driver":"hive", "base_url":endpoint, "image_token_env":"FIXTURE_HIVE_IMAGE_TOKEN",
            "image_project":48173, "image_model_type":"CATEGORIZATION",
            "image_classes":["general_nsfw","gun_in_hand"], "image_review_score":0.9,
            "token_env":"FIXTURE_HIVE_TOKEN", "text_project":48172, "text_classes":["hate","spam","violence"],
            "text_review_severity":2, "timeout_ms":3000,
        });
        if let Some(origin) = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")? {
            profiles["media_disk"] = json!("s3");
            profiles["media_origin"] = json!(origin);
        }
        configuration["drivers"]["hive-images"] = profiles;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_HIVE_OCR")? {
        let mut profile = json!({
            "driver":"hive", "base_url":endpoint, "ocr_token_env":"FIXTURE_HIVE_OCR_TOKEN",
            "ocr_project":48174, "ocr_classes":["spam","hate"], "ocr_review_severity":2,
            "timeout_ms":3000,
        });
        if let Some(origin) = env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")? {
            profile["media_disk"] = json!("s3");
            profile["media_origin"] = json!(origin);
        }
        configuration["drivers"]["hive-ocr"] = profile;
    }
    if let Some(endpoint) = env.get_utf8("MODERATION_FIXTURE_HIVE_VIDEO")? {
        let mut video = json!({
            "driver":"hive", "base_url":endpoint, "video_project":48175,
            "video_callback_url":env.get_utf8("MODERATION_FIXTURE_HIVE_CALLBACK")?,
            "media_disk":"s3", "media_origin":env.get_utf8("MODERATION_FIXTURE_IMAGE_ORIGIN")?,
            "credential_ref":"hive-video", "credential_revision":"r1", "timeout_ms":1000,
        });
        if let Some(api) = env.get_utf8("MODERATION_FIXTURE_HIVE_VIDEO_QUERY")? {
            video["video_query_api"] = json!(api);
            video["video_classes"] = json!(["general_nsfw", "gun_in_hand"]);
            video["video_review_score"] = json!(0.9);
        }
        configuration["credential_directory"] =
            json!(env.get_utf8("MODERATION_CREDENTIAL_DIRECTORY")?);
        configuration["drivers"]["hive-videos"] = video.clone();
        video["account"] = json!("hive-video-account");
        video["policy_revision"] = json!("hive-policy-1");
        video["required_modalities"] = json!(["visual", "speech", "ocr"]);
        configuration["profile_revisions"]["hive-videos"]["original"] = video;
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
            Queue::register(REVIEW_JOB, Arc::new(ReviewWorker::Lookup))?;
            Queue::register(SUBMIT_REVIEW_JOB, Arc::new(ReviewWorker::Submit))?;
            Queue::register(QUERY_REVIEW_JOB, Arc::new(ReviewWorker::Query))?;
            Queue::register(ALIYUN_JOB, Arc::new(ProviderVideoQuery("aliyun-videos")))?;
            Queue::register(TENCENT_JOB, Arc::new(ProviderVideoQuery("tencent-videos")))?;
            Queue::register(
                SIGHTENGINE_JOB,
                Arc::new(ProviderVideoQuery("sightengine-videos")),
            )?;
            Queue::register(HIVE_VIDEO_JOB, Arc::new(ProviderVideoQuery("hive-videos")))?;
            if let Some(path) = std::env::var_os("MODERATION_EVENT_OBSERVATIONS") {
                app.container().make::<Dispatcher>()?.register(|registry| {
                    registry.listen(
                        r"OpenRust\Http\Client\Events\RequestSending",
                        Arc::new(ObserveJobRequest(PathBuf::from(path))),
                        Delivery::Sync,
                    )?;
                    Ok(())
                })?;
            }
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

struct ObserveJobRequest(PathBuf);

impl Listener for ObserveJobRequest {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            use std::io::Write as _;

            let event = event
                .downcast_ref::<RequestSending>()
                .expect("fixture HTTP event");
            if event.request.url.path() == "/1.0/video/byid.json" {
                assert!(event.request.url.query().is_none());
                assert!(event.request.body.is_none());
                assert!(event.request.diagnostic_source.is_none());
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.0)
                    .expect("fixture event file");
                writeln!(file, "{}", event.request.method).expect("fixture event record");
            }
            Ok(None)
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
    Ok(render_assessment(result, Modality::Text))
}

async fn aliyun_image(version: &str, disk: &str) -> ModerationResult<Value> {
    let image = ImageInput::stored(StoredMedia::new(disk, "versions/image.png", version)?);
    let result = Moderation::driver("aliyun-images")?.image(&image).await?;
    Ok(render_assessment(result, Modality::Visual))
}

async fn submit_video(profile: &str, version: &str, disk: &str) -> ModerationResult<Value> {
    let source = StoredMedia::new(disk, "versions/clip.mp4", version)?;
    let driver = Moderation::driver(profile)?;
    let task = driver.submit_video(&VideoInput::new(source)).await?;
    Ok(json!({"task":task,"cancel":driver.supports(Operation::CancelVideo)}))
}

async fn query_video(profile: &str, id: &TaskId) -> ModerationResult<Value> {
    let driver = Moderation::driver(profile)?;
    Ok(render_video_status(driver.query_video(id).await?))
}

#[derive(Deserialize, Serialize)]
struct PinnedQuery {
    profile: ProfileRevision,
    id: TaskId,
}

async fn query_pinned(query: &PinnedQuery) -> ModerationResult<Value> {
    let driver = Moderation::driver_for_revision(&query.profile)?;
    Ok(render_video_status(driver.query_video(&query.id).await?))
}

fn render_video_status(status: TaskStatus) -> Value {
    let stage = match status {
        TaskStatus::Pending => "Pending",
        TaskStatus::Running => "Running",
        TaskStatus::Failed => "Failed",
        TaskStatus::Cancelled => "Cancelled",
        TaskStatus::Expired => "Expired",
        TaskStatus::Succeeded(result) => {
            let modalities = [Modality::Visual, Modality::Speech, Modality::Ocr];
            let coverage: BTreeMap<_, _> = modalities
                .into_iter()
                .map(|modality| {
                    (
                        format!("{modality:?}"),
                        format!("{:?}", result.coverage(modality)),
                    )
                })
                .collect();
            let decision = result
                .decision(&modalities)
                .map(|value| format!("{value:?}"));
            let mut response = render_assessment(result, Modality::Visual);
            response["decision"] = json!(decision);
            response["coverage"] = json!(coverage);
            return json!({"status":"Succeeded","assessment":response});
        }
    };
    json!({"status":stage})
}

pub(super) fn aliyun_video_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    video_route(request, "aliyun-videos")
}

pub(super) fn tencent_video_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    video_route(request, "tencent-videos")
}

pub(super) fn sightengine_video_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    video_route(request, "sightengine-videos")
}

pub(super) fn hive_video_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    video_route(request, "hive-videos")
}

fn video_route<'a>(
    request: &'a mut Request,
    profile: &'static str,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        let result = if let Some(id) = request.header("x-fixture-query-review")? {
            query_review("app", "tenant", id).await
        } else if let Some(id) = request.header("x-fixture-submit-review")? {
            submit_review("app", "tenant", id).await
        } else if let Some(intent) = request.header("x-fixture-review-intent")? {
            let version = request
                .header("x-fixture-version")?
                .ok_or(HttpError::InvalidRequest)?;
            stage_review("app", "tenant", intent, version).await
        } else if let Some(query) = request.header("x-fixture-pinned-query")? {
            let query = serde_json::from_str(query).map_err(|_| HttpError::InvalidRequest)?;
            query_pinned(&query).await
        } else if let Some(id) = request.header("x-fixture-task")? {
            match TaskId::new(id) {
                Ok(id) if request.header("x-fixture-cancel")? == Some("1") => {
                    cancel_video(profile, &id).await
                }
                Ok(id) => query_video(profile, &id).await,
                Err(error) => Err(error),
            }
        } else {
            let version = request
                .header("x-fixture-version")?
                .ok_or(HttpError::InvalidRequest)?;
            let disk = request.header("x-fixture-disk")?.unwrap_or("s3");
            submit_video(profile, version, disk).await
        };
        provider_response(result)
    })
}

async fn cancel_video(profile: &str, id: &TaskId) -> ModerationResult<Value> {
    Moderation::driver(profile)?.cancel_video(id).await?;
    Ok(json!({"cancelled":true}))
}

fn render_assessment(result: Assessment, modality: Modality) -> Value {
    let labels: Vec<_> = result
        .findings()
        .iter()
        .enumerate()
        .map(|(index, finding)| {
            let mut value = json!({"category":finding.category,"provider":finding.provider_label});
            if let Some(score) = result.confidence(index) {
                value["confidence"] = json!({"value":score.value(),"maximum":score.maximum()});
            }
            if let Some(span) = result.evidence_span(index) {
                value["span_ms"] = json!([span.start_ms(), span.end_ms()]);
                value["modality"] = json!(format!("{:?}", finding.modality));
            }
            value
        })
        .collect();
    json!({
        "decision":result.decision(&[modality]).map(|value| format!("{value:?}")),
        "request_id":result.request_id(), "labels":labels,
    })
}

pub(super) fn aliyun_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let raw = request.body_bytes().await?;
        let text = std::str::from_utf8(&raw).map_err(|_| HttpError::InvalidRequest)?;
        provider_response(aliyun(text, request.header("x-fixture-language")?).await)
    })
}

pub(super) fn aliyun_image_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let version = request
            .header("x-fixture-version")?
            .ok_or(HttpError::InvalidRequest)?;
        let disk = request.header("x-fixture-disk")?.unwrap_or("s3");
        provider_response(aliyun_image(version, disk).await)
    })
}

async fn tencent(text: &str, language: Option<&str>, profile: &str) -> ModerationResult<Value> {
    let mut input = TextInput::new(text)?;
    if let Some(language) = language {
        input = input.with_language(language)?;
    }
    let result = Moderation::driver(profile)?.text(&input).await?;
    Ok(render_assessment(result, Modality::Text))
}

pub(super) fn tencent_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let raw = request.body_bytes().await?;
        let text = std::str::from_utf8(&raw).map_err(|_| HttpError::InvalidRequest)?;
        let profile = request
            .header("x-fixture-profile")?
            .unwrap_or("tencent-default");
        provider_response(tencent(text, request.header("x-fixture-language")?, profile).await)
    })
}

async fn configured_image(profile: &str, input: ImageInput) -> ModerationResult<Value> {
    let result = Moderation::driver(profile)?.image(&input).await?;
    let visual = format!("{:?}", result.coverage(Modality::Visual));
    let ocr = format!("{:?}", result.coverage(Modality::Ocr));
    let combined = result.decision(&[Modality::Visual, Modality::Ocr]);
    let modality = if profile == "hive-ocr" {
        Modality::Ocr
    } else {
        Modality::Visual
    };
    let mut value = render_assessment(result, modality);
    value["visual"] = json!(visual);
    value["ocr"] = json!(ocr);
    value["combined"] = json!(combined.map(|decision| format!("{decision:?}")));
    Ok(value)
}

pub(super) fn sightengine_image_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    image_route(request, "sightengine-images")
}

pub(super) fn tencent_image_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    image_route(request, "tencent-images")
}

pub(super) fn hive_image_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    image_route(request, "hive-images")
}

pub(super) fn hive_ocr_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    image_route(request, "hive-ocr")
}

fn image_route<'a>(
    request: &'a mut Request,
    profile: &'static str,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = if let Some(version) = request.header("x-fixture-version")? {
            let disk = request.header("x-fixture-disk")?.unwrap_or("s3");
            StoredMedia::new(disk, "versions/image.png", version).map(ImageInput::stored)
        } else {
            ImageInput::inline(request.body_bytes().await?.to_vec())
        };
        provider_response(match input {
            Ok(input) => configured_image(profile, input).await,
            Err(error) => Err(error),
        })
    })
}

async fn configured_text(
    text: &str,
    language: Option<&str>,
    profile: &str,
) -> ModerationResult<Value> {
    let mut input = TextInput::new(text)?;
    if let Some(language) = language {
        input = input.with_language(language)?;
    }
    let result = Moderation::driver(profile)?.text(&input).await?;
    Ok(render_assessment(result, Modality::Text))
}

pub(super) fn sightengine_route(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let raw = request.body_bytes().await?;
        let text = std::str::from_utf8(&raw).map_err(|_| HttpError::InvalidRequest)?;
        let profile = request
            .header("x-fixture-profile")?
            .unwrap_or("sightengine-rules");
        provider_response(
            configured_text(text, request.header("x-fixture-language")?, profile).await,
        )
    })
}

pub(super) fn hive_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let raw = request.body_bytes().await?;
        let text = std::str::from_utf8(&raw).map_err(|_| HttpError::InvalidRequest)?;
        let profile = request.header("x-fixture-profile")?.unwrap_or("hive-text");
        provider_response(
            configured_text(text, request.header("x-fixture-language")?, profile).await,
        )
    })
}

fn provider_response(result: ModerationResult<Value>) -> Result<Response, HttpError> {
    match result {
        Ok(value) => Response::json(StatusCode::OK, value),
        Err(error) => {
            let status = match error {
                ModerationError::InvalidInput(_) | ModerationError::Rejected => {
                    StatusCode::BAD_REQUEST
                }
                ModerationError::Unauthorized => StatusCode::UNAUTHORIZED,
                ModerationError::ReviewConflict => StatusCode::CONFLICT,
                ModerationError::ReviewNotFound => StatusCode::NOT_FOUND,
                ModerationError::Throttled => StatusCode::TOO_MANY_REQUESTS,
                ModerationError::Timeout => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::BAD_GATEWAY,
            };
            Response::json(status, json!({"error":format!("{error:?}")}))
        }
    }
}

fn review_store(application: &str, tenant: &str) -> ModerationResult<ModerationStore> {
    let connection = Db::connection(None).map_err(|_| ModerationError::ReviewStorage)?;
    ModerationStore::new(connection, application, tenant)
}

async fn stage_review(
    application: &str,
    tenant: &str,
    intent: &str,
    version: &str,
) -> ModerationResult<Value> {
    let source = StoredMedia::new("s3", "versions/clip.mp4", version)?;
    let name = std::env::var("MODERATION_FIXTURE_DURABLE_VIDEO_PROFILE")
        .unwrap_or_else(|_| "sightengine-videos".to_owned());
    let profile = Moderation::pin_profile(&name, "original")?;
    let request = VideoReviewRequest::new(intent, "影片/1", "content-v1", source, profile, 300)?;
    let store = review_store(application, tenant)?;
    Ok(render_review(store.stage_video(&request).await?))
}

fn render_review(review: VideoReview) -> Value {
    use openrust_content_moderation::VideoSubmission;
    let submission = match review.submission() {
        VideoSubmission::Ready => json!({"state":"Ready"}),
        VideoSubmission::Submitting => json!({"state":"Submitting"}),
        VideoSubmission::Submitted(id) => json!({"state":"Submitted","task":id}),
        VideoSubmission::Unknown => json!({"state":"Unknown"}),
        VideoSubmission::Failed(reason) => json!({"state":"Failed","reason":reason}),
        VideoSubmission::Expired => json!({"state":"Expired"}),
    };
    let assessment = review.assessment().cloned().map(|record| {
        let coverage: BTreeMap<_, _> = [Modality::Visual, Modality::Speech, Modality::Ocr]
            .into_iter()
            .map(|kind| (format!("{kind:?}"), format!("{:?}", record.coverage(kind))))
            .collect();
        let mut rendered = render_assessment(record, Modality::Visual);
        rendered["coverage"] = json!(coverage);
        rendered
    });
    json!({
        "id":review.id(), "check_id":review.check_id(),
        "content_id":review.content_id(), "content_revision":review.content_revision(),
        "object_version":review.source().version(), "profile":review.profile(),
        "created_at":review.created_at(), "deadline_at":review.deadline_at(),
        "version":review.version(), "state":format!("{:?}",review.state()), "submission":submission,
        "query_attempts":review.query_attempts(), "query_failure":review.query_failure(),
        "next_query_at":review.next_query_at(), "assessment":assessment, "decision":review.decision(),
        "callback_receipt":review.confirmation_callback_id(),
    })
}

async fn submit_review(application: &str, tenant: &str, id: &str) -> ModerationResult<Value> {
    let store = review_store(application, tenant)?;
    let service =
        openrust_content_moderation::VideoReviewService::new(store, Moderation::manager()?);
    Ok(render_review(service.submit(id).await?))
}

async fn query_review(application: &str, tenant: &str, id: &str) -> ModerationResult<Value> {
    let store = review_store(application, tenant)?;
    let service =
        openrust_content_moderation::VideoReviewService::new(store, Moderation::manager()?);
    Ok(render_review(service.query(id).await?))
}

#[derive(Deserialize, Serialize)]
struct ReviewReference {
    application: String,
    tenant: String,
    id: String,
}

impl ReviewReference {
    async fn read(&self) -> ModerationResult<Value> {
        let store = review_store(&self.application, &self.tenant)?;
        Ok(store
            .find(&self.id)
            .await?
            .map_or(Value::Null, render_review))
    }
}

enum ReviewWorker {
    Lookup,
    Submit,
    Query,
}

impl JobHandler for ReviewWorker {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let reference: ReviewReference = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let record = match self {
                Self::Lookup => reference.read().await,
                Self::Submit => {
                    submit_review(&reference.application, &reference.tenant, &reference.id).await
                }
                Self::Query => {
                    query_review(&reference.application, &reference.tenant, &reference.id).await
                }
            }
            .map_err(queue_error)?;
            let path = std::env::var_os("MODERATION_FIXTURE_RESULT")
                .map(PathBuf::from)
                .ok_or(QueueError::InvalidPayload)?;
            std::fs::write(path, record.to_string()).map_err(queue_error)
        })
    }
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

struct ProviderVideoQuery(&'static str);

#[derive(Deserialize)]
#[serde(untagged)]
enum QueryPayload {
    Current(TaskId),
    Pinned(Box<PinnedQuery>),
}

impl JobHandler for ProviderVideoQuery {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let query: QueryPayload = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let record = match query {
                QueryPayload::Current(id) => query_video(self.0, &id).await,
                QueryPayload::Pinned(query) => query_pinned(&query).await,
            }
            .map_err(queue_error)?;
            let path = std::env::var_os("MODERATION_FIXTURE_RESULT")
                .map(PathBuf::from)
                .ok_or(QueueError::InvalidPayload)?;
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
    artisan.add_command(Box::new(ReviewProbe(CommandDefinition::new(
        clap::Command::new("drivers:moderation-review")
            .arg(clap::Arg::new("action").required(true).value_parser([
                "stage",
                "find",
                "queue",
                "submit",
                "queue-submit",
                "query",
                "queue-query",
            ]))
            .arg(clap::Arg::new("application").required(true))
            .arg(clap::Arg::new("tenant").required(true))
            .arg(clap::Arg::new("reference").required(true))
            .arg(clap::Arg::new("object-version")),
    ))))?;
    artisan.add_command(Box::new(Probe(CommandDefinition::new(
        clap::Command::new("drivers:moderation").arg(
            clap::Arg::new("queue")
                .long("queue")
                .action(clap::ArgAction::SetTrue),
        ),
    ))))?;
    artisan.add_command(Box::new(AliyunProbe(CommandDefinition::new(
        clap::Command::new("drivers:moderation-aliyun")
            .arg(clap::Arg::new("text").required(true))
            .arg(
                clap::Arg::new("image")
                    .long("image")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))?;
    for (name, profile, job) in [
        (
            "drivers:moderation-aliyun-video",
            "aliyun-videos",
            ALIYUN_JOB,
        ),
        (
            "drivers:moderation-tencent-video",
            "tencent-videos",
            TENCENT_JOB,
        ),
        (
            "drivers:moderation-sightengine-video",
            "sightengine-videos",
            SIGHTENGINE_JOB,
        ),
        (
            "drivers:moderation-hive-video",
            "hive-videos",
            HIVE_VIDEO_JOB,
        ),
    ] {
        artisan.add_command(Box::new(NamedVideoProbe(
            CommandDefinition::new(
                clap::Command::new(name)
                    .arg(clap::Arg::new("action").required(true).value_parser([
                        "submit",
                        "query",
                        "queue",
                        "cancel",
                        "pin",
                        "query-pinned",
                        "queue-pinned",
                    ]))
                    .arg(clap::Arg::new("reference").required(true)),
            ),
            profile,
            job,
        )))?;
    }
    for (name, profile) in [
        ("drivers:moderation-sightengine", "sightengine-rules"),
        ("drivers:moderation-hive", "hive-text"),
    ] {
        artisan.add_command(Box::new(NamedTextProbe(
            CommandDefinition::new(
                clap::Command::new(name).arg(clap::Arg::new("text").required(true)),
            ),
            profile,
        )))?;
    }
    for (name, profile) in [
        ("drivers:moderation-sightengine-image", "sightengine-images"),
        ("drivers:moderation-hive-image", "hive-images"),
        ("drivers:moderation-hive-ocr", "hive-ocr"),
    ] {
        artisan.add_command(Box::new(NamedImageProbe(
            CommandDefinition::new(
                clap::Command::new(name).arg(clap::Arg::new("object-version").required(true)),
            ),
            profile,
        )))?;
    }
    artisan.add_command(Box::new(TencentProbe(CommandDefinition::new(
        clap::Command::new("drivers:moderation-tencent")
            .arg(clap::Arg::new("text").required(true))
            .arg(
                clap::Arg::new("image")
                    .long("image")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
}

struct ReviewProbe(CommandDefinition);

impl Command for ReviewProbe {
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
            let required = |key| {
                input
                    .argument(key)
                    .and_then(|field| field.as_str().map(str::to_owned))
                    .expect("验收命令要求字符串参数")
            };
            let reference = ReviewReference {
                application: required("application"),
                tenant: required("tenant"),
                id: required("reference"),
            };
            let action = required("action");
            let value = match action.as_str() {
                "stage" => stage_review(
                    &reference.application,
                    &reference.tenant,
                    &reference.id,
                    &required("object-version"),
                )
                .await
                .map_err(ConsoleError::from_source)?,
                "find" => reference.read().await.map_err(ConsoleError::from_source)?,
                "submit" => submit_review(&reference.application, &reference.tenant, &reference.id)
                    .await
                    .map_err(ConsoleError::from_source)?,
                "query" => query_review(&reference.application, &reference.tenant, &reference.id)
                    .await
                    .map_err(ConsoleError::from_source)?,
                "queue" | "queue-submit" | "queue-query" => {
                    let job = match action.as_str() {
                        "queue-submit" => SUBMIT_REVIEW_JOB,
                        "queue-query" => QUERY_REVIEW_JOB,
                        _ => REVIEW_JOB,
                    };
                    let payload =
                        Payload::new(job, &reference).map_err(ConsoleError::from_source)?;
                    Queue::dispatch(payload, DispatchOptions::default(), None)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    json!({"queued":true})
                }
                _ => unreachable!("验收命令已检查操作"),
            };
            output
                .write_raw(&format!("{value}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct NamedVideoProbe(CommandDefinition, &'static str, &'static str);

impl Command for NamedVideoProbe {
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
            let action = input.argument("action").expect("fixture action");
            let reference = input.argument("reference").expect("fixture reference");
            let reference = reference.as_str().expect("fixture reference text");
            let result = match action.as_str() {
                Some("pin") => json!(
                    Moderation::pin_profile(self.1, reference)
                        .map_err(ConsoleError::from_source)?
                ),
                Some("query-pinned") => {
                    let query =
                        serde_json::from_str(reference).map_err(ConsoleError::from_source)?;
                    query_pinned(&query)
                        .await
                        .map_err(ConsoleError::from_source)?
                }
                Some("queue-pinned") => {
                    let query: PinnedQuery =
                        serde_json::from_str(reference).map_err(ConsoleError::from_source)?;
                    let payload =
                        Payload::new(self.2, &query).map_err(ConsoleError::from_source)?;
                    Queue::dispatch(payload, DispatchOptions::default(), None)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    json!({"queued":true})
                }
                Some("submit") => submit_video(self.1, reference, "s3")
                    .await
                    .map_err(ConsoleError::from_source)?,
                Some("query" | "cancel") => {
                    let id = TaskId::new(reference).map_err(ConsoleError::from_source)?;
                    let result = if action.as_str() == Some("query") {
                        query_video(self.1, &id).await
                    } else {
                        cancel_video(self.1, &id).await
                    };
                    result.map_err(ConsoleError::from_source)?
                }
                Some("queue") => {
                    let id = TaskId::new(reference).map_err(ConsoleError::from_source)?;
                    let payload = Payload::new(self.2, &id).map_err(ConsoleError::from_source)?;
                    Queue::dispatch(payload, DispatchOptions::default(), None)
                        .await
                        .map_err(ConsoleError::from_source)?;
                    json!({"queued":true})
                }
                _ => unreachable!("fixture action validated"),
            };
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct NamedTextProbe(CommandDefinition, &'static str);

impl Command for NamedTextProbe {
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
            let result = configured_text(text.as_str().expect("fixture text"), None, self.1)
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct NamedImageProbe(CommandDefinition, &'static str);

impl Command for NamedImageProbe {
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
            let argument = input
                .argument("object-version")
                .expect("fixture image argument");
            let version = argument.as_str().expect("fixture image version");
            let source = StoredMedia::new("s3", "versions/image.png", version)
                .map_err(ConsoleError::from_source)?;
            let result = configured_image(self.1, ImageInput::stored(source))
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

struct TencentProbe(CommandDefinition);

impl Command for TencentProbe {
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
            let text = text.as_str().expect("fixture text");
            let result = if input.option("image").and_then(|value| value.as_bool()) == Some(true) {
                let source = StoredMedia::new("s3", "versions/image.png", text)
                    .map_err(ConsoleError::from_source)?;
                configured_image("tencent-images", ImageInput::stored(source)).await
            } else {
                tencent(text, None, "tencent-default").await
            }
            .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
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
            let text = text.as_str().expect("fixture text");
            let result = if input
                .option("image")
                .is_some_and(|value| value.as_bool() == Some(true))
            {
                aliyun_image(text, "s3").await
            } else {
                aliyun(text, None).await
            }
            .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!("{result}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
