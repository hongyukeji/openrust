use std::{collections::BTreeMap, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{
    config::Environment,
    filesystem::FilesystemManager,
    http::client::{
        Factory,
        tencent_rpc::{CONTENT_TYPE, TencentRpcOperation, TencentRpcSigner},
    },
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{
    Endpoint, field, http_status, secret, tencent_image::ImageService, tencent_video::VideoService,
};
use crate::{
    Assessment, Confidence, Coverage, Decision, Finding, ImageInput, Modality, ModerationDriver,
    ModerationError, ModerationFuture, ModerationResult, Operation, TaskId, TaskStatus, TextInput,
    VideoInput,
};

pub(super) struct Tencent {
    text: Option<TextService>,
    image: Option<ImageService>,
    video: Option<VideoService>,
    secret_id: Zeroizing<String>,
    secret_key: Zeroizing<String>,
    security_token: Option<Zeroizing<String>>,
    region: String,
}

struct TextService {
    endpoint: Endpoint,
    biz_type: String,
    english_text: bool,
}

impl Tencent {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let region =
            field(profile, "region")?.ok_or(ModerationError::InvalidConfiguration("region"))?;
        let biz_type = field(profile, "biz_type")?;
        let english_text = match profile.get("english_text") {
            None => false,
            Some(value) => value
                .as_bool()
                .ok_or(ModerationError::InvalidConfiguration("english_text"))?,
        };
        let text = biz_type
            .map(|biz_type| {
                Ok::<_, ModerationError>(TextService {
                    endpoint: rpc_endpoint(
                        http.clone(),
                        profile,
                        "https://tms.tencentcloudapi.com",
                    )?,
                    biz_type: biz_type.to_owned(),
                    english_text,
                })
            })
            .transpose()?;
        let image = ImageService::configured(http.clone(), storage.clone(), profile)?;
        let video = VideoService::configured(http, storage, profile)?;
        if text.is_none() && image.is_none() && video.is_none() {
            return Err(ModerationError::InvalidConfiguration("tencent services"));
        }
        Ok(Self {
            text,
            image,
            video,
            secret_id: secret(env, profile, "secret_id_env")?
                .ok_or(ModerationError::InvalidConfiguration("secret_id_env"))?,
            secret_key: secret(env, profile, "secret_key_env")?
                .ok_or(ModerationError::InvalidConfiguration("secret_key_env"))?,
            security_token: secret(env, profile, "security_token_env")?,
            region: region.to_owned(),
        })
    }

    pub(super) async fn request<T: DeserializeOwned>(
        &self,
        endpoint: &Endpoint,
        service: &'static str,
        action: &'static str,
        version: &'static str,
        parameters: Value,
    ) -> ModerationResult<T> {
        let body = parameters.to_string();
        let headers = TencentRpcSigner {
            secret_id: &self.secret_id,
            secret_key: self.secret_key.as_bytes(),
            security_token: self.security_token.as_ref().map(|value| value.as_str()),
        }
        .sign_post(
            &endpoint.url,
            TencentRpcOperation {
                service,
                action,
                version,
                region: &self.region,
            },
            body.as_bytes(),
        )
        .map_err(|_| ModerationError::InvalidConfiguration("tencent credentials"))?;
        let response = endpoint.post_encoded(headers, body, CONTENT_TYPE).await?;
        http_status(response.status().as_u16())?;
        let envelope: Envelope = response.json().map_err(|_| ModerationError::Protocol)?;
        if let Some(error) = envelope
            .response
            .get("Error")
            .filter(|error| !error.is_null())
        {
            let error: ProviderError =
                serde_json::from_value(error.clone()).map_err(|_| ModerationError::Protocol)?;
            return Err(service_error(&error.code));
        }
        serde_json::from_value(envelope.response).map_err(|_| ModerationError::Protocol)
    }
}

pub(super) fn rpc_endpoint(
    http: Arc<Factory>,
    profile: &Value,
    default: &str,
) -> ModerationResult<Endpoint> {
    let endpoint = Endpoint::new(http, profile, default)?;
    if endpoint.url.path() != "/" {
        return Err(ModerationError::InvalidConfiguration("base_url"));
    }
    Ok(endpoint)
}

impl TextService {
    fn language(&self, hint: &str) -> ModerationResult<&'static str> {
        let hint = hint.to_ascii_lowercase();
        match hint.as_str() {
            "zh" | "zh-hans" | "zh-hant" | "zh-cn" | "zh-tw" => Ok("zh"),
            "en" | "en-us" | "en-gb" if self.english_text => Ok("en"),
            _ => Err(ModerationError::InvalidInput("tencent text language")),
        }
    }
}

