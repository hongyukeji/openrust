use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ModerationError, ModerationResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Visual,
    Speech,
    Ocr,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Review,
    Block,
}

/// 供应商给出的原始置信分及其量程。不同供应商的分值仍需由应用策略解释。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Confidence {
    value: f64,
    maximum: f64,
}

impl Confidence {
    pub fn new(value: f64, maximum: f64) -> ModerationResult<Self> {
        if !maximum.is_finite()
            || maximum <= 0.0
            || !value.is_finite()
            || value < 0.0
            || value > maximum
        {
            return Err(ModerationError::InvalidInput("confidence"));
        }
        Ok(Self { value, maximum })
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn maximum(&self) -> f64 {
        self.maximum
    }
}

/// 从原始素材起点计时的证据位置，单位为毫秒。静止帧使用相同的起止值。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceSpan {
    start_ms: u64,
    end_ms: u64,
}

impl EvidenceSpan {
    pub fn new(start_ms: u64, end_ms: u64) -> ModerationResult<Self> {
        if end_ms < start_ms {
            return Err(ModerationError::InvalidInput("evidence span"));
        }
        Ok(Self { start_ms, end_ms })
    }

    pub fn start_ms(&self) -> u64 {
        self.start_ms
    }

    pub fn end_ms(&self) -> u64 {
        self.end_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub modality: Modality,
    pub category: String,
    pub provider_label: String,
}

/// 归一化的供应商结果，不直接授权发布业务内容。
#[derive(Clone)]
pub struct Assessment {
    decision: Decision,
    coverage: BTreeMap<Modality, Coverage>,
    findings: Vec<Finding>,
    request_id: Option<String>,
    confidences: BTreeMap<usize, Confidence>,
    spans: BTreeMap<usize, EvidenceSpan>,
}

impl Assessment {
    pub(crate) fn recommendation(&self) -> Decision {
        self.decision
    }

    pub fn new(
        decision: Decision,
        coverage: BTreeMap<Modality, Coverage>,
        findings: Vec<Finding>,
    ) -> Self {
        Self {
            decision,
            coverage,
            findings,
            request_id: None,
            confidences: BTreeMap::new(),
            spans: BTreeMap::new(),
        }
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> ModerationResult<Self> {
        let value = request_id.into();
        if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(ModerationError::InvalidInput("request ID"));
        }
        self.request_id = Some(value);
        Ok(self)
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn with_confidence(
        mut self,
        finding: usize,
        confidence: Confidence,
    ) -> ModerationResult<Self> {
        if finding >= self.findings.len() {
            return Err(ModerationError::InvalidInput("confidence finding index"));
        }
        self.confidences.insert(finding, confidence);
        Ok(self)
    }

    pub fn confidence(&self, finding: usize) -> Option<Confidence> {
        self.confidences.get(&finding).copied()
    }

    pub fn with_evidence_span(
        mut self,
        finding: usize,
        span: EvidenceSpan,
    ) -> ModerationResult<Self> {
        if finding >= self.findings.len() {
            return Err(ModerationError::InvalidInput("span finding index"));
        }
        self.spans.insert(finding, span);
        Ok(self)
    }

    pub fn evidence_span(&self, finding: usize) -> Option<EvidenceSpan> {
        self.spans.get(&finding).copied()
    }

    pub fn coverage(&self, modality: Modality) -> Coverage {
        self.coverage
            .get(&modality)
            .copied()
            .unwrap_or(Coverage::Unavailable)
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// 允许结论须具备显式必检集合和完整覆盖。审查、拦截仍能保留已发现的风险证据。
    pub fn decision(&self, required: &[Modality]) -> Option<Decision> {
        if self.decision == Decision::Allow
            && (required.is_empty()
                || required
                    .iter()
                    .any(|&modality| self.coverage(modality) != Coverage::Complete))
        {
            return None;
        }
        Some(self.decision)
    }
}

pub enum TaskStatus {
    Pending,
    Running,
    Succeeded(Assessment),
    Failed,
    Cancelled,
    Expired,
}
