use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use openrust::{
    config::{ConfigError, Environment},
    contracts::config::Repository,
};
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};
use reqwest::{
    Url,
    blocking::Client,
    header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderValue},
};

use crate::invalid;

pub(crate) const EXPORT_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct Destination {
    pub(crate) endpoint: Url,
    license: Option<HeaderValue>,
}

impl Destination {
    pub(crate) fn resolve(config: &dyn Repository, env: &Environment) -> Result<Self, ConfigError> {
        if let Some(value) = config
            .get("new_relic.collector_endpoint")
            .filter(|value| !value.is_null())
        {
            let endpoint = value
                .as_str()
                .and_then(|value| Url::parse(value).ok())
                .filter(|url| {
                    let loopback = match url.host() {
                        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                        _ => false,
                    };
                    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
                        && url.host().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none()
                        && url.path() == "/"
                })
                .ok_or_else(|| {
                    invalid(
                        "new_relic.collector_endpoint",
                        "无凭据、路径和查询参数的 HTTPS 地址，或回环 HTTP 地址",
                    )
                })?;
            return Ok(Self {
                endpoint,
                license: None,
            });
        }
        let region = config.get("new_relic.region");
        let host = match region.as_ref() {
            None => Some("otlp.nr-data.net"),
            Some(value) => match value.as_str() {
                Some("us") => Some("otlp.nr-data.net"),
                Some("eu") => Some("otlp.eu01.nr-data.net"),
                Some("jp") => Some("otlp.jp01.nr-data.net"),
                Some("us-gov") => Some("gov-otlp.nr-data.net"),
                _ => None,
            },
        }
        .ok_or_else(|| invalid("new_relic.region", "us、eu、jp 或 us-gov"))?;
        // 许可证仅在启动时读取，避免写进配置缓存。
        let mut license = env
            .get_utf8("NEW_RELIC_LICENSE_KEY")?
            .filter(|key| !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_graphic()))
            .and_then(|key| HeaderValue::from_str(key).ok())
            .ok_or_else(|| invalid("NEW_RELIC_LICENSE_KEY", "有效的 New Relic 摄取许可证"))?;
        license.set_sensitive(true);
        let endpoint = Url::parse(&format!("https://{host}")).expect("固定的供应商地址");
        Ok(Self {
            endpoint,
            license: Some(license),
        })
    }

    pub(crate) fn client(&self) -> Result<Transport, reqwest::Error> {
        let client = Client::builder()
            .timeout(EXPORT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Transport {
            client,
            license: self.license.clone(),
            stats: Arc::default(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Transport {
    client: Client,
    license: Option<HeaderValue>,
    stats: Arc<Counters>,
}

/// OTLP 出口计数。SDK 重试计为独立请求尝试。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportStats {
    pub attempts: u64,
    pub failures: u64,
}

#[derive(Default)]
struct Counters {
    attempts: AtomicU64,
    failures: AtomicU64,
}

impl Transport {
    pub(crate) fn stats(&self) -> ExportStats {
        ExportStats {
            attempts: self.stats.attempts.load(Ordering::Relaxed),
            failures: self.stats.failures.load(Ordering::Relaxed),
        }
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OtlpTransport")
    }
}

#[async_trait::async_trait]
impl HttpClient for Transport {
    async fn send_bytes(&self, mut request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        // 只允许编码相关头穿过 SDK，避免通用 OTEL 环境头覆盖凭据或收集器配置。
        let headers = std::mem::take(request.headers_mut());
        for name in [CONTENT_TYPE, CONTENT_ENCODING] {
            if let Some(value) = headers.get(&name) {
                request.headers_mut().insert(name, value.clone());
            }
        }
        if let Some(license) = &self.license {
            request.headers_mut().insert("api-key", license.clone());
        }
        self.stats.attempts.fetch_add(1, Ordering::Relaxed);
        let result = self.client.send_bytes(request).await;
        if result.as_ref().is_err()
            || result
                .as_ref()
                .is_ok_and(|response| !response.status().is_success())
        {
            self.stats.failures.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}
