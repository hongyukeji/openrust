use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    filesystem::FilesystemManager,
    http::client::{Factory, Part},
};
use serde::Deserialize;
use serde_json::Value;

use super::{
    field,
    hive::{Model, configured_set, provider_class},
    media::VersionedMediaLinks,
};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationError,
    ModerationResult,
};

pub(super) struct ImageService {
    model: Model,
    review_score: f64,
    classes: std::collections::BTreeSet<String>,
    media: Option<VersionedMediaLinks>,
}

impl ImageService {
    pub(super) fn configured(
        http: Arc<Factory>,
        env: &openrust::config::Environment,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("image_project").is_none() {
            return Ok(None);
        }
        let model_type = field(profile, "image_model_type")?
            .filter(|value| {
                value.len() <= 64
                    && value.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    && *value != "TEXT_CLASSIFICATION"
            })
            .ok_or(ModerationError::InvalidConfiguration("image_model_type"))?;
        let model = Model::new(
            http,
            env,
            profile,
            "image_token_env",
            "image_project",
            model_type,
        )?;
        let classes = configured_set(profile, "image_classes", provider_class)?;
        let review_score = profile
            .get("image_review_score")
            .and_then(Value::as_f64)
            .filter(|score| (0.0..=1.0).contains(score))
            .ok_or(ModerationError::InvalidConfiguration("image_review_score"))?;
        let media = match (
            field(profile, "media_disk")?,
            field(profile, "media_origin")?,
        ) {
            (None, None) => None,
            _ => Some(VersionedMediaLinks::new(storage, profile)?),
        };
        Ok(Some(Self {
            model,
            review_score,
            classes,
            media,
        }))
    }

    pub(super) async fn assess(&self, input: &ImageInput) -> ModerationResult<Assessment> {
        let (id, result): (_, ImageOutput) = if let Some(bytes) = input.as_bytes() {
            self.model
                .submit_parts(vec![(
                    "media",
                    Part::bytes(bytes.to_vec()).file_name("image"),
                )])
                .await?
        } else if let (Some(media), Some(source)) = (&self.media, input.as_stored()) {
            let url = media.create(source).await?;
            let payload = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("url", url.as_str())
                .finish();
            self.model.submit(payload).await?
        } else {
            return Err(ModerationError::InvalidInput("hive stored image"));
        };
        self.result(id, result)
    }

    fn result(&self, id: String, result: ImageOutput) -> ModerationResult<Assessment> {
        let mut maximum = BTreeMap::new();
        for frame in &result.output {
            let mut scores = BTreeMap::new();
            for prediction in &frame.classes {
                if !(0.0..=1.0).contains(&prediction.score)
                    || scores.insert(&prediction.class, prediction.score).is_some()
                {
                    return Err(ModerationError::Protocol);
                }
            }
            for class in &self.classes {
                if let Some(&score) = scores.get(class) {
                    let previous = maximum.entry(class).or_insert(0.0_f64);
                    *previous = previous.max(score);
                }
            }
        }
        // 图片提交也可包含动图。供应商返回的结果描述采样帧，缺少原件总帧数。
        let coverage = if maximum.is_empty() {
            Coverage::Unavailable
        } else {
            Coverage::Partial
        };
        let mut findings = Vec::new();
        let mut scores = Vec::new();
        for (class, score) in maximum {
            if score >= self.review_score {
                findings.push(Finding {
                    modality: Modality::Visual,
                    category: category(class).to_owned(),
                    provider_label: class.clone(),
                });
                scores.push(Confidence::new(score, 1.0)?);
            }
        }
        let decision = if findings.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let mut result = Assessment::new(
            decision,
            BTreeMap::from([(Modality::Visual, coverage)]),
            findings,
        )
        .with_request_id(id)
        .map_err(|_| ModerationError::Protocol)?;
        for (index, confidence) in scores.into_iter().enumerate() {
            result = result.with_confidence(index, confidence)?;
        }
        Ok(result)
    }
}

#[derive(Deserialize)]
struct ImageOutput {
    output: Vec<Frame>,
}

#[derive(Deserialize)]
struct Frame {
    classes: Vec<Prediction>,
}

#[derive(Deserialize)]
struct Prediction {
    class: String,
    score: f64,
}

fn category(class: &str) -> &'static str {
    match class {
        "general_nsfw" | "general_suggestive" => "sexual",
        "gun_in_hand" | "gun_not_in_hand" | "animated_gun" => "violence",
        _ => "unknown",
    }
}
