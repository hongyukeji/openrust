use std::sync::Arc;

use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, basic_language, secret, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct LibreTranslate {
    endpoint: Endpoint,
    key: Option<Zeroizing<String>>,
}

impl LibreTranslate {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        Ok(Self {
            endpoint: Endpoint::new(http, profile, None, "/translate")?,
            key: secret(env, profile, "api_key_env")?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    translated_text: String,
    detected_language: Option<DetectedLanguage>,
}

#[derive(Deserialize)]
struct DetectedLanguage {
    language: String,
}

impl Translator for LibreTranslate {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let source = request
                .source()
                .map(language)
                .transpose()?
                .unwrap_or("auto");
            let target = language(request.target())?;
            let mut body =
                json!({"q":request.text(), "source":source, "target":target, "format":"text"});
            if let Some(key) = &self.key {
                body["api_key"] = key.as_str().into();
            }
            let response = self.endpoint.post(&body, |req| req).await?;
            if response.status().as_u16() != 200 {
                return Err(status(response.status().as_u16()));
            }
            let output: Output = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            let result = Translation::new(output.translated_text);
            match output.detected_language {
                Some(DetectedLanguage { language }) => {
                    result.with_detected_source(match language.as_str() {
                        "zh" => "zh-Hans",
                        "zt" => "zh-Hant",
                        "pb" => "pt-BR",
                        code => code,
                    })
                }
                None => Ok(result),
            }
        })
    }
}

fn language(language: &str) -> TranslationResult<&str> {
    match language {
        "zh-Hans" => Ok("zh"),
        "zh-Hant" => Ok("zt"),
        "pt-BR" => Ok("pb"),
        _ => basic_language(language),
    }
}
