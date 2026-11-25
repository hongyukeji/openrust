use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    Assessment, Confidence, Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError,
    ModerationResult,
};

const MAX_BYTES: usize = 48 * 1024;
const MAX_FINDINGS: usize = 256;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    modality: Modality,
    category: String,
    provider_label: String,
    confidence: Option<[f64; 2]>,
    span_ms: Option<[u64; 2]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    format: u8,
    recommendation: Decision,
    coverage: BTreeMap<Modality, Coverage>,
    provider_reference: Option<String>,
    findings: Vec<Evidence>,
}

pub(super) fn encode(assessment: &Assessment) -> ModerationResult<String> {
    let limits = |value: &str| {
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
    };
    if assessment.findings().len() > MAX_FINDINGS
        || assessment
            .findings()
            .iter()
            .any(|finding| !limits(&finding.category) || !limits(&finding.provider_label))
    {
        return Err(ModerationError::Protocol);
    }
    let coverage = [
        Modality::Text,
        Modality::Visual,
        Modality::Speech,
        Modality::Ocr,
    ]
    .into_iter()
    .map(|modality| (modality, assessment.coverage(modality)))
    .collect();
    let findings = assessment
        .findings()
        .iter()
        .enumerate()
        .map(|(index, finding)| Evidence {
            modality: finding.modality,
            category: finding.category.clone(),
            provider_label: finding.provider_label.clone(),
            confidence: assessment
                .confidence(index)
                .map(|score| [score.value(), score.maximum()]),
            span_ms: assessment
                .evidence_span(index)
                .map(|span| [span.start_ms(), span.end_ms()]),
        })
        .collect();
    let record = Record {
        format: 1,
        recommendation: assessment.recommendation(),
        coverage,
        provider_reference: assessment.request_id().map(str::to_owned),
        findings,
    };
    serde_json::to_string(&record)
        .ok()
        .filter(|value| value.len() <= MAX_BYTES)
        .ok_or(ModerationError::Protocol)
}

pub(super) fn decode(raw: &str) -> ModerationResult<Assessment> {
    if raw.len() > MAX_BYTES {
        return Err(ModerationError::ReviewStorage);
    }
    let record: Record = serde_json::from_str(raw).map_err(|_| ModerationError::ReviewStorage)?;
    if record.format != 1 || record.findings.len() > MAX_FINDINGS {
        return Err(ModerationError::ReviewStorage);
    }
    let labels = record
        .findings
        .iter()
        .map(|evidence| Finding {
            modality: evidence.modality,
            category: evidence.category.clone(),
            provider_label: evidence.provider_label.clone(),
        })
        .collect();
    let mut assessment = Assessment::new(record.recommendation, record.coverage, labels);
    if let Some(reference) = record.provider_reference {
        assessment = assessment
            .with_request_id(reference)
            .map_err(|_| ModerationError::ReviewStorage)?;
    }
    for (index, finding) in record.findings.into_iter().enumerate() {
        if let Some([score, range]) = finding.confidence {
            assessment = assessment
                .with_confidence(
                    index,
                    Confidence::new(score, range).map_err(|_| ModerationError::ReviewStorage)?,
                )
                .map_err(|_| ModerationError::ReviewStorage)?;
        }
        if let Some([start, end]) = finding.span_ms {
            assessment = assessment
                .with_evidence_span(
                    index,
                    EvidenceSpan::new(start, end).map_err(|_| ModerationError::ReviewStorage)?,
                )
                .map_err(|_| ModerationError::ReviewStorage)?;
        }
    }
    Ok(assessment)
}
