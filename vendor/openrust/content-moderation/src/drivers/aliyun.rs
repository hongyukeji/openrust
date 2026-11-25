use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Environment,
    filesystem::FilesystemManager,
    http::client::{Factory, aliyun_rpc::AliyunRpcSigner},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{
    Endpoint, aliyun_video::VideoService, field, http_status, media::VersionedMediaLinks, secret,
};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationDriver,
    ModerationError, ModerationFuture, ModerationResult, Operation, TaskId, TaskStatus, TextInput,
    VideoInput, input::valid_language_hint,
};

pub(super) struct Aliyun {
    endpoint: Endpoint,
    access_key_id: Zeroizing<String>,
    access_key_secret: Zeroizing<String>,
    security_token: Option<Zeroizing<String>>,
    text_service: Option<String>,
    text_languages: Vec<String>,
    image: Option<ImageService>,
    video: Option<VideoService>,
}

struct ImageService {
    name: String,
    media: VersionedMediaLinks,
}

impl Aliyun {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let region = field(profile, "region")?.unwrap_or("cn-shanghai");
        if !matches!(
            region,
            "cn-shanghai" | "cn-beijing" | "cn-hangzhou" | "cn-shenzhen" | "cn-chengdu"
        ) {
            return Err(ModerationError::InvalidConfiguration("region"));
        }
        let base = format!("https://green-cip.{region}.aliyuncs.com");
        let endpoint = Endpoint::new(http, profile, &base)?;
        if endpoint.url.path() != "/" {
            return Err(ModerationError::InvalidConfiguration("base_url"));
        }
        let text_service = field(profile, "text_service")?.map(str::to_owned);
        let image = field(profile, "image_service")?
            .map(|name| {
                Ok::<_, ModerationError>(ImageService {
                    name: name.to_owned(),
                    media: VersionedMediaLinks::new(storage.clone(), profile)?,
                })
            })
            .transpose()?;
        let video = VideoService::configured(storage, profile, region)?;
        if text_service.is_none() && image.is_none() && video.is_none() {
            return Err(ModerationError::InvalidConfiguration("aliyun services"));
        }
        let mut text_languages = Vec::new();
        if let Some(value) = profile.get("text_languages") {
            let languages = value
                .as_array()
                .filter(|values| values.len() <= 64)
                .ok_or(ModerationError::InvalidConfiguration("text_languages"))?;
            for value in languages {
                let language = value
                    .as_str()
                    .filter(|language| valid_language_hint(language))
                    .ok_or(ModerationError::InvalidConfiguration("text_languages"))?;
                text_languages.push(language.to_owned());
            }
        }
        Ok(Self {
            endpoint,
            access_key_id: secret(env, profile, "access_key_id_env")?
                .ok_or(ModerationError::InvalidConfiguration("access_key_id_env"))?,
            access_key_secret: secret(env, profile, "access_key_secret_env")?.ok_or(
                ModerationError::InvalidConfiguration("access_key_secret_env"),
            )?,
            security_token: secret(env, profile, "security_token_env")?,
            text_service,
            text_languages,
            image,
            video,
        })
    }

    async fn request(
        &self,
        action: &str,
        service: &str,
        parameters: Value,
    ) -> ModerationResult<Output> {
        let output: Output = self.rpc(action, service, parameters).await?;
        match output.code {
            200 => Ok(output),
            400 => Err(ModerationError::Rejected),
            408 => Err(ModerationError::Unauthorized),
            581 => Err(ModerationError::Timeout),
            588 => Err(ModerationError::Throttled),
            500..=599 => Err(ModerationError::ServiceUnavailable),
            _ => Err(ModerationError::Protocol),
        }
    }

    pub(super) async fn rpc<T: DeserializeOwned>(
        &self,
        action: &str,
        service: &str,
        parameters: Value,
    ) -> ModerationResult<T> {
        const MEDIA_TYPE: &str = "application/x-www-form-urlencoded";
        let encoded = parameters.to_string();
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([("Service", service), ("ServiceParameters", &encoded)])
            .finish();
        let headers = AliyunRpcSigner {
            access_key_id: &self.access_key_id,
            access_key_secret: self.access_key_secret.as_bytes(),
            security_token: self.security_token.as_ref().map(|token| token.as_str()),
        }
        .sign_post(
            &self.endpoint.url,
            action,
            "2022-03-02",
            Some(MEDIA_TYPE),
            body.as_bytes(),
        )
        .map_err(|_| ModerationError::InvalidConfiguration("aliyun credentials"))?;
        let response = self
            .endpoint
            .post_encoded(headers, body, MEDIA_TYPE)
            .await?;
        http_status(response.status().as_u16())?;
        response.json().map_err(|_| ModerationError::Protocol)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Output {
    code: u16,
    data: Option<Value>,
    request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TextData {
    #[serde(alias = "labels")]
    labels: String,
    #[serde(alias = "reason")]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextReason {
    risk_level: Option<String>,
}

impl ModerationDriver for Aliyun {
    fn supports(&self, operation: Operation) -> bool {
        match operation {
            Operation::Text => self.text_service.is_some(),
            Operation::Image => self.image.is_some(),
            Operation::SubmitVideo | Operation::QueryVideo => self.video.is_some(),
            _ => false,
        }
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let service = self
                .text_service
                .as_deref()
                .ok_or(ModerationError::Unsupported(Operation::Text))?;
            if input.text().chars().count() > 600 {
                return Err(ModerationError::InvalidInput("aliyun text length"));
            }
            if input.language().is_some_and(|requested| {
                !self
                    .text_languages
                    .iter()
                    .any(|language| language.eq_ignore_ascii_case(requested))
            }) {
                return Err(ModerationError::InvalidInput("aliyun text language"));
            }
            let output = self
                .request(
                    "TextModeration",
                    service,
                    json!({ "content": input.text() }),
                )
                .await?;
            read_text(output)
        })
    }

    fn image<'a>(&'a self, input: &'a ImageInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let image = self
                .image
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::Image))?;
            let source = input
                .as_stored()
                .ok_or(ModerationError::InvalidInput("aliyun stored image"))?;
            let url = image.media.create(source).await?;
            let output = self
                .request(
                    "ImageModeration",
                    &image.name,
                    json!({ "imageUrl": url.as_str() }),
                )
                .await?;
            read_image(output)
        })
    }

    fn submit_video<'a>(&'a self, input: &'a VideoInput) -> ModerationFuture<'a, TaskId> {
        Box::pin(async move {
            let service = self
                .video
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::SubmitVideo))?;
            service.submit(self, input).await
        })
    }

    fn query_video<'a>(&'a self, id: &'a TaskId) -> ModerationFuture<'a, TaskStatus> {
        Box::pin(async move {
            let service = self
                .video
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::QueryVideo))?;
            service.query(self, id).await
        })
    }
}

