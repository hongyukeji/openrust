use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use super::{
    sightengine::{options, provider_tag},
    video::VideoEvidence,
};
use crate::{
    Confidence, Coverage, EvidenceSpan, Finding, Modality, ModerationError, ModerationResult,
};

const MODELS: &[&str] = &["nudity-2.1", "violence", "gore-2.0", "text-content-2.0"];
const OCR_CATEGORIES: &[&str] = &[
    "sexual",
    "insult",
    "inappropriate",
    "discriminatory",
    "phone_number",
    "email",
];
const NUDITY: &[&str] = &[
    "sexual_activity",
    "sexual_display",
    "erotica",
    "very_suggestive",
    "suggestive",
    "mildly_suggestive",
];
type VisualClass = (&'static str, &'static str, &'static str);

pub(super) struct MediaModels {
    models: Vec<String>,
    ocr: Option<Vec<String>>,
    review_score: f64,
}

impl MediaModels {
    pub(super) fn new(
        profile: &Value,
        models_key: &'static str,
        score_key: &'static str,
        ocr_key: &'static str,
    ) -> ModerationResult<Self> {
        let models = options(profile, models_key, MODELS)?;
        let ocr = models
            .iter()
            .any(|model| model == "text-content-2.0")
            .then(|| options(profile, ocr_key, OCR_CATEGORIES))
            .transpose()?;
        let review_score = profile
            .get(score_key)
            .and_then(Value::as_f64)
            .filter(|score| (0.0..=1.0).contains(score))
            .ok_or(ModerationError::InvalidConfiguration(score_key))?;
        Ok(Self {
            models,
            ocr,
            review_score,
        })
    }

    pub(super) fn parameters(&self) -> Vec<(&'static str, String)> {
        let mut parameters = vec![("models", self.models.join(","))];
        if let Some(ocr) = &self.ocr {
            parameters.push(("text_categories", ocr.join(",")));
        }
        parameters
    }

    pub(super) fn observe(
        &self,
        frame: &BTreeMap<String, Value>,
        findings: &mut MediaFindings,
    ) -> ModerationResult<()> {
        for model in &self.models {
            let (name, field, labels, category): (_, _, &[&str], _) = match model.as_str() {
                "nudity-2.1" => ("nudity-2.1", "nudity", NUDITY, "sexual"),
                "violence" => ("violence", "violence", &["prob"], "violence"),
                "gore-2.0" => ("gore-2.0", "gore", &["prob"], "violence"),
                _ => continue,
            };
            let Some(output) = frame.get(field) else {
                continue;
            };
            for &label in labels {
                let Some(score) = output.get(label) else {
                    continue;
                };
                let score = score
                    .as_f64()
                    .filter(|score| (0.0..=1.0).contains(score))
                    .ok_or(ModerationError::Protocol)?;
                findings.visual_seen = true;
                if score >= self.review_score {
                    let highest = findings.visual.entry((name, label, category)).or_default();
                    *highest = highest.max(score);
                }
            }
        }
        if let (Some(configured), Some(text)) = (&self.ocr, frame.get("text")) {
            let output: OcrOutput =
                OcrOutput::deserialize(text).map_err(|_| ModerationError::Protocol)?;
            for label in output.detected_categories {
                if !provider_tag(&label) {
                    return Err(ModerationError::Protocol);
                }
                findings.ocr.insert(if configured.contains(&label) {
                    label
                } else {
                    "unknown".to_owned()
                });
            }
            findings.ocr_seen = true;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct MediaFindings {
    visual_seen: bool,
    visual: BTreeMap<VisualClass, f64>,
    ocr_seen: bool,
    ocr: BTreeSet<String>,
}

impl MediaFindings {
    pub(super) fn coverage(&self) -> BTreeMap<Modality, Coverage> {
        // 返回值只证明相应帧完成检查，无法证明完整覆盖顺序素材。
        let mut coverage = BTreeMap::new();
        if self.visual_seen {
            coverage.insert(Modality::Visual, Coverage::Partial);
        }
        if self.ocr_seen {
            coverage.insert(Modality::Ocr, Coverage::Partial);
        }
        coverage
    }

    pub(super) fn evidence(
        self,
        span: Option<EvidenceSpan>,
    ) -> ModerationResult<Vec<VideoEvidence>> {
        let mut evidence = Vec::with_capacity(self.visual.len() + self.ocr.len());
        for ((model, field, category), score) in self.visual {
            evidence.push(VideoEvidence {
                finding: Finding {
                    modality: Modality::Visual,
                    provider_label: format!("{model}/{field}"),
                    category: category.to_owned(),
                },
                score: Some(Confidence::new(score, 1.0)?),
                span,
            });
        }
        for label in self.ocr {
            let category = match label.as_str() {
                "phone_number" | "email" => "personal_information",
                "insult" | "inappropriate" => "harassment",
                other => other,
            };
            evidence.push(VideoEvidence {
                finding: Finding {
                    modality: Modality::Ocr,
                    provider_label: format!("text-content-2.0/{label}"),
                    category: category.to_owned(),
                },
                score: None,
                span,
            });
        }
        Ok(evidence)
    }
}

#[derive(Deserialize)]
struct OcrOutput {
    detected_categories: Vec<String>,
}
