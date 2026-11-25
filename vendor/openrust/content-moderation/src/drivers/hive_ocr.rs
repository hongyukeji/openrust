use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use openrust::{config::Environment, filesystem::FilesystemManager, http::client::Factory};
use serde::Deserialize;
use serde_json::Value;

use super::{
    hive::{
        Model, ModelResult, PatternMatches, ScoredClass, category, configured_set, media_links,
        provider_class, severity_scores, severity_threshold,
    },
    media::VersionedMediaLinks,
};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationError,
    ModerationResult,
};

pub(super) struct OcrService {
    model: Model,
    classes: BTreeSet<String>,
    review_severity: i8,
    media: Option<VersionedMediaLinks>,
}

impl OcrService {
    pub(super) fn configured(
        http: Arc<Factory>,
        env: &Environment,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("ocr_project").is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            model: Model::new(
                http,
                env,
                profile,
                "ocr_token_env",
                "ocr_project",
                "OCR_MODERATION",
            )?,
            classes: configured_set(profile, "ocr_classes", provider_class)?,
            review_severity: severity_threshold(profile, "ocr_review_severity")?,
            media: media_links(storage, profile)?,
        }))
    }

    pub(super) async fn assess(&self, input: &ImageInput) -> ModerationResult<Assessment> {
        let result: ModelResult<OcrOutput> =
            self.model.submit_image(input, self.media.as_ref()).await?;
        self.read(result)
    }

    fn read(&self, result: ModelResult<OcrOutput>) -> ModerationResult<Assessment> {
        let frames = &result.output.output;
        let mut complete = result.single_still_frame()
            && matches!(frames.as_slice(), [frame] if frame.time == Some(0.0));
        let mut maximum = BTreeMap::new();
        let mut patterns = BTreeMap::new();
        for block in frames.iter().flat_map(|frame| &frame.frame_results) {
            let values = severity_scores(&block.classes)?;
            for class in &self.classes {
                if let Some(&severity) = values
                    .get(class.as_str())
                    .filter(|severity| **severity >= 0)
                {
                    let highest = maximum.entry(class).or_insert(0);
                    *highest = (*highest).max(severity);
                } else {
                    complete = false;
                }
            }
            for finding in block.patterns.findings(Modality::Ocr) {
                patterns.insert(finding.provider_label.clone(), finding);
            }
        }

        let mut findings = Vec::new();
        let mut scores = Vec::new();
        for (class, score) in maximum {
            if score >= self.review_severity {
                findings.push(Finding {
                    modality: Modality::Ocr,
                    category: category(class).to_owned(),
                    provider_label: class.clone(),
                });
                scores.push(Confidence::new(f64::from(score), 3.0)?);
            }
        }
        findings.extend(patterns.into_values());
        let decision = if findings.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let coverage = if complete {
            Coverage::Complete
        } else if frames.is_empty() {
            Coverage::Unavailable
        } else {
            Coverage::Partial
        };
        let mut assessment = Assessment::new(
            decision,
            BTreeMap::from([(Modality::Ocr, coverage)]),
            findings,
        )
        .with_request_id(result.id)
        .map_err(|_| ModerationError::Protocol)?;
        for (index, score) in scores.into_iter().enumerate() {
            assessment = assessment.with_confidence(index, score)?;
        }
        Ok(assessment)
    }
}

#[derive(Deserialize)]
struct OcrOutput {
    output: Vec<Frame>,
}

#[derive(Deserialize)]
struct Frame {
    time: Option<f64>,
    frame_results: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    classes: Vec<ScoredClass>,
    #[serde(flatten)]
    patterns: PatternMatches,
}
