use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use openrust::support::{Str, facades::Config};
use serde::Deserialize;
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use crate::{
    BackoffStrategy, BoundTarget, Error, Result, Signer, SigningContext, TargetPolicy,
    body::{Payload, encode_json},
    configuration,
    destination::{MAX_URL_BYTES, parse_target},
    endpoints::{Endpoint, EndpointBinding},
    extensions,
};

/// 构造可验证的 Webhook 请求。准备过程只生成不可变快照。
pub struct WebhookCall {
    target: Option<String>,
    uuid: String,
    payload: Payload,
    meta: Value,
    config: CallConfig,
    headers: HeaderMap,
    use_timestamp: bool,
    signing: bool,
    secret: Option<Secret>,
    endpoint: Option<EndpointBinding>,
}

enum Secret {
    Inline(Zeroizing<String>),
    Reference(String),
}

#[derive(Deserialize)]
struct CallConfig {
    queue: Option<String>,
    connection: Option<String>,
    http_verb: String,
    signer: String,
    signature_header_name: String,
    timestamp_header_name: String,
    webhook_id_header_name: Option<String>,
    headers: BTreeMap<String, HeaderInput>,
    timeout_in_seconds: u32,
    tries: u32,
    backoff_strategy: String,
    retryable_status_codes: Option<Vec<u16>>,
    respect_retry_after: bool,
    retry_with_jitter: bool,
    webhook_job: String,
    verify_ssl: bool,
    throw_exception_on_failure: bool,
    proxy: Value,
    tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum HeaderInput {
    One(String),
    Many(Vec<String>),
}

/// 已序列化的记录。正文、签名输入及策略在尝试之间保持稳定。
pub struct PreparedWebhook {
    target: Url,
    signing_url: Url,
    uuid: String,
    method: Method,
    bytes: Vec<u8>,
    meta: Value,
    headers: HeaderMap,
    timestamp_header: Option<HeaderName>,
    signing: Option<(Secret, Arc<dyn Signer>)>,
    backoff: Arc<dyn BackoffStrategy>,
    config: CallConfig,
    endpoint: Option<EndpointBinding>,
}

impl WebhookCall {
    pub fn new() -> Result<Self> {
        Self::from_configuration(&configuration::defaults())
    }

    /// 从当前应用已合并的配置创建。
    pub fn create() -> Result<Self> {
        let config = Config::get("webhook-server")
            .map_err(|_| Error::InvalidConfig("webhook-server"))?
            .ok_or(Error::InvalidConfig("webhook-server"))?;
        Self::from_configuration(&config)
    }

    pub fn from_configuration(config: &Value) -> Result<Self> {
        let mut merged = configuration::defaults();
        for (key, value) in config
            .as_object()
            .ok_or(Error::InvalidConfig("webhook-server"))?
        {
            merged[key] = value.clone();
        }
        let mut config =
            CallConfig::deserialize(merged).map_err(|_| Error::InvalidConfig("webhook-server"))?;
        let mut headers = HeaderMap::new();
        for (name, input) in std::mem::take(&mut config.headers) {
            let name = parse_header("headers", &name)?;
            let values = match &input {
                HeaderInput::One(value) => std::slice::from_ref(value),
                HeaderInput::Many(values) => values,
            };
            for value in values {
                headers.append(
                    name.clone(),
                    value.parse().map_err(|_| Error::InvalidConfig("headers"))?,
                );
            }
        }
        Ok(Self {
            target: None,
            uuid: Str::uuid()
                .map_err(|_| Error::InvalidConfig("uuid"))?
                .to_string(),
            payload: Payload::Json(Value::Array(vec![])),
            meta: Value::Array(vec![]),
            config,
            headers,
            use_timestamp: false,
            signing: true,
            secret: None,
            endpoint: None,
        })
    }

    /// 采用数据库端点当前的目标、签名凭据引用及配置版本。
    pub fn use_endpoint(mut self, endpoint: &Endpoint) -> Self {
        self.target = Some(endpoint.url.clone());
        self.secret = endpoint.credential.clone().map(Secret::Reference);
        self.signing = self.secret.is_some();
        self.endpoint = Some(endpoint.binding.clone());
        self
    }

    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.target = Some(url.into());
        self
    }

