use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use openrust::{
    filesystem::FilesystemManager,
    http::client::{Factory, Part},
};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Endpoint, field, http_status,
    media::VersionedMediaLinks,
    sightengine::{endpoint, options, provider_tag},
};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationError,
    ModerationResult,
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
type VisualClass = (&'static str, &'static str, &'static str);

const NUDITY: &[&str] = &[
    "sexual_activity",
    "sexual_display",
    "erotica",
    "very_suggestive",
    "suggestive",
    "mildly_suggestive",
];

pub(super) struct ImageService {
    endpoint: Endpoint,
    models: Vec<String>,
    ocr: Option<Vec<String>>,
    review_score: f64,
    media: Option<VersionedMediaLinks>,
}

impl ImageService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("image_models").is_none() {
            return Ok(None);
        }
        let models = options(profile, "image_models", MODELS)?;
        let ocr = models
            .iter()
            .any(|model| model == "text-content-2.0")
            .then(|| options(profile, "image_text_categories", OCR_CATEGORIES))
            .transpose()?;
        let review_score = profile
            .get("image_review_score")
            .and_then(Value::as_f64)
            .filter(|score| (0.0..=1.0).contains(score))
            .ok_or(ModerationError::InvalidConfiguration("image_review_score"))?;
        let endpoint = endpoint(http, profile, "/1.0/check.json")?;
        let media = match (
            field(profile, "media_disk")?,
            field(profile, "media_origin")?,
        ) {
            (None, None) => None,
            _ => Some(VersionedMediaLinks::new(storage, profile)?),
        };
        Ok(Some(Self {
            endpoint,
            models,
            ocr,
            review_score,
            media,
        }))
    }

    pub(super) async fn assess(
        &self,
        user: &str,
        secret: &str,
        input: &ImageInput,
    ) -> ModerationResult<Assessment> {
        let mut parameters = vec![
            ("api_user", user.to_owned()),
            ("api_secret", secret.to_owned()),
            ("models", self.models.join(",")),
        ];
        if let Some(categories) = &self.ocr {
            parameters.push(("text_categories", categories.join(",")));
        }
        let headers = http::HeaderMap::new();
        let response = if let Some(bytes) = input.as_bytes() {
            let mut parts: Vec<_> = parameters
                .into_iter()
                .map(|(name, value)| (name, Part::text(value)))
                .collect();
            parts.push(("media", Part::bytes(bytes.to_vec()).file_name("image")));
            self.endpoint.post_parts(headers, parts).await?
        } else if let (Some(media), Some(source)) = (&self.media, input.as_stored()) {
            let url = media.create(source).await?;
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(parameters)
                .append_pair("url", url.as_str())
                .finish();
            self.endpoint
                .post_encoded(headers, body, "application/x-www-form-urlencoded")
                .await?
        } else {
            return Err(ModerationError::InvalidInput("sightengine stored image"));
        };
        http_status(response.status().as_u16())?;
        let output: ImageOutput = response.json().map_err(|_| ModerationError::Protocol)?;
        self.result(output)
    }

    fn result(&self, output: ImageOutput) -> ModerationResult<Assessment> {
        if output.status != "success" {
            return Err(ModerationError::Protocol);
        }
        let mut coverage = BTreeMap::new();
        let mut visual_findings = BTreeMap::new();
        let mut ocr_findings = BTreeSet::new();
        let frames =
            std::iter::once(&output.models).chain(output.data.iter().flat_map(|data| &data.frames));
        for frame in frames {
            if self.read_visual(frame, &mut visual_findings)? {
                coverage.insert(Modality::Visual, Coverage::Partial);
            }
            if let (Some(configured), Some(text)) = (&self.ocr, frame.get("text")) {
                let output: OcrOutput =
                    serde_json::from_value(text.clone()).map_err(|_| ModerationError::Protocol)?;
                for label in output.detected_categories {
                    if !provider_tag(&label) {
                        return Err(ModerationError::Protocol);
                    }
                    if configured.contains(&label) {
                        ocr_findings.insert(label);
                    } else {
                        ocr_findings.insert("unknown".to_owned());
                    }
                }
                coverage.insert(Modality::Ocr, Coverage::Partial);
            }
        }
        // 仅代表已处理的图像或帧，响应无法证明顺序素材的完整覆盖。
        let mut findings = Vec::new();
        let mut confidence = Vec::new();
        for ((model, field, category), score) in visual_findings {
            confidence.push((findings.len(), Confidence::new(score, 1.0)?));
            findings.push(Finding {
                modality: Modality::Visual,
                provider_label: format!("{model}/{field}"),
                category: category.to_owned(),
            });
        }
        for label in ocr_findings {
            let category = match label.as_str() {
                "phone_number" | "email" => "personal_information",
                "insult" | "inappropriate" => "harassment",
                other => other,
            };
            findings.push(Finding {
                modality: Modality::Ocr,
                category: category.to_owned(),
                provider_label: format!("text-content-2.0/{label}"),
            });
        }
        let decision = if findings.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let mut result = Assessment::new(decision, coverage, findings)
            .with_request_id(output.request.id)
            .map_err(|_| ModerationError::Protocol)?;
        for (index, score) in confidence {
            result = result.with_confidence(index, score)?;
        }
        Ok(result)
    }

    fn read_visual(
        &self,
        results: &BTreeMap<String, Value>,
        risks: &mut BTreeMap<VisualClass, f64>,
    ) -> ModerationResult<bool> {
        let mut found = false;
        for model in &self.models {
            let (model_name, field, labels, category): (_, _, &[&str], _) = match model.as_str() {
                "nudity-2.1" => ("nudity-2.1", "nudity", NUDITY, "sexual"),
                "violence" => ("violence", "violence", &["prob"], "violence"),
                "gore-2.0" => ("gore-2.0", "gore", &["prob"], "violence"),
                _ => continue,
            };
            let Some(output) = results.get(field) else {
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
                found = true;
                if score >= self.review_score {
                    let highest = risks.entry((model_name, label, category)).or_default();
                    *highest = highest.max(score);
                }
            }
        }
        Ok(found)
    }
}

#[derive(Deserialize)]
struct ImageOutput {
    status: String,
    request: RequestMetadata,
    data: Option<FrameData>,
    #[serde(flatten)]
    models: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct RequestMetadata {
    id: String,
}

#[derive(Deserialize)]
struct FrameData {
    frames: Vec<BTreeMap<String, Value>>,
}

#[derive(Deserialize)]
struct OcrOutput {
    detected_categories: Vec<String>,
}
