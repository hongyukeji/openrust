use std::collections::BTreeMap;

use crate::{ModerationError, ModerationResult};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Modality {
    Text,
    Visual,
    Speech,
    Ocr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Review,
    Block,
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
}

impl Assessment {
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
