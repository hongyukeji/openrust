mod deepl;
mod libretranslate;

use std::{net::IpAddr, sync::Arc, time::Duration};

use http::HeaderValue;
use openrust::{
    config::Environment,
    http::client::{Error as HttpError, Factory, PendingRequest, Response},
};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::{TranslateManager, TranslationError, TranslationResult};

pub(crate) fn register(manager: &TranslateManager, http: Arc<Factory>, env: Arc<Environment>) {
    let client = http.clone();
    let environment = env.clone();
    manager.extend("deepl", move |profile| {
        Ok(Arc::new(deepl::DeepL::new(
            client.clone(),
            &environment,
            profile,
        )?))
    });
    manager.extend("libretranslate", move |profile| {
        Ok(Arc::new(libretranslate::LibreTranslate::new(
            http.clone(),
            &env,
            profile,
        )?))
    });
}

struct Endpoint {
    http: Arc<Factory>,
    url: Url,
    timeout: Duration,
}

impl Endpoint {
    fn new(
        http: Arc<Factory>,
        profile: &Value,
        default_base: Option<&str>,
        path: &str,
    ) -> TranslationResult<Self> {
        let base = field(profile, "base_url")?
            .or(default_base)
            .ok_or(TranslationError::InvalidConfiguration("base_url"))?;
        let mut url =
            Url::parse(base).map_err(|_| TranslationError::InvalidConfiguration("base_url"))?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(TranslationError::InvalidConfiguration("base_url"));
        }
        let Some(host) = url.host() else {
            return Err(TranslationError::InvalidConfiguration("base_url"));
        };
        let loopback = match host {
            url::Host::Domain(host) => host == "localhost",
            url::Host::Ipv4(ip) => IpAddr::V4(ip).is_loopback(),
            url::Host::Ipv6(ip) => IpAddr::V6(ip).is_loopback(),
        };
        if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
            return Err(TranslationError::InvalidConfiguration("base_url"));
        }
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| TranslationError::InvalidConfiguration("base_url"))?;
        segments
            .pop_if_empty()
            .extend(path.trim_start_matches('/').split('/'));
        drop(segments);
        let millis = match profile.get("timeout_ms") {
            None => 10_000,
            Some(value) => value
                .as_u64()
                .filter(|ms| *ms > 0)
                .ok_or(TranslationError::InvalidConfiguration("timeout_ms"))?,
        };
        Ok(Self {
            http,
            url,
            timeout: Duration::from_millis(millis),
        })
    }

    async fn post(
        &self,
        body: &(impl Serialize + ?Sized),
        configure: impl FnOnce(PendingRequest) -> PendingRequest,
    ) -> TranslationResult<Response> {
        self.http
            .without_global_configuration_async(async {
                let request = self
                    .http
                    .pending_request()
                    .accept_json()
                    .redact_payload()
                    .without_redirecting()
                    .connect_timeout(self.timeout)
                    .timeout(self.timeout)
                    .max_response_bytes(1024 * 1024);
                configure(request).post(self.url.as_str(), body).await
            })
            .await
            .map_err(|error| match error {
                HttpError::Timeout => TranslationError::Timeout,
                HttpError::BodyTooLarge | HttpError::InvalidJson(_) => {
                    TranslationError::InvalidResponse
                }
                _ => TranslationError::Transport,
            })
    }
}

fn field<'a>(profile: &'a Value, name: &'static str) -> TranslationResult<Option<&'a str>> {
    match profile.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Err(TranslationError::InvalidConfiguration(name)),
    }
}

fn secret(
    env: &Environment,
    profile: &Value,
    name: &'static str,
) -> TranslationResult<Option<zeroize::Zeroizing<String>>> {
    field(profile, name)?
        .map(|variable| {
            env.get_utf8(variable)
                .ok()
                .flatten()
                .filter(|value| !value.trim().is_empty())
                .map(|value| zeroize::Zeroizing::new(value.to_owned()))
                .ok_or(TranslationError::InvalidConfiguration(name))
        })
        .transpose()
}

fn sensitive_header(value: &str, field: &'static str) -> TranslationResult<HeaderValue> {
    let mut value =
        HeaderValue::from_str(value).map_err(|_| TranslationError::InvalidConfiguration(field))?;
    value.set_sensitive(true);
    Ok(value)
}

fn status(status: u16) -> TranslationError {
    match status {
        401 | 403 => TranslationError::Authentication,
        402 => TranslationError::Quota,
        408 | 504 => TranslationError::Timeout,
        429 => TranslationError::RateLimited,
        400 | 404 | 413 | 422 => TranslationError::Rejected,
        _ => TranslationError::Service {
            status: Some(status),
        },
    }
}

fn basic_language(language: &str) -> TranslationResult<&str> {
    if (2..=3).contains(&language.len()) && language.bytes().all(|byte| byte.is_ascii_lowercase()) {
        Ok(language)
    } else {
        Err(TranslationError::UnsupportedLanguage)
    }
}
