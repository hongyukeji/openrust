use std::sync::Arc;

use md5::{Digest, Md5};
use openrust::{config::Environment, http::client::Factory};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{Endpoint, basic_language, secret, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct Baidu {
    endpoint: Endpoint,
    app_id: Zeroizing<String>,
    key: Zeroizing<String>,
}

impl Baidu {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        Ok(Self {
            endpoint: Endpoint::new(
                http,
                profile,
                Some("https://fanyi-api.baidu.com"),
                "/api/trans/vip/translate",
            )?,
            app_id: secret(env, profile, "app_id_env")?
                .ok_or(TranslationError::InvalidConfiguration("app_id_env"))?,
            key: secret(env, profile, "api_key_env")?
                .ok_or(TranslationError::InvalidConfiguration("api_key_env"))?,
        })
    }
}

#[derive(Deserialize)]
struct Output {
    from: Option<String>,
    trans_result: Option<Vec<Segment>>,
    error_code: Option<Value>,
}

#[derive(Deserialize)]
struct Segment {
    dst: String,
}

impl Translator for Baidu {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let source = request
                .source()
                .map(language)
                .transpose()?
                .unwrap_or("auto");
            let target = language(request.target())?;
            let salt = fastrand::u64(..).to_string();
            let mut signer = Md5::new();
            for bytes in [
                self.app_id.as_bytes(),
                request.text().as_bytes(),
                salt.as_bytes(),
                self.key.as_bytes(),
            ] {
                signer.update(bytes);
            }
            let sign = hex::encode(signer.finalize());
            let body = json!({"q":request.text(),"from":source,"to":target,
                "appid":self.app_id.as_str(),"salt":salt,"sign":sign});
            let response = self
                .endpoint
                .post(&body, |request| request.as_form())
                .await?;
            let code = response.status().as_u16();
            if code != 200 {
                return Err(status(code));
            }
            let output: Output = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            if let Some(code) = output.error_code {
                return Err(service_error(&code));
            }
            let segments = output
                .trans_result
                .filter(|segments| !segments.is_empty())
                .ok_or(TranslationError::InvalidResponse)?;
            let result = Translation::new(
                segments
                    .into_iter()
                    .map(|segment| segment.dst)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            match output.from.as_deref() {
                None | Some("" | "auto") => Ok(result),
                Some(code) => result.with_detected_source(detected(code)),
            }
        })
    }
}

const VARIANTS: &[(&str, &str)] = &[
    ("zh-Hans", "zh"),
    ("zh-Hant", "cht"),
    ("yue", "yue"),
    ("ja", "jp"),
    ("ko", "kor"),
    ("fr", "fra"),
    ("es", "spa"),
    ("ar", "ara"),
    ("da", "dan"),
    ("vi", "vie"),
    ("sv", "swe"),
    ("bg", "bul"),
    ("et", "est"),
    ("fi", "fin"),
    ("ro", "rom"),
    ("sl", "slo"),
];

fn language(language: &str) -> TranslationResult<&str> {
    if let Some((_, code)) = VARIANTS.iter().find(|(tag, _)| *tag == language) {
        return Ok(code);
    }
    basic_language(language)
}

fn detected(language: &str) -> &str {
    VARIANTS
        .iter()
        .find(|(_, code)| *code == language)
        .map_or(language, |(tag, _)| tag)
}

fn service_error(code: &Value) -> TranslationError {
    match code.as_u64().or_else(|| code.as_str()?.parse().ok()) {
        Some(52001) => TranslationError::Timeout,
        Some(52003 | 54001 | 58000 | 58002 | 90107) => TranslationError::Authentication,
        Some(54004) => TranslationError::Quota,
        Some(54003 | 54005) => TranslationError::RateLimited,
        Some(58001) => TranslationError::UnsupportedLanguage,
        Some(54000 | 58003) => TranslationError::Rejected,
        _ => TranslationError::Service { status: None },
    }
}
