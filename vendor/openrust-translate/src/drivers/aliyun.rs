use std::sync::Arc;

use openrust::{
    config::Environment,
    http::client::{Factory, aliyun_rpc::AliyunRpcSigner},
};
use serde::Deserialize;
use serde_json::Value;
use zeroize::Zeroizing;

use super::{Endpoint, basic_language, field, secret, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct Aliyun {
    endpoint: Endpoint,
    access_key_id: Zeroizing<String>,
    access_key_secret: Zeroizing<String>,
    security_token: Option<Zeroizing<String>>,
}

impl Aliyun {
    pub(super) fn new(
        http: Arc<Factory>,
        env: &Environment,
        profile: &Value,
    ) -> TranslationResult<Self> {
        let region = field(profile, "region")?.unwrap_or("cn-hangzhou");
        if !region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(TranslationError::InvalidConfiguration("region"));
        }
        let base = format!("https://alimt.{region}.aliyuncs.com");
        Ok(Self {
            endpoint: Endpoint::new(http, profile, Some(&base), "/")?,
            access_key_id: secret(env, profile, "access_key_id_env")?
                .ok_or(TranslationError::InvalidConfiguration("access_key_id_env"))?,
            access_key_secret: secret(env, profile, "access_key_secret_env")?.ok_or(
                TranslationError::InvalidConfiguration("access_key_secret_env"),
            )?,
            security_token: secret(env, profile, "security_token_env")?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Output {
    code: Value,
    data: Option<Data>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Data {
    translated: String,
    detected_language: Option<String>,
}

impl Translator for Aliyun {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            const MEDIA: &str = "application/x-www-form-urlencoded";
            let source = request
                .source()
                .map(language)
                .transpose()?
                .unwrap_or("auto");
            let target = language(request.target())?;
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("FormatType", "text"),
                    ("Scene", "general"),
                    ("SourceLanguage", source),
                    ("SourceText", request.text()),
                    ("TargetLanguage", target),
                ])
                .finish();
            let headers = AliyunRpcSigner {
                access_key_id: &self.access_key_id,
                access_key_secret: self.access_key_secret.as_bytes(),
                security_token: self.security_token.as_ref().map(|token| token.as_str()),
            }
            .sign_post(
                &self.endpoint.url,
                "TranslateGeneral",
                "2018-10-12",
                Some(MEDIA),
                body.as_bytes(),
            )
            .map_err(|_| TranslationError::InvalidConfiguration("aliyun credentials"))?;
            let response = self.endpoint.post_encoded(headers, body, MEDIA).await?;
            let code = response.status().as_u16();
            if matches!(code, 401 | 402 | 403 | 408 | 429 | 500..=599 | 300..=399) {
                return Err(status(code));
            }
            let output: Output = response.json().map_err(|_| {
                if code == 200 {
                    TranslationError::InvalidResponse
                } else {
                    status(code)
                }
            })?;
            if code != 200 || !(output.code == 200 || output.code == "200") {
                return Err(service_error(&output.code, code));
            }
            let data = output.data.ok_or(TranslationError::InvalidResponse)?;
            let result = Translation::new(data.translated);
            match data.detected_language.as_deref() {
                None | Some("" | "auto") => Ok(result),
                Some("zh") => result.with_detected_source("zh-Hans"),
                Some("zh-tw") => result.with_detected_source("zh-Hant"),
                Some(code) => result.with_detected_source(code),
            }
        })
    }
}

fn language(language: &str) -> TranslationResult<&str> {
    match language {
        "zh-Hans" => Ok("zh"),
        "zh-Hant" => Ok("zh-tw"),
        _ => basic_language(language),
    }
}

fn service_error(code: &Value, http: u16) -> TranslationError {
    let numeric = code.as_u64().or_else(|| code.as_str()?.parse().ok());
    match numeric {
        Some(10001 | 101) => TranslationError::Timeout,
        Some(10005 | 105) => TranslationError::UnsupportedLanguage,
        Some(10009..=10011 | 109..=111) => TranslationError::Authentication,
        Some(10013 | 113) => TranslationError::Quota,
        Some(10003 | 10004 | 10006 | 10008 | 103 | 104 | 106 | 108) => TranslationError::Rejected,
        _ => match code.as_str() {
            Some("Throttling" | "Throttling.User" | "Throttling.Api" | "Throttling.System") => {
                TranslationError::RateLimited
            }
            Some(
                "InvalidAccessKeyId.NotFound"
                | "SignatureDoesNotMatch"
                | "Forbidden.RAM"
                | "InvalidSecurityToken.Expired",
            ) => TranslationError::Authentication,
            _ if http != 200 => status(http),
            _ => TranslationError::Service { status: None },
        },
    }
}
