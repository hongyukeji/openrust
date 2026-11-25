use std::sync::Arc;

use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, basic_language, field, secret, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct Tencent {
    endpoint: Endpoint,
    model: String,
    key: Zeroizing<String>,
}

impl Tencent {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        Ok(Self {
            endpoint: Endpoint::new(
                http,
                profile,
                Some("https://tokenhub.tencentmaas.com"),
                "/v1/api/translations",
            )?,
            model: field(profile, "model")?.unwrap_or("hy-mt3-plus").to_owned(),
            key: secret(env, profile, "api_key_env")?
                .ok_or(TranslationError::InvalidConfiguration("api_key_env"))?,
        })
    }
}

#[derive(Deserialize)]
struct Output {
    choices: [Choice; 1],
    source: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: String,
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

impl Translator for Tencent {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let target = language(request.target())?;
            let mut body =
                json!({"model":self.model,"text":request.text(),"target":target,"stream":false});
            if let Some(source) = request.source() {
                body["source"] = language(source)?.into();
            }
            let response = self
                .endpoint
                .post(&body, |request| request.with_token(&self.key))
                .await?;
            match response.status().as_u16() {
                200 => {}
                451 => return Err(TranslationError::Rejected),
                code => return Err(status(code)),
            }
            let Output {
                choices: [choice],
                source,
            } = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            match choice.finish_reason.as_str() {
                "stop" => {}
                "sensitive" => return Err(TranslationError::Rejected),
                _ => return Err(TranslationError::InvalidResponse),
            }
            let result = Translation::new(choice.message.content);
            match source.as_deref() {
                None | Some("" | "auto") => Ok(result),
                Some("zh") => result.with_detected_source("zh-Hans"),
                Some("zh-TR") => result.with_detected_source("zh-Hant"),
                Some(code) => result.with_detected_source(code),
            }
        })
    }
}

fn language(language: &str) -> TranslationResult<&str> {
    match language {
        "zh-Hans" => Ok("zh"),
        "zh-Hant" => Ok("zh-TR"),
        _ => basic_language(language),
    }
}