    pub fn get_url(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn uuid(mut self, id: impl Into<String>) -> Self {
        self.uuid = id.into();
        self
    }

    pub fn get_uuid(&self) -> &str {
        &self.uuid
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = Payload::Json(payload);
        self
    }

    pub fn send_raw_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.payload = Payload::Raw(body.into());
        self
    }

    pub fn on_queue(mut self, queue: Option<String>) -> Self {
        self.config.queue = queue;
        self
    }

    pub fn on_connection(mut self, connection: Option<String>) -> Self {
        self.config.connection = connection;
        self
    }

    pub fn use_http_verb(mut self, method: Method) -> Self {
        self.config.http_verb = method.as_str().into();
        self
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers.extend(headers);
        self
    }

    pub fn with_webhook_id_header(mut self, header: Option<HeaderName>) -> Self {
        self.config.webhook_id_header_name = header.map(|name| name.to_string());
        self
    }

    pub fn use_timestamp(mut self, enabled: bool) -> Self {
        self.use_timestamp = enabled;
        self
    }

    pub fn use_secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(Secret::Inline(Zeroizing::new(secret.into())));
        self
    }

    pub fn use_secret_reference(mut self, reference: impl Into<String>) -> Self {
        self.secret = Some(Secret::Reference(reference.into()));
        self
    }

    pub fn sign_using(mut self, name: impl Into<String>) -> Self {
        self.config.signer = name.into();
        self
    }

    pub fn do_not_sign(mut self) -> Self {
        self.signing = false;
        self.secret = None;
        self
    }

    pub fn maximum_tries(mut self, tries: u32) -> Self {
        self.config.tries = tries;
        self
    }

    pub fn retry_on(mut self, statuses: Option<Vec<StatusCode>>) -> Self {
        self.config.retryable_status_codes =
            statuses.map(|codes| codes.iter().map(StatusCode::as_u16).collect());
        self
    }

    pub fn respect_retry_after(mut self, enabled: bool) -> Self {
        self.config.respect_retry_after = enabled;
        self
    }

    pub fn with_jitter(mut self, enabled: bool) -> Self {
        self.config.retry_with_jitter = enabled;
        self
    }

    pub fn use_backoff_strategy(mut self, name: impl Into<String>) -> Self {
        self.config.backoff_strategy = name.into();
        self
    }

    pub fn timeout_in_seconds(mut self, seconds: u32) -> Self {
        self.config.timeout_in_seconds = seconds;
        self
    }

    pub fn use_job(mut self, name: impl Into<String>) -> Self {
        self.config.webhook_job = name.into();
        self
    }

    pub fn meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.config.tags = tags;
        self
    }

    pub fn verify_ssl(mut self, verify: bool) -> Self {
        self.config.verify_ssl = verify;
        self
    }

    /// 请求跳过证书和主机名校验。执行时还需应用投递策略明确许可。
    pub fn do_not_verify_ssl(self) -> Self {
        self.verify_ssl(false)
    }

    pub fn throw_exception_on_failure(mut self, enabled: bool) -> Self {
        self.config.throw_exception_on_failure = enabled;
        self
    }

    pub fn prepare(mut self, policy: &TargetPolicy) -> Result<PreparedWebhook> {
        let url = parse_target(self.target.as_deref().ok_or(Error::InvalidConfig("url"))?)?;
        policy.validate_target(&url)?;
        validate_message_id(&self.uuid)?;
        let method = self.config.validate(&self.meta)?;
        let (bytes, query) = self.payload.prepare(method == Method::GET)?;
        let mut target = url.clone();
        if let Some(query) = query {
            target.set_query((!query.is_empty()).then_some(query.as_str()));
        }
        if target.as_str().len() > MAX_URL_BYTES {
            return Err(Error::TargetDenied);
        }
        if let Some(name) = &self.config.webhook_id_header_name {
            let name = parse_header("webhook_id_header_name", name)?;
            if !self.headers.contains_key(&name) {
                self.headers.insert(
                    name,
                    self.uuid
                        .parse()
                        .map_err(|_| Error::InvalidConfig("uuid"))?,
                );
            }
        }
        validate_headers(&self.headers)?;
        let signing = if self.signing {
            let secret = self.secret.ok_or(Error::InvalidConfig("signing_secret"))?;
            match &secret {
                Secret::Inline(value) => validate_secret(value)?,
                Secret::Reference(name) => {
                    extensions::validate_name("signing_secret_reference", name)?
                }
            }
            let signer =
                extensions::signer(&self.config.signer, &self.config.signature_header_name)?;
            Some((secret, signer))
        } else {
            None
        };
        let timestamp_header = if self.use_timestamp {
            Some(parse_header(
                "timestamp_header_name",
                &self.config.timestamp_header_name,
            )?)
        } else {
            None
        };
        let backoff = extensions::backoff(&self.config.backoff_strategy)?;
        Ok(PreparedWebhook {
            target,
            signing_url: url,
            uuid: self.uuid,
            method,
            bytes,
            meta: self.meta,
            headers: self.headers,
            timestamp_header,
            signing,
            backoff,
            config: self.config,
            endpoint: self.endpoint,
        })
    }
}

