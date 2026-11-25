use std::sync::Arc;

use http::{HeaderName, HeaderValue};
use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Endpoint, basic_language, field, secret, sensitive_header, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct Azure {
    endpoint: Endpoint,
    key: HeaderValue,
    region: Option<HeaderValue>,
}

impl Azure {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        let key = secret(env, profile, "api_key_env")?
            .ok_or(TranslationError::InvalidConfiguration("api_key_env"))?;
        let region = field(profile, "region")?
            .map(|region| {
                if !region
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                {
                    return Err(TranslationError::InvalidConfiguration("region"));
                }
                HeaderValue::from_str(region)
                    .map_err(|_| TranslationError::InvalidConfiguration("region"))
            })
            .transpose()?;
        Ok(Self {
            endpoint: Endpoint::new(
                http,
                profile,
                Some("https://api.cognitive.microsofttranslator.com"),
                "/translate",
            )?,
            key: sensitive_header(&key, "api_key_env")?,
            region,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    translations: [Entry; 1],
    detected_language: Option<DetectedLanguage>,
}

#[derive(Deserialize)]
struct Entry {
    text: String,
    to: String,
}

#[derive(Deserialize)]
struct DetectedLanguage {
    language: String,
}

impl Translator for Azure {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let target = language(request.target())?;
            let mut query = vec![
                ("api-version".to_owned(), "3.0".to_owned()),
                ("to".to_owned(), target.clone()),
            ];
            if let Some(source) = request.source() {
                query.push(("from".to_owned(), language(source)?));
            }
            let response = self
                .endpoint
                .post(&json!([{"Text":request.text()}]), |request| {
                    let mut request = request.with_query_parameters(query).with_header(
                        HeaderName::from_static("ocp-apim-subscription-key"),
                        self.key.clone(),
                    );
                    if let Some(region) = &self.region {
                        request = request.with_header(
                            HeaderName::from_static("ocp-apim-subscription-region"),
                            region.clone(),
                        );
                    }
                    request
                })
                .await?;
            let http = response.status().as_u16();
            if http != 200 {
                let body: Value = response.json().unwrap_or_default();
                return Err(match body["error"]["code"].as_u64() {
                    Some(400003 | 400019 | 400023 | 400035 | 400036) => {
                        TranslationError::UnsupportedLanguage
                    }
                    Some(403001) => TranslationError::Quota,
                    _ => status(http),
                });
            }
            let [
                Output {
                    translations: [entry],
                    detected_language,
                },
            ]: [Output; 1] = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            if !entry.to.eq_ignore_ascii_case(&target) {
                return Err(TranslationError::InvalidResponse);
            }
            let result = Translation::new(entry.text);
            match detected_language {
                Some(detected) => result.with_detected_source(&detected.language),
                None => Ok(result),
            }
        })
    }
}

fn language(tag: &str) -> TranslationResult<String> {
    match tag {
        "zh-Hans" | "zh-Hant" | "pt-BR" | "pt-PT" | "fr-CA" | "sr-Cyrl" | "sr-Latn" | "mn-Cyrl"
        | "mn-Mong" | "iu-Latn" => Ok(tag.to_owned()),
        _ => Ok(basic_language(tag)?.to_owned()),
    }
}
