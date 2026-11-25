use std::{sync::Arc, time::Instant};

use google_cloud_auth::credentials::{Builder, CacheableResource, Credentials};
use http::{Extensions, header::AUTHORIZATION};
use openrust::http::client::Factory;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use super::{Endpoint, basic_language, field, status};
use crate::{
    Translation, TranslationError, TranslationFuture, TranslationRequest, TranslationResult,
    Translator,
};

pub(super) struct Google {
    endpoint: Endpoint,
    quota_project: Option<String>,
    credentials: OnceCell<Credentials>,
}

impl Google {
    pub(super) fn new(http: Arc<Factory>, profile: &Value) -> TranslationResult<Self> {
        let project =
            field(profile, "project")?.ok_or(TranslationError::InvalidConfiguration("project"))?;
        let location = field(profile, "location")?.unwrap_or("global");
        for (field, value) in [("project", project), ("location", location)] {
            if !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(TranslationError::InvalidConfiguration(field));
            }
        }
        let endpoint = Endpoint::new(
            http,
            profile,
            Some("https://translation.googleapis.com"),
            &format!("/v3/projects/{project}/locations/{location}:translateText"),
        )?;
        Ok(Self {
            endpoint,
            quota_project: field(profile, "quota_project")?.map(str::to_owned),
            credentials: OnceCell::new(),
        })
    }

    async fn credentials(&self) -> TranslationResult<&Credentials> {
        // 官方凭据会启动令牌刷新任务，在第一次异步翻译时建立。
        self.credentials
            .get_or_try_init(|| async {
                let mut builder = Builder::default()
                    .with_scopes(["https://www.googleapis.com/auth/cloud-translation"]);
                if let Some(project) = &self.quota_project {
                    builder = builder.with_quota_project_id(project);
                }
                builder
                    .build()
                    .map_err(|_| TranslationError::Authentication)
            })
            .await
    }
}

#[derive(Deserialize)]
struct Output {
    translations: [Entry; 1],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    translated_text: String,
    detected_language_code: Option<String>,
}

impl Translator for Google {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            let target = language(request.target())?;
            let mut body = json!({"contents":[request.text()], "targetLanguageCode":target, "mimeType":"text/plain"});
            if let Some(source) = request.source() {
                body["sourceLanguageCode"] = language(source)?.into();
            }
            let started = Instant::now();
            let resource = tokio::time::timeout(self.endpoint.timeout, async {
                self.credentials()
                    .await?
                    .headers(Extensions::new())
                    .await
                    .map_err(|_| TranslationError::Authentication)
            })
            .await
            .map_err(|_| TranslationError::Timeout)??;
            let CacheableResource::New {
                data: mut headers, ..
            } = resource
            else {
                return Err(TranslationError::Authentication);
            };
            headers
                .get_mut(AUTHORIZATION)
                .ok_or(TranslationError::Authentication)?
                .set_sensitive(true);
            let remaining = self.endpoint.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(TranslationError::Timeout);
            }
            let response = self
                .endpoint
                .post(&body, |request| {
                    request.with_headers(headers).timeout(remaining)
                })
                .await?;
            let http = response.status().as_u16();
            if http != 200 {
                let body: Value = response.json().unwrap_or_default();
                return Err(google_error(&body, http));
            }
            let Output {
                translations: [entry],
            } = response
                .json()
                .map_err(|_| TranslationError::InvalidResponse)?;
            let result = Translation::new(entry.translated_text);
            match entry.detected_language_code.as_deref() {
                Some("zh-CN") => result.with_detected_source("zh-Hans"),
                Some("zh-TW") => result.with_detected_source("zh-Hant"),
                Some(code) => result.with_detected_source(code),
                None => Ok(result),
            }
        })
    }
}

fn language(tag: &str) -> TranslationResult<&str> {
    match tag {
        "zh-Hans" => Ok("zh-CN"),
        "zh-Hant" => Ok("zh-TW"),
        _ => basic_language(tag),
    }
}

fn google_error(body: &Value, http: u16) -> TranslationError {
    let error = &body["error"];
    for field in ["details", "errors"] {
        for detail in error[field].as_array().into_iter().flatten() {
            match detail["reason"].as_str() {
                Some("RATE_LIMIT_EXCEEDED" | "rateLimitExceeded" | "userRateLimitExceeded") => {
                    return TranslationError::RateLimited;
                }
                Some(
                    "QUOTA_EXCEEDED" | "BILLING_DISABLED" | "dailyLimitExceeded" | "quotaExceeded",
                ) => {
                    return TranslationError::Quota;
                }
                _ => {}
            }
        }
    }
    status(http)
}