impl CallConfig {
    fn validate(&self, meta: &Value) -> Result<Method> {
        let method = Method::from_bytes(self.http_verb.to_ascii_uppercase().as_bytes())
            .map_err(|_| Error::InvalidConfig("http_verb"))?;
        if method == Method::CONNECT {
            return Err(Error::InvalidConfig("http_verb"));
        }
        if !(1..=120).contains(&self.timeout_in_seconds) || !(1..=100).contains(&self.tries) {
            return Err(Error::InvalidConfig("timeout_in_seconds/tries"));
        }
        if self.retryable_status_codes.as_ref().is_some_and(|codes| {
            codes.len() > 500 || codes.iter().any(|code| !(100..=599).contains(code))
        }) {
            return Err(Error::InvalidConfig("retryable_status_codes"));
        }
        if !self.proxy.is_null() {
            return Err(Error::InvalidConfig("proxy"));
        }
        for (field, name) in [
            ("webhook_job", Some(self.webhook_job.as_str())),
            ("queue", self.queue.as_deref()),
            ("connection", self.connection.as_deref()),
        ] {
            if let Some(name) = name {
                extensions::validate_name(field, name)?;
            }
        }
        if self.tags.len() > 64
            || self
                .tags
                .iter()
                .any(|tag| tag.len() > 128 || tag.chars().any(char::is_control))
            || !matches!(meta, Value::Object(_) | Value::Array(_))
            || encode_json(meta, 64 * 1024).is_err()
        {
            return Err(Error::InvalidConfig("meta/tags"));
        }
        Ok(method)
    }
}

