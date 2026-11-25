use std::sync::Arc;

use http::{HeaderValue, header::AUTHORIZATION};
use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Endpoint, basic_language, field, secret, sensitive_header, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct DeepL {
    endpoint: Endpoint,
    authorization: HeaderValue,
}

impl DeepL {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        let key = secret(env, profile, "api_key_env")?
            .ok_or(TranslationError::InvalidConfiguration("api_key_env"))?;
        let base = match field(profile, "plan")? {
            Some("pro") => "https://api.deepl.com",
            None | Some("free") => "https://api-free.deepl.com",
            _ => return Err(TranslationError::InvalidConfiguration("plan")),
        };
        let header = zeroize::Zeroizing::new(format!("DeepL-Auth-Key {}", key.as_str()));
        Ok(Self {
            endpoint: Endpoint::new(http, profile, Some(base), "/v2/translate")?,
            authorization: sensitive_header(&header, "api_key_env")?,
        })
    }
}

#[derive(Deserialize)]
struct Output {
    translations: [Entry; 1],
}

#[derive(Deserialize)]
struct Entry {
    text: String,
    detected_source_language: Option<String>,
}

impl Translator for DeepL {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let target = language(request.target(), true)?;
            let mut body = json!({"text":[request.text()], "target_lang":target});
            if let Some(source) = request.source() {
                body["source_lang"] = language(source, false)?.into();
            }
            let response = self
                .endpoint
                .post(&body, |req| {
                    req.with_header(AUTHORIZATION, self.authorization.clone())
                })
                .await?;
            match response.status().as_u16() {
                200 => {}
                456 => return Err(TranslationError::Quota),
                code => return Err(status(code)),
            }
            let Output {
                translations: [entry],
            } = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            let result = Translation::new(entry.text);
            match entry.detected_source_language {
                Some(source) => result.with_detected_source(&source),
                None => Ok(result),
            }
        })
    }
}

fn language(language: &str, target: bool) -> TranslationResult<String> {
    match language {
        "en-US" | "en-GB" | "pt-BR" | "pt-PT" | "es-419" | "zh-Hans" | "zh-Hant" if target => {
            Ok(language.to_ascii_uppercase())
        }
        _ => Ok(basic_language(language)?.to_ascii_uppercase()),
    }
}
