use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Environment,
    http::client::{Factory, aliyun_rpc::AliyunRpcSigner},
};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, field, http_status, secret};
use crate::{
    Assessment, Coverage, Decision, Finding, Modality, ModerationDriver, ModerationError,
    ModerationFuture, ModerationResult, Operation, TextInput, input::valid_language_hint,
};

pub(super) struct Aliyun {
    endpoint: Endpoint,
    access_key_id: Zeroizing<String>,
    access_key_secret: Zeroizing<String>,
    security_token: Option<Zeroizing<String>>,
    text_service: String,
    text_languages: Vec<String>,
}

impl Aliyun {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
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
        let text_service = field(profile, "text_service")?
            .ok_or(ModerationError::InvalidConfiguration("text_service"))?
            .to_owned();
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
        })
    }

    async fn request(
        &self,
        action: &str,
        service: &str,
        parameters: Value,
    ) -> ModerationResult<Output> {
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
        let output: Output = response.json().map_err(|_| ModerationError::Protocol)?;
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
        operation == Operation::Text
    }

    fn text<'a>(&'a self, input: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async move {
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
                    &self.text_service,
                    json!({ "content": input.text() }),
                )
                .await?;
            read_text(output)
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

fn category(label: &str) -> &'static str {
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
