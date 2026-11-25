use std::{collections::BTreeMap, sync::Arc};

use openrust::{filesystem::FilesystemManager, http::client::Factory, support::Str};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{
    Endpoint, field,
    media::VersionedMediaLinks,
    tencent::{Tencent, Verdict, rpc_endpoint, service_error},
    video::{VideoEvidence, build_assessment, seconds_to_ms, unknown_submission},
};
use crate::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Modality, ModerationError,
    ModerationResult, TaskId, TaskStatus, VideoInput,
};

pub(super) struct VideoService {
    endpoint: Endpoint,
    biz_type: String,
    storage: VersionedMediaLinks,
}

impl VideoService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        let Some(biz_type) = field(profile, "video_biz_type")? else {
            return Ok(None);
        };
        if !(3..=32).contains(&biz_type.len())
            || !biz_type
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || value == b'_')
        {
            return Err(ModerationError::InvalidConfiguration("video_biz_type"));
        }
        let endpoint_profile = json!({
            "base_url": field(profile, "video_base_url")?,
            "timeout_ms": profile.get("timeout_ms").cloned().unwrap_or(json!(10_000)),
        });
        Ok(Some(Self {
            endpoint: rpc_endpoint(http, &endpoint_profile, "https://vm.tencentcloudapi.com")?,
            biz_type: biz_type.to_owned(),
            storage: VersionedMediaLinks::new(storage, profile)?,
        }))
    }

    pub(super) async fn submit(
        &self,
        driver: &Tencent,
        input: &VideoInput,
    ) -> ModerationResult<TaskId> {
        let url = self.storage.create(input.source()).await?;
        let reference = Str::uuid()
            .map_err(|_| ModerationError::Unavailable)?
            .to_string();
        let result: Created = self
            .call(
                driver,
                "CreateVideoModerationTask",
                json!({"BizType":self.biz_type,"Type":"VIDEO","Tasks":[{
                    "DataId":reference,"Input":{"Type":"URL","Url":url.as_str()},
                }]}),
            )
            .await
            .map_err(unknown_submission)?;
        let [result] = result.results.as_slice() else {
            return Err(ModerationError::SubmissionUnknown);
        };
        if result.data_id.as_deref() != Some(&reference) {
            return Err(ModerationError::SubmissionUnknown);
        }
        if result.code != "OK" {
            return Err(unknown_submission(service_error(&result.code)));
        }
        TaskId::new(result.task_id.as_deref().unwrap_or_default())
            .map_err(|_| ModerationError::SubmissionUnknown)
    }

    pub(super) async fn query(
        &self,
        driver: &Tencent,
        id: &TaskId,
    ) -> ModerationResult<TaskStatus> {
        let details = self.details(driver, id, true).await?;
        match details.status.as_str() {
            "PENDING" => Ok(TaskStatus::Pending),
            "RUNNING" => Ok(TaskStatus::Running),
            "ERROR" => Ok(TaskStatus::Failed),
            "CANCELLED" => Ok(TaskStatus::Cancelled),
            "FINISH" => Ok(TaskStatus::Succeeded(assess(details)?)),
            _ => Err(ModerationError::Protocol),
        }
    }

    pub(super) async fn cancel(&self, driver: &Tencent, id: &TaskId) -> ModerationResult<()> {
        let details = self.details(driver, id, false).await?;
        match details.status.as_str() {
            "CANCELLED" => return Ok(()),
            "PENDING" => {}
            "RUNNING" | "FINISH" | "ERROR" => return Err(ModerationError::Rejected),
            _ => return Err(ModerationError::Protocol),
        }
        let result: Cancelled = self
            .call(driver, "CancelTask", json!({"TaskId":id.as_str()}))
            .await?;
        if result
            .task_id
            .as_deref()
            .is_some_and(|task| task != id.as_str())
        {
            return Err(ModerationError::Protocol);
        }
        TaskId::new(result.request_id).map_err(|_| ModerationError::Protocol)?;
        Ok(())
    }

    async fn details(
        &self,
        driver: &Tencent,
        id: &TaskId,
        show_segments: bool,
    ) -> ModerationResult<Details> {
        let result: Details = self
            .call(
                driver,
                "DescribeTaskDetail",
                json!({"TaskId":id.as_str(), "ShowAllSegments":show_segments}),
            )
            .await?;
        if result.task_id != id.as_str()
            || result.biz_type != self.biz_type
            || result.kind != "VIDEO"
        {
            return Err(ModerationError::Protocol);
        }
        Ok(result)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        driver: &Tencent,
        action: &'static str,
        parameters: Value,
    ) -> ModerationResult<T> {
        driver
            .request(&self.endpoint, "vm", action, "2021-09-22", parameters)
            .await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Created {
    results: Vec<CreatedTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreatedTask {
    code: String,
    task_id: Option<String>,
    data_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Cancelled {
    request_id: String,
    task_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Details {
    task_id: String,
    biz_type: String,
    #[serde(rename = "Type")]
    kind: String,
    status: String,
    suggestion: Option<String>,
    labels: Option<Vec<Verdict>>,
    label: Option<String>,
    image_segments: Option<Vec<Segment<ImageResult>>>,
    audio_segments: Option<Vec<Segment<AudioResult>>>,
    request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Segment<T> {
    offset_time: String,
    result: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageResult {
    #[serde(flatten)]
    verdict: Verdict,
    results: Option<Vec<ImageItem>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageItem {
    #[serde(flatten)]
    verdict: Verdict,
    hit_type: Option<String>,
    details: Option<Vec<ImageDetail>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageDetail {
    #[serde(flatten)]
    verdict: Verdict,
    ocr_hit_infos: Option<Vec<serde::de::IgnoredAny>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AudioResult {
    #[serde(flatten)]
    verdict: Verdict,
    duration: Option<String>,
    text_results: Option<Vec<Verdict>>,
    moan_results: Option<Vec<Moan>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Moan {
    #[serde(flatten)]
    verdict: Verdict,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

fn assess(details: Details) -> ModerationResult<Assessment> {
    let mut decision = match details.suggestion.as_deref() {
        Some("Pass")
            if details
                .label
                .as_deref()
                .is_none_or(|value| value == "Normal" || value.is_empty()) =>
        {
            Decision::Allow
        }
        Some("Pass" | "Review") => Decision::Review,
        Some("Block") => Decision::Block,
        _ => return Err(ModerationError::Protocol),
    };
    for label in details.labels.unwrap_or_default() {
        decision = strongest(decision, label.decision()?);
    }

    let mut coverage = BTreeMap::new();
    let mut evidence = Vec::new();
    for frame in details.image_segments.unwrap_or_default() {
        let at = offset(&frame.offset_time)?;
        let span = EvidenceSpan::new(at, at)?;
        coverage.insert(Modality::Visual, Coverage::Partial);
        record(
            frame.result.verdict,
            Modality::Visual,
            span,
            &mut decision,
            &mut evidence,
        )?;
        for result in frame.result.results.unwrap_or_default() {
            let is_ocr = result.hit_type.as_deref() == Some("text_rule");
            let modality = if is_ocr {
                Modality::Ocr
            } else {
                Modality::Visual
            };
            coverage.insert(modality, Coverage::Partial);
            record(result.verdict, modality, span, &mut decision, &mut evidence)?;
            for detail in result.details.unwrap_or_default() {
                let modality = if detail.ocr_hit_infos.is_some_and(|hits| !hits.is_empty()) {
                    Modality::Ocr
                } else {
                    modality
                };
                coverage.insert(modality, Coverage::Partial);
                record(detail.verdict, modality, span, &mut decision, &mut evidence)?;
            }
        }
    }

    for section in details.audio_segments.unwrap_or_default() {
        let start = offset(&section.offset_time)?;
        let duration: u64 = section
            .result
            .duration
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(str::parse)
            .transpose()
            .map_err(|_| ModerationError::Protocol)?
            .unwrap_or(0);
        let end = start
            .checked_add(duration)
            .ok_or(ModerationError::Protocol)?;
        let span = EvidenceSpan::new(start, end)?;
        coverage.insert(Modality::Speech, Coverage::Partial);
        record(
            section.result.verdict,
            Modality::Speech,
            span,
            &mut decision,
            &mut evidence,
        )?;
        for result in section.result.text_results.unwrap_or_default() {
            record(result, Modality::Speech, span, &mut decision, &mut evidence)?;
        }
        for result in section.result.moan_results.unwrap_or_default() {
            let evidence_span = match (result.start_time, result.end_time) {
                (None, None) => span,
                (Some(from), Some(to)) => {
                    let from = start
                        .checked_add(millis(from)?)
                        .ok_or(ModerationError::Protocol)?;
                    let to = start
                        .checked_add(millis(to)?)
                        .ok_or(ModerationError::Protocol)?;
                    EvidenceSpan::new(from, to).map_err(|_| ModerationError::Protocol)?
                }
                _ => return Err(ModerationError::Protocol),
            };
            record(
                result.verdict,
                Modality::Speech,
                evidence_span,
                &mut decision,
                &mut evidence,
            )?;
        }
    }

    build_assessment(decision, coverage, details.request_id, evidence)
}

fn record(
    verdict: Verdict,
    modality: Modality,
    span: EvidenceSpan,
    decision: &mut Decision,
    results: &mut Vec<VideoEvidence>,
) -> ModerationResult<()> {
    let suggestion = verdict.decision()?;
    *decision = strongest(*decision, suggestion);
    if suggestion != Decision::Allow {
        let (finding, score) = verdict.finding(modality);
        if finding.provider_label.len() > 128
            || !finding
                .provider_label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/'))
        {
            return Err(ModerationError::Protocol);
        }
        let score = score
            .map(|value| Confidence::new(value, 100.0))
            .transpose()?;
        results.push(VideoEvidence {
            finding,
            span: Some(span),
            score,
        });
    }
    Ok(())
}

fn strongest(left: Decision, right: Decision) -> Decision {
    match (left, right) {
        (Decision::Block, _) | (_, Decision::Block) => Decision::Block,
        (Decision::Review, _) | (_, Decision::Review) => Decision::Review,
        _ => Decision::Allow,
    }
}

fn offset(raw: &str) -> ModerationResult<u64> {
    let seconds: f64 = raw.parse().map_err(|_| ModerationError::Protocol)?;
    seconds_to_ms(seconds)
}

fn millis(value: f64) -> ModerationResult<u64> {
    if (0.0..=f64::from(u32::MAX)).contains(&value) {
        Ok(value.round() as u64)
    } else {
        Err(ModerationError::Protocol)
    }
}
