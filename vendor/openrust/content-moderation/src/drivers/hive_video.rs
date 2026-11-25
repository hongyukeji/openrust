use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use http::Method;
use openrust::{filesystem::FilesystemManager, http::client::Factory, support::Str};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use url::Url;

use super::{
    Endpoint, field,
    hive::{Model, ModelResult, configured_set, provider_class, severity_threshold},
    hive_image::{Frame, category},
    hive_ocr::Frame as OcrFrame,
    hive_video_severity::{SeverityEvidence, SpeechChunk},
    http_status,
    media::VersionedMediaLinks,
    video::{VideoEvidence, build_assessment, seconds_to_ms, unknown_submission},
};
use crate::{
    Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError,
    ModerationResult, Operation, TaskId, TaskStatus, VideoInput, credentials::DriverCredentials,
};

const PAGE_SIZE: usize = 64;
const MAX_PAGES: usize = 4;

pub(super) struct VideoService {
    model: Model,
    storage: VersionedMediaLinks,
    callback: Url,
    summary: Option<Summary>,
}

impl VideoService {
    pub(super) fn configured(
        http: Arc<Factory>,
        credentials: &DriverCredentials<'_>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("video_project").is_none() {
            return Ok(None);
        }
        let model_type = field(profile, "video_model_type")?.unwrap_or("CATEGORIZATION");
        if !matches!(
            model_type,
            "CATEGORIZATION" | "OCR_MODERATION" | "TRANSCRIPTION"
        ) {
            return Err(ModerationError::InvalidConfiguration("video_model_type"));
        }
        let mut model = Model::new(
            http,
            credentials,
            profile,
            "video_token_env",
            "video_project",
            model_type,
        )?;
        model.endpoint.url.set_path("/api/v2/task/async");
        let callback = super::video::callback_url(profile)?
            .ok_or(ModerationError::InvalidConfiguration("video_callback_url"))?;
        let summary = Summary::configured(&model.endpoint, model_type, profile)?;
        Ok(Some(Self {
            model,
            storage: VersionedMediaLinks::new(storage, profile)?,
            callback,
            summary,
        }))
    }

    pub(super) async fn submit(&self, input: &VideoInput) -> ModerationResult<TaskId> {
        let media = self.storage.create(input.source()).await?;
        let payload = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", media.as_str())
            .append_pair("callback_url", self.callback.as_str())
            .finish();
        let response = self
            .model
            .endpoint
            .post_encoded(
                self.model.headers(),
                payload,
                "application/x-www-form-urlencoded",
            )
            .await
            .map_err(unknown_submission)?;
        submission_status(response.status().as_u16())?;
        let task: Acknowledgement = response
            .json()
            .map_err(|_| ModerationError::SubmissionUnknown)?;
        if let Some(code) = task.code {
            submission_status(code)?;
        }
        if task.project_id != Some(self.model.project)
            || task
                .callback
                .as_ref()
                .is_some_and(|callback| callback != self.callback.as_str())
        {
            return Err(ModerationError::SubmissionUnknown);
        }
        let id = task
            .id
            .filter(|id| valid_task(id))
            .ok_or(ModerationError::SubmissionUnknown)?;
        TaskId::new(id).map_err(|_| ModerationError::SubmissionUnknown)
    }

    pub(super) fn can_query(&self) -> bool {
        self.summary.is_some()
    }

    pub(super) async fn query(&self, id: &TaskId) -> ModerationResult<TaskStatus> {
        let summary = self
            .summary
            .as_ref()
            .ok_or(ModerationError::Unsupported(Operation::QueryVideo))?;
        if !valid_task(id.as_str()) {
            return Err(ModerationError::InvalidInput("hive video task"));
        }
        summary.query(&self.model, id).await
    }
}

#[derive(Deserialize)]
struct Acknowledgement {
    id: Option<String>,
    project_id: Option<u64>,
    callback: Option<String>,
    code: Option<u16>,
}

fn submission_status(code: u16) -> ModerationResult<()> {
    http_status(code).map_err(unknown_submission)
}

pub(crate) fn valid_task(id: &str) -> bool {
    Str::is_uuid(id, None) && !Str::is_uuid(id, Some(0))
}

struct Summary {
    endpoint: Endpoint,
    classes: BTreeSet<String>,
    policy: SummaryPolicy,
}

#[derive(Clone, Copy)]
enum SummaryPolicy {
    Visual(f64),
    Ocr(i8),
    Speech(i8),
}

#[derive(Deserialize)]
struct Output<T> {
    output: Vec<T>,
}

impl Summary {
    fn configured(
        endpoint: &Endpoint,
        model_type: &str,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        let Some(api) = field(profile, "video_query_api")? else {
            return Ok(None);
        };
        if api != "v1-task-summary" {
            return Err(ModerationError::InvalidConfiguration("video_query_api"));
        }
        let classes = configured_set(profile, "video_classes", provider_class)?;
        let policy = match model_type {
            "CATEGORIZATION" => SummaryPolicy::Visual(
                profile
                    .get("video_review_score")
                    .and_then(Value::as_f64)
                    .filter(|score| (0.0..=1.0).contains(score))
                    .ok_or(ModerationError::InvalidConfiguration("video_review_score"))?,
            ),
            "OCR_MODERATION" => {
                SummaryPolicy::Ocr(severity_threshold(profile, "video_review_severity")?)
            }
            "TRANSCRIPTION" => {
                SummaryPolicy::Speech(severity_threshold(profile, "video_review_severity")?)
            }
            _ => return Err(ModerationError::InvalidConfiguration("video_model_type")),
        };
        let mut endpoint = endpoint.clone();
        endpoint.url.set_path("/api/v1/task");
        // 四页查询的 HTTP 等待总时长须落在单次审核租约内。
        endpoint.timeout = endpoint.timeout.min(Duration::from_secs(30));
        Ok(Some(Self {
            endpoint,
            classes,
            policy,
        }))
    }