impl ModerationDriver for Tencent {
    fn supports(&self, operation: Operation) -> bool {
        match operation {
            Operation::Text => self.text.is_some(),
            Operation::Image => self.image.is_some(),
            Operation::SubmitVideo | Operation::QueryVideo | Operation::CancelVideo => {
                self.video.is_some()
            }
        }
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let text = self
                .text
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::Text))?;
            if input.text().chars().count() > 10_000 {
                return Err(ModerationError::InvalidInput("tencent text length"));
            }
            let mut body = json!({"Content":STANDARD.encode(input.text()),"Type":"TEXT","BizType":text.biz_type});
            if let Some(language) = input.language() {
                body["SourceLanguage"] = json!(text.language(language)?);
            }
            let output = self
                .request(&text.endpoint, "tms", "TextModeration", "2020-12-29", body)
                .await?;
            read_text(output, &text.biz_type)
        })
    }

    fn image<'a>(&'a self, input: &'a ImageInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
            let image = self
                .image
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::Image))?;
            image.assess(self, input).await
        })
    }

    fn submit_video<'a>(&'a self, input: &'a VideoInput) -> ModerationFuture<'a, TaskId> {
        Box::pin(async move {
            self.video
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::SubmitVideo))?
                .submit(self, input)
                .await
        })
    }

    fn query_video<'a>(&'a self, id: &'a TaskId) -> ModerationFuture<'a, TaskStatus> {
        Box::pin(async move {
            self.video
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::QueryVideo))?
                .query(self, id)
                .await
        })
    }

    fn cancel_video<'a>(&'a self, id: &'a TaskId) -> ModerationFuture<'a, ()> {
        Box::pin(async move {
            self.video
                .as_ref()
                .ok_or(ModerationError::Unsupported(Operation::CancelVideo))?
                .cancel(self, id)
                .await
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Envelope {
    response: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Output {
    request_id: Option<String>,
    biz_type: Option<String>,
    #[serde(flatten)]
    result: Option<Verdict>,
    detail_results: Option<Vec<Verdict>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProviderError {
    code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Verdict {
    suggestion: String,
    pub(super) label: String,
    sub_label: Option<String>,
    score: Option<u8>,
}

impl Verdict {
    pub(super) fn decision(&self) -> ModerationResult<Decision> {
        if self.label.trim().is_empty() || self.score.is_some_and(|value| value > 100) {
            return Err(ModerationError::Protocol);
        }
        match self.suggestion.as_str() {
            "Pass" => Ok(Decision::Allow),
            "Review" => Ok(Decision::Review),
            "Block" => Ok(Decision::Block),
            _ => Err(ModerationError::Protocol),
        }
    }

    pub(super) fn finding(&self, modality: Modality) -> (Finding, Option<f64>) {
        let category = match self.label.as_str() {
            "Porn" => "sexual",
            "Abuse" => "harassment",
            "Ad" => "advertising",
            "Terror" | "Violence" => "violence",
            "Illegal" => "contraband",
            "Politics" => "political",
            _ => "unknown",
        };
        let provider_label = match self.sub_label.as_deref().filter(|label| !label.is_empty()) {
            Some(sub_label) => format!("{}/{sub_label}", self.label),
            None => self.label.clone(),
        };
        (
            Finding {
                modality,
                category: category.to_owned(),
                provider_label,
            },
            self.score.map(f64::from),
        )
    }
}

fn read_text(output: Output, biz_type: &str) -> ModerationResult<Assessment> {
    if output.biz_type.as_deref() != Some(biz_type) {
        return Err(ModerationError::Protocol);
    }
    let result = output.result.ok_or(ModerationError::Protocol)?;
    let mut decision = result.decision()?;
    let mut details = Vec::new();
    if decision != Decision::Allow || result.label != "Normal" {
        details.push(result.finding(Modality::Text));
        if decision == Decision::Allow {
            decision = Decision::Review;
        }
    }
    for result in output.detail_results.unwrap_or_default() {
        match result.decision()? {
            Decision::Allow => continue,
            Decision::Block => decision = Decision::Block,
            Decision::Review if decision == Decision::Allow => decision = Decision::Review,
            Decision::Review => {}
        }
        let detail = result.finding(Modality::Text);
        if details.first() != Some(&detail) {
            details.push(detail);
        }
    }
    let (findings, scores): (Vec<_>, Vec<_>) = details.into_iter().unzip();
    let mut assessment = Assessment::new(
        decision,
        BTreeMap::from([(Modality::Text, Coverage::Complete)]),
        findings,
    )
    .with_request_id(output.request_id.ok_or(ModerationError::Protocol)?)
    .map_err(|_| ModerationError::Protocol)?;
    for (index, score) in scores.into_iter().enumerate() {
        if let Some(score) = score {
            assessment = assessment.with_confidence(index, Confidence::new(score, 100.0)?)?;
        }
    }
    Ok(assessment)
}

pub(super) fn service_error(code: &str) -> ModerationError {
    match code.split('.').next().unwrap_or_default() {
        "AuthFailure" | "UnauthorizedOperation" => ModerationError::Unauthorized,
        "LimitExceeded" | "RequestLimitExceeded" => ModerationError::Throttled,
        "InternalError" | "ResourceUnavailable" => ModerationError::ServiceUnavailable,
        _ => ModerationError::Rejected,
    }
}