impl PreparedWebhook {
    pub fn get_uuid(&self) -> &str {
        &self.uuid
    }
    pub fn get_url(&self) -> &str {
        self.target.as_str()
    }
    pub fn method(&self) -> &Method {
        &self.method
    }
    pub fn body(&self) -> &[u8] {
        if self.method == Method::GET {
            &[]
        } else {
            &self.bytes
        }
    }
    pub fn signing_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn queue(&self) -> Option<&str> {
        self.config.queue.as_deref()
    }
    pub fn connection(&self) -> Option<&str> {
        self.config.connection.as_deref()
    }
    pub fn job(&self) -> &str {
        &self.config.webhook_job
    }
    pub fn meta(&self) -> &Value {
        &self.meta
    }
    pub fn tags(&self) -> &[String] {
        &self.config.tags
    }
    pub fn tries(&self) -> u32 {
        self.config.tries
    }
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.timeout_in_seconds.into())
    }
    pub fn retries_status(&self, status: StatusCode) -> bool {
        !status.is_success()
            && self
                .config
                .retryable_status_codes
                .as_ref()
                .is_none_or(|codes| codes.contains(&status.as_u16()))
    }
    pub fn respects_retry_after(&self) -> bool {
        self.config.respect_retry_after
    }
    pub fn uses_jitter(&self) -> bool {
        self.config.retry_with_jitter
    }
    pub fn throws_on_failure(&self) -> bool {
        self.config.throw_exception_on_failure
    }
    pub fn backoff_seconds(&self, attempt: u32) -> u64 {
        self.backoff.wait_in_seconds_after_attempt(attempt)
    }
    pub(crate) fn freeze(&self, lifetime: u32) -> Result<crate::snapshot::Snapshot> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        if matches!(self.signing, Some((Secret::Inline(_), _))) {
            return Err(Error::InvalidConfig("signing_secret_reference"));
        }
        if self.headers.iter().any(|(name, value)| {
            value.is_sensitive() || matches!(*name, header::AUTHORIZATION | header::COOKIE)
        }) {
            return Err(Error::InvalidConfig("persistent_headers"));
        }
        Ok(crate::snapshot::Snapshot {
            version: 1,
            uuid: self.uuid.clone(),
            url: self.target.as_str().into(),
            signing_url: self.signing_url.as_str().into(),
            method: self.method.as_str().into(),
            body: STANDARD.encode(&self.bytes),
            headers: self
                .headers
                .iter()
                .map(|(name, value)| (name.as_str().into(), STANDARD.encode(value.as_bytes())))
                .collect(),
            credential: self.credential_reference().map(str::to_owned),
            signer: self.config.signer.clone(),
            signature_header: self.config.signature_header_name.clone(),
            timestamp_header: self.timestamp_header.as_ref().map(HeaderName::to_string),
            timeout: self.config.timeout_in_seconds,
            tries: self.config.tries,
            retryable: self.config.retryable_status_codes.clone(),
            backoff: self.config.backoff_strategy.clone(),
            respect_retry_after: self.config.respect_retry_after,
            jitter: self.config.retry_with_jitter,
            throws_on_failure: self.config.throw_exception_on_failure,
            queue: self.config.queue.clone(),
            connection: self.config.connection.clone(),
            job: self.config.webhook_job.clone(),
            custom_job: extensions::has_call_webhook_job(&self.config.webhook_job),
            tags: self.config.tags.clone(),
            meta: self.meta.clone(),
            lifetime,
            replay: false,
            endpoint: self.endpoint.clone(),
            message_ref: None,
            hydrated_body: None,
            unverified_tls: !self.config.verify_ssl,
        })
    }

    pub fn credential_reference(&self) -> Option<&str> {
        match &self.signing {
            Some((Secret::Reference(name), _)) => Some(name),
            _ => None,
        }
    }

    pub fn bind_target(
        &self,
        policy: &TargetPolicy,
        addresses: Vec<SocketAddr>,
    ) -> Result<BoundTarget> {
        policy.bind(self.get_url(), addresses)
    }

    pub fn headers_at(&self, timestamp: i64, resolved_secret: Option<&str>) -> Result<HeaderMap> {
        let mut headers = self.headers.clone();
        if let Some((secret, signer)) = &self.signing {
            let secret = match secret {
                Secret::Inline(secret) => secret.as_str(),
                Secret::Reference(_) => {
                    resolved_secret.ok_or(Error::InvalidConfig("signing_secret"))?
                }
            };
            validate_secret(secret)?;
            let mut signed = signer.headers(&SigningContext {
                webhook_url: self.signing_url.as_str(),
                body: &self.bytes,
                secret,
                message_id: &self.uuid,
                timestamp,
            })?;
            for value in signed.values_mut() {
                value.set_sensitive(true);
            }
            headers.extend(signed);
        }
        if let Some(name) = &self.timestamp_header {
            headers.insert(name.clone(), HeaderValue::from(timestamp));
        }
        validate_headers(&headers)?;
        Ok(headers)
    }
}

fn parse_header(field: &'static str, name: &str) -> Result<HeaderName> {
    HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::InvalidConfig(field))
}

pub(crate) fn validate_secret(value: &str) -> Result<()> {
    if value.is_empty() || value == "0" || value.len() > 8192 {
        return Err(Error::InvalidConfig("signing_secret"));
    }
    Ok(())
}

fn validate_message_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 256
        || !id.is_ascii()
        || id.trim() != id
        || id.chars().any(char::is_control)
    {
        return Err(Error::InvalidConfig("uuid"));
    }
    Ok(())
}

pub(crate) fn validate_headers(headers: &HeaderMap) -> Result<()> {
    let bytes = headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
        .fold(0usize, usize::saturating_add);
    if headers.len() > 128
        || bytes > 64 * 1024
        || headers.keys().any(|name| {
            matches!(
                *name,
                header::HOST
                    | header::CONTENT_LENGTH
                    | header::TRANSFER_ENCODING
                    | header::CONNECTION
                    | header::UPGRADE
                    | header::PROXY_AUTHORIZATION
            )
        })
    {
        return Err(Error::InvalidConfig("headers"));
    }
    Ok(())
}