    async fn page<T: DeserializeOwned>(
        &self,
        model: &Model,
        id: &TaskId,
        offset: usize,
    ) -> ModerationResult<Option<Vec<T>>> {
        let response = self
            .endpoint
            .send(Method::GET, model.headers(), |request| {
                request.with_query_parameters([
                    ("task_id".to_owned(), id.as_str().to_owned()),
                    ("frame_offset".to_owned(), offset.to_string()),
                    ("frame_limit".to_owned(), PAGE_SIZE.to_string()),
                ])
            })
            .await?;
        let result: Option<ModelResult<Output<T>>> = model.validate_summary(response, id)?;
        Ok(result.map(|result| result.output.output))
    }

    async fn read_pages<T: DeserializeOwned>(
        &self,
        model: &Model,
        id: &TaskId,
        mut read: impl FnMut(T) -> ModerationResult<()>,
    ) -> ModerationResult<bool> {
        for page in 0..MAX_PAGES {
            let Some(frames) = self.page::<T>(model, id, page * PAGE_SIZE).await? else {
                return if page == 0 {
                    Ok(false)
                } else {
                    Err(ModerationError::Protocol)
                };
            };
            if frames.len() > PAGE_SIZE {
                return Err(ModerationError::Protocol);
            }
            let count = frames.len();
            for frame in frames {
                read(frame)?;
            }
            if count < PAGE_SIZE {
                break;
            }
        }
        Ok(true)
    }

    async fn query(&self, model: &Model, id: &TaskId) -> ModerationResult<TaskStatus> {
        let (threshold, modality) = match self.policy {
            SummaryPolicy::Visual(score) => return self.visual(model, id, score).await,
            SummaryPolicy::Ocr(threshold) => (threshold, Modality::Ocr),
            SummaryPolicy::Speech(threshold) => (threshold, Modality::Speech),
        };
        let mut severity = SeverityEvidence::new(&self.classes, threshold, modality);
        let complete = if modality == Modality::Ocr {
            self.read_pages(model, id, |frame: OcrFrame| severity.ocr(frame))
                .await?
        } else {
            self.read_pages(model, id, |chunk: SpeechChunk| severity.speech(chunk))
                .await?
        };
        if complete {
            severity.finish(id)
        } else {
            Ok(TaskStatus::Running)
        }
    }

    async fn visual(
        &self,
        model: &Model,
        id: &TaskId,
        review_score: f64,
    ) -> ModerationResult<TaskStatus> {
        let mut maximum = BTreeMap::new();
        let mut last_time = None;
        if !self
            .read_pages(model, id, |frame: Frame| {
                self.read_frame(&frame, &mut last_time, &mut maximum)
            })
            .await?
        {
            return Ok(TaskStatus::Running);
        }
        let coverage = if maximum.is_empty() {
            Coverage::Unavailable
        } else {
            Coverage::Partial
        };
        let evidence = maximum
            .into_iter()
            .filter(|(_, (score, _))| *score >= review_score)
            .map(|(class, (score, millis))| {
                Ok(VideoEvidence {
                    finding: Finding {
                        modality: Modality::Visual,
                        category: category(class).to_owned(),
                        provider_label: class.to_owned(),
                    },
                    span: Some(EvidenceSpan::new(millis, millis)?),
                    score: Some(Confidence::new(score, 1.0)?),
                })
            })
            .collect::<ModerationResult<Vec<_>>>()?;
        let decision = if evidence.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        Ok(TaskStatus::Succeeded(build_assessment(
            decision,
            BTreeMap::from([(Modality::Visual, coverage)]),
            Some(id.as_str().to_owned()),
            evidence,
        )?))
    }

    fn read_frame<'a>(
        &'a self,
        frame: &Frame,
        last: &mut Option<u64>,
        maximum: &mut BTreeMap<&'a str, (f64, u64)>,
    ) -> ModerationResult<()> {
        let time = seconds_to_ms(frame.time.ok_or(ModerationError::Protocol)?)?;
        if last.is_some_and(|previous| time < previous) {
            return Err(ModerationError::Protocol);
        }
        *last = Some(time);
        let mut seen = BTreeSet::new();
        for prediction in &frame.classes {
            if !(0.0..=1.0).contains(&prediction.score) || !seen.insert(prediction.class.as_str()) {
                return Err(ModerationError::Protocol);
            }
            if let Some(class) = self.classes.get(&prediction.class) {
                let peak = maximum.entry(class).or_insert((prediction.score, time));
                if prediction.score > peak.0 {
                    *peak = (prediction.score, time);
                }
            }
        }
        Ok(())
    }
}
