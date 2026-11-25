use std::collections::BTreeMap;

use serde_json::Value;
use url::Url;

use crate::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError,
    ModerationResult,
};

pub(super) struct VideoEvidence {
    pub(super) finding: Finding,
    pub(super) span: Option<EvidenceSpan>,
    pub(super) score: Option<Confidence>,
}

pub(super) fn callback_url(profile: &Value) -> ModerationResult<Option<Url>> {
    super::field(profile, "video_callback_url")?
        .map(|value| {
            (value.len() <= 2048)
                .then(|| Url::parse(value).ok())
                .flatten()
                .filter(|url| {
                    url.scheme() == "https"
                        && url.host().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.fragment().is_none()
                        && url.query().is_none()
                })
                .ok_or(ModerationError::InvalidConfiguration("video_callback_url"))
        })
        .transpose()
}

pub(super) fn build_assessment(
    decision: Decision,
    coverage: BTreeMap<Modality, Coverage>,
    request: Option<String>,
    evidence: Vec<VideoEvidence>,
) -> ModerationResult<Assessment> {
    let (findings, measurements): (Vec<_>, Vec<_>) = evidence
        .into_iter()
        .map(|entry| (entry.finding, (entry.span, entry.score)))
        .unzip();
    let mut result = Assessment::new(decision, coverage, findings)
        .with_request_id(request.ok_or(ModerationError::Protocol)?)
        .map_err(|_| ModerationError::Protocol)?;
    for (index, (span, score)) in measurements.into_iter().enumerate() {
        if let Some(span) = span {
            result = result.with_evidence_span(index, span)?;
        }
        if let Some(score) = score {
            result = result.with_confidence(index, score)?;
        }
    }
    Ok(result)
}

pub(super) fn seconds_to_ms(seconds: f64) -> ModerationResult<u64> {
    let millis = (seconds * 1000.0).round();
    if (0.0..=f64::from(u32::MAX)).contains(&millis) && seconds >= 0.0 {
        Ok(millis as u64)
    } else {
        Err(ModerationError::Protocol)
    }
}

pub(super) fn unknown_submission(error: ModerationError) -> ModerationError {
    match error {
        ModerationError::Protocol
        | ModerationError::ServiceUnavailable
        | ModerationError::Timeout => ModerationError::SubmissionUnknown,
        other => other,
    }
}
