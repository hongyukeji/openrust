mod aliyun;
mod aliyun_video;
mod hive;
mod hive_image;
mod hive_ocr;
mod media;
mod sightengine;
mod sightengine_image;
mod tencent;
mod tencent_image;

use std::{net::IpAddr, sync::Arc, time::Duration};

use http::{HeaderMap, HeaderValue, Method};
use openrust::{
    config::Environment,
    filesystem::FilesystemManager,
    http::client::{Error as HttpError, Factory, Part, PendingRequest, Response},
};
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use crate::{ModerationError, ModerationManager, ModerationResult};

pub(crate) fn register(
    manager: &ModerationManager,
    http: Arc<Factory>,
    env: Arc<Environment>,
    storage: Arc<FilesystemManager>,
) {
    let client = http.clone();
    let environment = env.clone();
    let media = storage.clone();
    manager.extend("aliyun", move |profile| {
        Ok(Arc::new(aliyun::Aliyun::new(
            client.clone(),
            &environment,
            media.clone(),
            profile,
        )?))
    });
    let client = http.clone();
    let environment = env.clone();
    let media = storage.clone();
    manager.extend("sightengine", move |profile| {
        Ok(Arc::new(sightengine::Sightengine::new(
            client.clone(),
            &environment,
            media.clone(),
            profile,
        )?))
    });
    let client = http.clone();
    let environment = env.clone();
    let media = storage.clone();
    manager.extend("hive", move |profile| {
        Ok(Arc::new(hive::Hive::new(
            client.clone(),
            &environment,
            media.clone(),
            profile,
        )?))
    });
    manager.extend("tencent", move |profile| {
        Ok(Arc::new(tencent::Tencent::new(
            http.clone(),
            &env,
            storage.clone(),
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
    fn new(http: Arc<Factory>, profile: &Value, default_base: &str) -> ModerationResult<Self> {
        let base = field(profile, "base_url")?.unwrap_or(default_base);
        let url =
            Url::parse(base).map_err(|_| ModerationError::InvalidConfiguration("base_url"))?;
        let loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
            Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
            Some(url::Host::Domain(_)) => false,
            None => return Err(ModerationError::InvalidConfiguration("base_url")),
        };
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ModerationError::InvalidConfiguration("base_url"));
        }
        let millis = match profile.get("timeout_ms") {
            None => 10_000,
            Some(value) => value
                .as_u64()
                .filter(|ms| (1..=120_000).contains(ms))
                .ok_or(ModerationError::InvalidConfiguration("timeout_ms"))?,
        };
        Ok(Self {
            http,
            url,
            timeout: Duration::from_millis(millis),
        })
    }

    async fn post_encoded(
        &self,
        headers: HeaderMap,
        body: String,
        media_type: &'static str,
    ) -> ModerationResult<Response> {
        self.post(headers, |request| {
            request.with_body(body, HeaderValue::from_static(media_type))
        })
        .await
    }

    async fn post_parts(
        &self,
        headers: HeaderMap,
        parts: Vec<(&'static str, Part)>,
    ) -> ModerationResult<Response> {
        self.post(headers, |request| {
            parts
                .into_iter()
                .fold(request.as_multipart(), |request, (name, part)| {
                    request.attach(name, part)
                })
        })
        .await
    }

    async fn post(
        &self,
        headers: HeaderMap,
        payload: impl FnOnce(PendingRequest) -> PendingRequest,
    ) -> ModerationResult<Response> {
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
                    .max_response_bytes(1024 * 1024)
                    .with_headers(headers);
                payload(request).send(Method::POST, self.url.as_str()).await
            })
            .await
            .map_err(|error| match error {
                HttpError::Timeout => ModerationError::Timeout,
                HttpError::BodyTooLarge => ModerationError::Protocol,
                _ => ModerationError::ServiceUnavailable,
            })
    }
}

fn field<'a>(profile: &'a Value, name: &'static str) -> ModerationResult<Option<&'a str>> {
    match profile.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Err(ModerationError::InvalidConfiguration(name)),
    }
}

fn secret(
    env: &Environment,
    profile: &Value,
    name: &'static str,
) -> ModerationResult<Option<Zeroizing<String>>> {
    field(profile, name)?
        .map(|variable| {
            env.get_utf8(variable)
                .ok()
                .flatten()
                .filter(|value| !value.trim().is_empty())
                .map(|value| Zeroizing::new(value.to_owned()))
                .ok_or(ModerationError::InvalidConfiguration(name))
        })
        .transpose()
}

fn http_status(status: u16) -> ModerationResult<()> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(ModerationError::Unauthorized),
        408 | 504 => Err(ModerationError::Timeout),
        429 => Err(ModerationError::Throttled),
        500..=599 => Err(ModerationError::ServiceUnavailable),
        _ => Err(ModerationError::Rejected),
    }
}