fn read_text(output: Output) -> ModerationResult<Assessment> {
    let data: TextData = serde_json::from_value(output.data.ok_or(ModerationError::Protocol)?)
        .map_err(|_| ModerationError::Protocol)?;
    let reason = data
        .reason
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(serde_json::from_str::<TextReason>)
        .transpose()
        .map_err(|_| ModerationError::Protocol)?;
    let findings: Vec<_> = data
        .labels
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(|label| Finding {
            modality: Modality::Text,
            category: category(label).to_owned(),
            provider_label: label.to_owned(),
        })
        .collect();
    let level = reason
        .as_ref()
        .and_then(|reason| reason.risk_level.as_deref());
    let decision = match level {
        Some("high") => Decision::Block,
        Some("medium" | "low") => Decision::Review,
        Some("none") if data.labels.trim().is_empty() => Decision::Allow,
        None if reason.is_none() && data.labels.trim().is_empty() => Decision::Allow,
        _ => Decision::Review,
    };
    Assessment::new(
        decision,
        BTreeMap::from([(Modality::Text, Coverage::Complete)]),
        findings,
    )
    .with_request_id(output.request_id.ok_or(ModerationError::Protocol)?)
    .map_err(|_| ModerationError::Protocol)
}

pub(super) fn category(label: &str) -> &'static str {
    match label {
        "sexual_content" => "sexual",
        "violence" => "violence",
        "cyberbullying" => "harassment",
        "profanity" => "profanity",
        "contraband" => "contraband",
        "ad" | "ad_compliance" => "advertising",
        "spam" => "spam",
        "political_content" => "political",
        "religion" => "religion",
        "C_customized" => "custom",
        _ => "unknown",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageData {
    result: Vec<ImageFinding>,
    risk_level: String,
    frame_num: Option<u32>,
    frame: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageFinding {
    label: String,
    confidence: Option<f64>,
}

fn read_image(output: Output) -> ModerationResult<Assessment> {
    let data: ImageData = serde_json::from_value(output.data.ok_or(ModerationError::Protocol)?)
        .map_err(|_| ModerationError::Protocol)?;
    let mut partial = data.frame_num.is_some_and(|count| count > 0)
        || data
            .frame
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    let mut findings = Vec::with_capacity(data.result.len());
    let mut scores = Vec::new();
    for result in data.result {
        if result.label.trim().is_empty() {
            return Err(ModerationError::Protocol);
        }
        if result.label == "nonLabel" && data.risk_level == "none" {
            // 服务也会在检测项全关闭时返回该标签，无法据此认定完整覆盖。
            partial = true;
            continue;
        }
        if let Some(score) = result.confidence {
            let confidence =
                Confidence::new(score, 100.0).map_err(|_| ModerationError::Protocol)?;
            scores.push((findings.len(), confidence));
        }
        findings.push(Finding {
            modality: Modality::Visual,
            category: image_category(&result.label).to_owned(),
            provider_label: result.label,
        });
    }
    let decision = match data.risk_level.as_str() {
        "high" => Decision::Block,
        "none" if findings.is_empty() => Decision::Allow,
        _ => Decision::Review,
    };
    let coverage = if partial {
        Coverage::Partial
    } else {
        Coverage::Complete
    };
    let mut assessment = Assessment::new(
        decision,
        BTreeMap::from([(Modality::Visual, coverage)]),
        findings,
    )
    .with_request_id(output.request_id.ok_or(ModerationError::Protocol)?)
    .map_err(|_| ModerationError::Protocol)?;
    for (index, confidence) in scores {
        assessment = assessment.with_confidence(index, confidence)?;
    }
    Ok(assessment)
}

pub(super) fn image_category(label: &str) -> &'static str {
    match label.split('_').next().unwrap_or_default() {
        "porn" | "sexual" => "sexual",
        "violent" | "violence" => "violence",
        "political" => "political",
        "ad" => "advertising",
        _ => "unknown",
    }
}
