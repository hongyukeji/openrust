use std::{collections::BTreeMap, sync::Arc};

use http::{HeaderMap, Method};
use openrust::{filesystem::FilesystemManager, http::client::Factory};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Endpoint, http_status,
    media::VersionedMediaLinks,
    sightengine::endpoint,
    sightengine_models::{MediaFindings, MediaModels},
    video::{build_assessment, seconds_to_ms, unknown_submission},
};
use crate::{
    Decision, EvidenceSpan, ModerationError, ModerationResult, TaskId, TaskStatus, VideoInput,
};

const FORM: &str = "application/x-www-form-urlencoded";

pub(super) struct VideoService {
    submit_endpoint: Endpoint,
    job_endpoint: Endpoint,
    storage: VersionedMediaLinks,
    models: MediaModels,
    max_duration: u64,
}

impl VideoService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("video_models").is_none() {
            return Ok(None);
        }
        let max_duration = match profile.get("video_max_duration") {
            None => 1800,
            Some(value) => value
                .as_u64()
                .filter(|value| (1..=3600).contains(value))
                .ok_or(ModerationError::InvalidConfiguration("video_max_duration"))?,
        };
        Ok(Some(Self {
            submit_endpoint: endpoint(http.clone(), profile, "/1.0/video/check.json")?,
            job_endpoint: endpoint(http, profile, "/1.0/video/byid.json")?,
            storage: VersionedMediaLinks::new(storage, profile)?,
            models: MediaModels::new(
                profile,
                "video_models",
                "video_review_score",
                "video_text_categories",
            )?,
            max_duration,
        }))
    }

    pub(super) async fn submit(
        &self,
        user: &str,
        secret: &str,
        input: &VideoInput,
    ) -> ModerationResult<TaskId> {
        let url = self.storage.create(input.source()).await?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(self.models.parameters())
            .extend_pairs([
                ("api_user", user),
                ("api_secret", secret),
                ("stream_url", url.as_str()),
                ("max_duration", &self.max_duration.to_string()),
            ])
            .finish();
        let response = self
            .submit_endpoint
            .post_encoded(HeaderMap::new(), body, FORM)
            .await
            .map_err(unknown_submission)?;
        http_status(response.status().as_u16()).map_err(unknown_submission)?;
        let result: Submission = response
            .json()
            .map_err(|_| ModerationError::SubmissionUnknown)?;
        if result.status != "success" || !media_id(&result.media.id) {
            return Err(ModerationError::SubmissionUnknown);
        }
        TaskId::new(result.media.id).map_err(|_| ModerationError::SubmissionUnknown)
    }

    pub(super) async fn query(
        &self,
        user: &str,
        secret: &str,
        id: &TaskId,
    ) -> ModerationResult<TaskStatus> {
        let fields = job_fields(user, secret, id)?;
        // GET 的 query 只在线路上传送。共享 Endpoint 会令事件快照省略整个查询串。
        let response = self
            .job_endpoint
            .get_fields(
                fields
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value.to_owned())),
            )
            .await?;
        http_status(response.status().as_u16())?;
        let result: Query = response.json().map_err(|_| ModerationError::Protocol)?;
        if result.status != "success" || result.output.media.id != id.as_str() {
            return Err(ModerationError::Protocol);
        }
        let output = result.output.data;
        if output
            .progress
            .is_some_and(|progress| !(0.0..=1.0).contains(&progress))
        {
            return Err(ModerationError::Protocol);
        }
        match output.status.as_str() {
            "ongoing" => Ok(TaskStatus::Running),
            "stopped" => Ok(TaskStatus::Cancelled),
            "failure" => Ok(TaskStatus::Failed),
            "finished" => {
                let mut evidence = Vec::new();
                let mut coverage = BTreeMap::new();
                for frame in output.frames.unwrap_or_default() {
                    let offset = seconds_to_ms(frame.info.position)?;
                    let span = EvidenceSpan::new(offset, offset)?;
                    let mut findings = MediaFindings::default();
                    self.models.observe(&frame.results, &mut findings)?;
                    coverage.extend(findings.coverage());
                    evidence.extend(findings.evidence(Some(span))?);
                }
                let decision = if evidence.is_empty() {
                    Decision::Allow
                } else {
                    Decision::Review
                };
                Ok(TaskStatus::Succeeded(build_assessment(
                    decision,
                    coverage,
                    Some(result.request.id),
                    evidence,
                )?))
            }
            _ => Err(ModerationError::Protocol),
        }
    }

    pub(super) async fn cancel(
        &self,
        user: &str,
        secret: &str,
        id: &TaskId,
    ) -> ModerationResult<()> {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(job_fields(user, secret, id)?)
            .finish();
        let response = self
            .job_endpoint
            .send_encoded(Method::DELETE, HeaderMap::new(), body, FORM)
            .await?;
        http_status(response.status().as_u16())?;
        let result: Cancel = response.json().map_err(|_| ModerationError::Protocol)?;
        if result.status != "success" {
            return Err(ModerationError::Protocol);
        }
        TaskId::new(result.request.id).map_err(|_| ModerationError::Protocol)?;
        Ok(())
    }
}

fn job_fields<'a>(
    user: &'a str,
    secret: &'a str,
    id: &'a TaskId,
) -> ModerationResult<[(&'static str, &'a str); 3]> {
    if !media_id(id.as_str()) {
        return Err(ModerationError::InvalidInput("sightengine media ID"));
    }
    Ok([
        ("id", id.as_str()),
        ("api_user", user),
        ("api_secret", secret),
    ])
}

pub(crate) fn media_id(value: &str) -> bool {
    value.strip_prefix("med_").is_some_and(|token| {
        !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

#[derive(Deserialize)]
struct RequestMetadata {
    id: String,
}

#[derive(Deserialize)]
struct Submission {
    status: String,
    media: Media,
}

#[derive(Deserialize)]
struct Media {
    id: String,
}

#[derive(Deserialize)]
struct Query {
    status: String,
    request: RequestMetadata,
    output: QueryOutput,
}

#[derive(Deserialize)]
struct QueryOutput {
    media: Media,
    data: Job,
}

#[derive(Deserialize)]
struct Job {
    status: String,
    progress: Option<f64>,
    frames: Option<Vec<Frame>>,
}

#[derive(Deserialize)]
struct Frame {
    info: FrameInfo,
    #[serde(flatten)]
    results: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct FrameInfo {
    position: f64,
}

#[derive(Deserialize)]
struct Cancel {
    status: String,
    request: RequestMetadata,
}
