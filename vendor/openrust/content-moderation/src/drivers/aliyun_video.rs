use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use openrust::filesystem::FilesystemManager;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use zeroize::Zeroizing;

use super::{
    aliyun::{Aliyun, category, image_category},
    field,
    media::VersionedMediaLinks,
    video::{VideoEvidence, build_assessment, seconds_to_ms, unknown_submission},
};
use crate::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError,
    ModerationResult, TaskId, TaskStatus, VideoInput, credentials::DriverCredentials,
};

pub(super) struct VideoService {
    name: String,
    media: VersionedMediaLinks,
    callback: Option<(Url, Zeroizing<String>)>,
}

pub(crate) fn valid_callback_seed(seed: &str) -> bool {
    (16..=64).contains(&seed.len())
        && seed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl VideoService {
    pub(super) fn configured(
        credentials: &DriverCredentials<'_>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
        region: &str,
    ) -> ModerationResult<Option<Self>> {
        let Some(name) = field(profile, "video_service")? else {
            return Ok(None);
        };
        if !matches!(
            (name, region),
            (
                "videoDetection",
                "cn-shanghai" | "cn-beijing" | "cn-hangzhou"
            ) | ("videoDetectionByVL", "cn-shanghai" | "cn-beijing")
        ) {
            return Err(ModerationError::InvalidConfiguration("video_service"));
        }
        Ok(Some(Self {
            name: name.to_owned(),
            media: VersionedMediaLinks::new(storage, profile)?,
            callback: super::video::signed_callback(profile, credentials, valid_callback_seed)?,
        }))
    }

    pub(super) async fn submit(
        &self,
        driver: &Aliyun,
        input: &VideoInput,
    ) -> ModerationResult<TaskId> {
        let url = self.media.create(input.source()).await?;
        if url.as_str().len() > 2048 {
            return Err(ModerationError::InvalidInput("aliyun video URL length"));
        }
        let mut parameters = json!({"url":url.as_str()});
        if let Some((url, seed)) = &self.callback {
            parameters["callback"] = json!(url.as_str());
            parameters["seed"] = json!(seed.as_str());
            parameters["cryptType"] = json!("SHA256");
        }
        let output: Output<Submission> = driver
            .rpc("VideoModeration", &self.name, parameters)
            .await
            .map_err(unknown_submission)?;
        match output.code.number().map_err(unknown_submission)? {
            200 | 288 => {
                let data = output.data.ok_or(ModerationError::SubmissionUnknown)?;
                TaskId::new(data.task_id).map_err(|_| ModerationError::SubmissionUnknown)
            }
            code => Err(unknown_submission(business_failure(code))),
        }
    }

    pub(super) async fn query(&self, driver: &Aliyun, id: &TaskId) -> ModerationResult<TaskStatus> {
        let output: Output<Query> = driver
            .rpc(
                "VideoModerationResult",
                &self.name,
                json!({"taskId":id.as_str()}),
            )
            .await?;
        if output
            .data
            .as_ref()
            .and_then(|data| data.task_id.as_deref())
            .is_some_and(|returned| returned != id.as_str())
        {
            return Err(ModerationError::Protocol);
        }
        match output.code.number()? {
            200 => {
                let data = output.data.ok_or(ModerationError::Protocol)?;
                if data.task_id.as_deref() != Some(id.as_str()) {
                    return Err(ModerationError::Protocol);
                }
                Ok(TaskStatus::Succeeded(assessment(output.request_id, data)?))
            }
            280 => Ok(TaskStatus::Running),
            288 => Ok(TaskStatus::Pending),
            404..=407 => Ok(TaskStatus::Failed),
            409 => Ok(TaskStatus::Expired),
            code => Err(business_failure(code)),
        }
    }
}

fn business_failure(code: u16) -> ModerationError {
    match code {
        400..=402 | 404..=407 | 409 => ModerationError::Rejected,
        403 | 480 | 588 => ModerationError::Throttled,
        408 => ModerationError::Unauthorized,
        500..=599 => ModerationError::ServiceUnavailable,
        _ => ModerationError::Protocol,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Output<T> {
    code: Code,
    data: Option<T>,
    request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Code {
    Number(u16),
    Text(String),
}

impl Code {
    pub(crate) fn number(self) -> ModerationResult<u16> {
        match self {
            Self::Number(code) => Ok(code),
            Self::Text(code)
                if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                code.parse().map_err(|_| ModerationError::Protocol)
            }
            _ => Err(ModerationError::Protocol),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Submission {
    task_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Query {
    task_id: Option<String>,
    risk_level: Option<String>,
    frame_result: Option<Visual>,
    audio_result: Option<Audio>,
    manual_task_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Visual {
    risk_level: Option<String>,
    frame_num: Option<u64>,
    frame_summarys: Option<Vec<Summary>>,
    frames: Option<Vec<Frame>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Audio {
    risk_level: Option<String>,
    audio_summarys: Option<Vec<Summary>>,
    slice_details: Option<Vec<Slice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Summary {
    label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Frame {
    offset: f64,
    risk_level: Option<String>,
    results: Vec<FrameService>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FrameService {
    result: Vec<FrameLabel>,
    text_in_image: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FrameLabel {
    label: String,
    confidence: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Slice {
    start_time: f64,
    end_time: f64,
    labels: String,
    risk_level: Option<String>,
}

fn assessment(request: Option<String>, data: Query) -> ModerationResult<Assessment> {
    let overall = data
        .risk_level
        .as_deref()
        .ok_or(ModerationError::Protocol)?;
    let mut decision = risk(Some(overall), Decision::Allow)?;
    if data.manual_task_id.is_some() && decision == Decision::Allow {
        decision = Decision::Review;
    }
    let mut coverage = BTreeMap::new();
    let mut evidence = Vec::new();
    if let Some(visual) = data.frame_result {
        read_visual(visual, &mut coverage, &mut evidence, &mut decision)?;
    }
    if let Some(audio) = data.audio_result {
        read_audio(audio, &mut coverage, &mut evidence, &mut decision)?;
    }
    if decision == Decision::Allow && !evidence.is_empty() {
        decision = Decision::Review;
    }
    build_assessment(decision, coverage, request, evidence)
}

fn read_visual(
    visual: Visual,
    coverage: &mut BTreeMap<Modality, Coverage>,
    evidence: &mut Vec<VideoEvidence>,
    decision: &mut Decision,
) -> ModerationResult<()> {
    let frames = visual.frames.unwrap_or_default();
    let summaries = visual.frame_summarys.unwrap_or_default();
    if visual.frame_num.is_some_and(|count| count > 0)
        || !frames.is_empty()
        || !summaries.is_empty()
    {
        coverage.insert(Modality::Visual, Coverage::Partial);
    }
    *decision = risk(visual.risk_level.as_deref(), *decision)?;
    let mut seen = BTreeSet::new();
    for frame in frames {
        let offset = seconds_to_ms(frame.offset)?;
        let span = EvidenceSpan::new(offset, offset)?;
        *decision = risk(frame.risk_level.as_deref(), *decision)?;
        for service in frame.results {
            if service.text_in_image.is_some() {
                coverage.insert(Modality::Ocr, Coverage::Partial);
            }
            for label in service.result {
                let score = label
                    .confidence
                    .map(|value| Confidence::new(value, 100.0))
                    .transpose()
                    .map_err(|_| ModerationError::Protocol)?;
                if let Some(entry) = finding(Modality::Visual, &label.label, Some(span), score)? {
                    seen.insert(label.label);
                    evidence.push(entry);
                }
            }
        }
    }
    for summary in summaries {
        if !seen.contains(&summary.label) {
            evidence.extend(finding(Modality::Visual, &summary.label, None, None)?);
        }
    }
    Ok(())
}

fn read_audio(
    audio: Audio,
    coverage: &mut BTreeMap<Modality, Coverage>,
    evidence: &mut Vec<VideoEvidence>,
    decision: &mut Decision,
) -> ModerationResult<()> {
    let slices = audio.slice_details.unwrap_or_default();
    let summaries = audio.audio_summarys.unwrap_or_default();
    if !slices.is_empty() || !summaries.is_empty() {
        coverage.insert(Modality::Speech, Coverage::Partial);
    }
    *decision = risk(audio.risk_level.as_deref(), *decision)?;
    let mut seen = BTreeSet::new();
    for slice in slices {
        let span = EvidenceSpan::new(
            seconds_to_ms(slice.start_time)?,
            seconds_to_ms(slice.end_time)?,
        )
        .map_err(|_| ModerationError::Protocol)?;
        *decision = risk(slice.risk_level.as_deref(), *decision)?;
        for label in slice
            .labels
            .split(',')
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            if let Some(entry) = finding(Modality::Speech, label, Some(span), None)? {
                seen.insert(label.to_owned());
                evidence.push(entry);
            }
        }
    }
    for summary in summaries {
        if !seen.contains(&summary.label) {
            evidence.extend(finding(Modality::Speech, &summary.label, None, None)?);
        }
    }
    Ok(())
}

fn finding(
    modality: Modality,
    label: &str,
    span: Option<EvidenceSpan>,
    score: Option<Confidence>,
) -> ModerationResult<Option<VideoEvidence>> {
    if !label
        .bytes()
        .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-'))
        || label.is_empty()
        || label.len() > 128
    {
        return Err(ModerationError::Protocol);
    }
    if label == "nonLabel" {
        return Ok(None);
    }
    let category = if modality == Modality::Visual {
        image_category(label)
    } else {
        category(label)
    };
    Ok(Some(VideoEvidence {
        finding: Finding {
            modality,
            category: category.to_owned(),
            provider_label: label.to_owned(),
        },
        span,
        score,
    }))
}

fn risk(value: Option<&str>, existing: Decision) -> ModerationResult<Decision> {
    match (value, existing) {
        (Some("high"), _) => Ok(Decision::Block),
        (Some("medium" | "low"), Decision::Allow) => Ok(Decision::Review),
        (None | Some("none" | "medium" | "low"), decision) => Ok(decision),
        _ => Err(ModerationError::Protocol),
    }
}
