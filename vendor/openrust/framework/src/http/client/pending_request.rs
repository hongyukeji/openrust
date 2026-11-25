use std::{collections::BTreeMap, num::NonZeroUsize, sync::Arc, time::Duration};

use http::{HeaderMap, HeaderValue, Method, header};
use serde::Serialize;

use super::factory::{
    BODY_LIMIT, CONNECT_TIMEOUT, TIMEOUT, build_client, build_client_with_socket,
    build_pinned_client,
};
use super::transport::Transport;
use super::uri_template;
use super::{
    BatchResults, Error, Factory, Form, Pool, RecordedMultipartField, RecordedRequestDataSource,
    Response, Result,
};
use super::{destination::PinnedDestination, digest::DigestCredentials};

pub(super) type RequestMiddleware = dyn Fn(&mut reqwest::Request) -> Result<()> + Send + Sync;
type ResponseMiddleware = dyn Fn(Response) -> Result<Response> + Send + Sync;
type ThrowCondition = dyn Fn(&Response) -> bool + Send + Sync;
type RetryCondition = dyn Fn(&Error) -> bool + Send + Sync;
type RetryRequestCondition = dyn Fn(&Error, &mut reqwest::Request) -> Result<bool> + Send + Sync;

enum RetryWhen {
    Inspect(Arc<RetryCondition>),
    Modify(Box<RetryRequestCondition>),
}

impl RetryWhen {
    fn permits(&self, error: &Error, next: Option<&mut reqwest::Request>) -> Result<bool> {
        match self {
            Self::Inspect(condition) => Ok(condition(error)),
            Self::Modify(condition) => next.map_or(Ok(false), |request| condition(error, request)),
        }
    }
}

#[derive(Default)]
struct Retry {
    delays: RetryDelays,
    when: Option<RetryWhen>,
    throw: bool,
}

#[derive(Default)]
enum RetryDelays {
    #[default]
    None,
    Fixed {
        remaining: usize,
        delay: Duration,
    },
    Backoff(std::vec::IntoIter<Duration>),
}

impl RetryDelays {
    fn len(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Fixed { remaining, .. } => *remaining,
            Self::Backoff(delays) => delays.len(),
        }
    }

    fn next(&mut self) -> Option<Duration> {
        match self {
            Self::Fixed { remaining, delay } if *remaining > 0 => {
                *remaining -= 1;
                Some(*delay)
            }
            Self::Backoff(delays) => delays.next(),
            _ => None,
        }
    }
}

enum Payload {
    Json,
    Form,
    Raw(reqwest::Body),
    Multipart(Vec<Attachment>),
}

struct Attachment {
    name: String,
    part: super::Part,
    metadata: RecordedMultipartField,
}

/// Builder for one outbound request.
pub struct PendingRequest {
    factory: Factory,
    middleware: Vec<Arc<super::Middleware>>,
    base_url: Option<String>,
    headers: HeaderMap,
    invalid_header: bool,
    digest: Option<DigestCredentials>,
    query: indexmap::IndexMap<String, String>,
    uri_parameters: serde_json::Map<String, serde_json::Value>,
    attributes: serde_json::Map<String, serde_json::Value>,
    redact_payload: bool,
    redact_response: bool,
    connect_timeout: Duration,
    verify_tls: bool,
    tls_ca: Option<Vec<super::TlsCertificate>>,
    tls_identity: Option<super::TlsIdentity>,
    pinned_proxy: Option<super::PinnedProxy>,
    unix_socket: Option<std::path::PathBuf>,
    destination: Option<PinnedDestination>,
    timeout: Duration,
    redirects: Option<usize>,
    sink: Option<super::Sink>,
    body_limit: usize,
    payload: Payload,
    retry: Retry,
    throw_condition: Option<Box<ThrowCondition>>,
    before_sending: Vec<Box<RequestMiddleware>>,
    after_response: Vec<Box<ResponseMiddleware>>,
}

impl PendingRequest {
    pub(super) fn new(factory: Factory) -> Self {
        let middleware = factory.middleware();
        Self {
            factory,
            middleware,
            base_url: None,
            headers: HeaderMap::new(),
            invalid_header: false,
            digest: None,
            query: indexmap::IndexMap::new(),
            uri_parameters: Default::default(),
            attributes: serde_json::Map::new(),
            redact_payload: false,
            redact_response: false,
            connect_timeout: CONNECT_TIMEOUT,
            verify_tls: true,
            tls_ca: None,
            tls_identity: None,
            pinned_proxy: None,
            unix_socket: None,
            destination: None,
            timeout: TIMEOUT,
            redirects: Some(5),
            sink: None,
            body_limit: BODY_LIMIT,
            payload: Payload::Json,
            retry: Retry::default(),
            throw_condition: None,
            before_sending: Vec::new(),
            after_response: Vec::new(),
        }
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        for name in headers.keys() {
            for value in headers.get_all(name) {
                self.headers.append(name, value.clone());
            }
        }
        self
    }

    pub fn replace_headers(mut self, headers: HeaderMap) -> Self {
        for name in headers.keys() {
            self.headers.remove(name);
            for value in headers.get_all(name) {
                self.headers.append(name, value.clone());
            }
        }
        self
    }

    pub fn with_header(mut self, name: http::HeaderName, value: HeaderValue) -> Self {
        self.headers.append(name, value);
        self
    }

    pub fn content_type(mut self, content_type: HeaderValue) -> Self {
        self.headers.insert(header::CONTENT_TYPE, content_type);
        self
    }

    pub fn as_multipart(mut self) -> Self {
        if !matches!(self.payload, Payload::Multipart(_)) {
            self.payload = Payload::Multipart(Vec::new());
        }
        self
    }

    pub fn max_redirects(mut self, maximum: usize) -> Self {
        self.redirects = Some(maximum);
        self
    }

    /// 将已受响应大小限制的响应保存到文件；文件错误保留原始原因。
    pub fn sink(mut self, path: impl Into<super::Sink>) -> Self {
        self.sink = Some(path.into());
        self
    }

    pub fn accept(mut self, content_type: HeaderValue) -> Self {
        self.headers.insert(header::ACCEPT, content_type);
        self
    }

    pub fn accept_json(self) -> Self {
        self.accept(HeaderValue::from_static("application/json"))
    }

    pub fn with_token(self, token: &str) -> Self {
        self.authorization(format!("Bearer {token}"))
    }

    /// 设置请求标识，按协议约定去除首尾 ASCII 空白。
    pub fn with_user_agent(mut self, user_agent: &str) -> Self {
        let value = user_agent.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']);
        match HeaderValue::from_str(value) {
            Ok(value) => {
                self.headers.insert(header::USER_AGENT, value);
            }
            Err(_) => self.invalid_header = true,
        }
        self
    }

    /// 只对当前请求关闭 TLS 证书及主机名校验。
    pub fn without_verifying(mut self) -> Self {
        self.verify_tls = false;
        self
    }

    /// 使用指定 CA 取代系统信任库，只用于固定地址的 HTTPS 请求。
    pub fn with_tls_ca(mut self, certificates: Vec<super::TlsCertificate>) -> Self {
        self.tls_ca = Some(certificates);
        self
    }

    /// 仅向固定地址的 HTTPS 对端出示此客户端证书。
    pub fn with_tls_identity(mut self, identity: super::TlsIdentity) -> Self {
        self.tls_identity = Some(identity);
        self
    }

    /// 仅在固定地址且启用证书及主机名校验的 HTTPS 连接上使用显式 SOCKS5。
    pub fn through_pinned_socks_proxy(self, proxy: super::PinnedSocksProxy) -> Self {
        self.through_pinned_proxy(proxy)
    }

    /// 仅在固定地址且启用证书及主机名校验的 HTTPS 连接上使用具名代理。
    pub fn through_pinned_proxy(mut self, proxy: impl Into<super::PinnedProxy>) -> Self {
        self.pinned_proxy = Some(proxy.into());
        self
    }

    pub fn with_basic_auth(self, username: &str, password: &str) -> Self {
        use base64::{Engine, engine::general_purpose::STANDARD};
        self.authorization(format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        ))
    }

    pub fn with_digest_auth(mut self, username: &str, password: &str) -> Self {
        self.headers.remove(header::AUTHORIZATION);
        self.digest = Some(DigestCredentials {
            username: username.to_owned(),
            password: password.to_owned(),
        });
        self
    }

    fn authorization(mut self, value: String) -> Self {
        self.digest = None;
        if let Ok(mut value) = HeaderValue::from_str(&value) {
            value.set_sensitive(true);
            self.headers.insert(header::AUTHORIZATION, value);
        } else {
            self.invalid_header = true;
        }
        self
    }

    pub fn with_url_parameters(
        mut self,
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.uri_parameters.extend(parameters);
        self
    }

    /// Omits request and response bodies from HTTP event observations.
    pub fn redact_payload(mut self) -> Self {
        self.redact_response = true;
        self.redact_request_payload_only()
    }

    pub(crate) fn redact_request_payload_only(mut self) -> Self {
        self.redact_payload = true;
        self
    }

    pub fn with_query_parameters(
        mut self,
        parameters: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.query.extend(parameters);
        self
    }

    pub fn as_form(mut self) -> Self {
        self.payload = Payload::Form;
        self
    }

    pub fn as_json(mut self) -> Self {
        self.payload = Payload::Json;
        self
    }

    pub fn with_body(mut self, body: impl Into<reqwest::Body>, content_type: HeaderValue) -> Self {
        self.payload = Payload::Raw(body.into());
        self.headers.insert(header::CONTENT_TYPE, content_type);
        self
    }

    pub fn attach(self, name: impl Into<String>, part: super::Part) -> Self {
        let field = part
            .buffered_field_bytes()
            .and_then(|bytes| std::str::from_utf8(bytes).ok());
        let metadata = if let Some(field) = field {
            RecordedMultipartField::Value(field.into())
        } else {
            let mut headers = HeaderMap::new();
            if let Some(mime) = part.mime_type().and_then(|mime| mime.parse().ok()) {
                headers.insert(header::CONTENT_TYPE, mime);
            }
            for (name, value) in part.custom_headers() {
                headers.append(name, value.clone());
            }
            RecordedMultipartField::Attachment {
                filename: part.filename().map(str::to_owned),
                length: part.content_length(),
                headers,
            }
        };
        self.add_multipart_part(name.into(), part, metadata)
    }

    /// 以串流发送常规文件。跳转或重试时从同一文件句柄开头重新读取。
    pub async fn attach_file(
        self,
        name: impl Into<String>,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let metadata = tokio::fs::metadata(path).await.map_err(Error::Attachment)?;
        if !metadata.is_file() {
            return Err(Error::Attachment(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP 附件必须是常规文件",
            )));
        }
        let file = tokio::fs::File::open(path)
            .await
            .map_err(Error::Attachment)?;
        let length = file.metadata().await.map_err(Error::Attachment)?.len();
        let filename = path.file_name().map(|name| name.to_string_lossy());
        let content_type = mime_guess::from_path(path).first_or_octet_stream();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type.as_ref()).map_err(|_| Error::InvalidRequest)?,
        );
        Ok(self.attach_body(
            name,
            reqwest::Body::from_seekable_file(file, length),
            filename.as_deref(),
            headers,
            Some(length),
        ))
    }

    /// 附加缓冲正文或串流。未知长度传入 `None`，缓冲正文的长度会自动取得。
    pub fn attach_body(
        self,
        name: impl Into<String>,
        body: impl Into<reqwest::Body>,
        filename: Option<&str>,
        headers: HeaderMap,
        length: Option<u64>,
    ) -> Self {
        let body = body.into();
        let length = body.as_bytes().map(|bytes| bytes.len() as u64).or(length);
        let mut part = match length {
            Some(length) => super::Part::stream_with_length(body, length),
            None => super::Part::stream(body),
        };
        let filename = filename.map(str::to_owned);
        if let Some(filename) = &filename {
            part = part.file_name(filename.clone());
        }
        self.add_multipart_part(
            name.into(),
            part.headers(headers.clone()),
            RecordedMultipartField::Attachment {
                filename,
                length,
                headers,
            },
        )
    }

    fn add_multipart_part(
        mut self,
        name: String,
        part: super::Part,
        metadata: RecordedMultipartField,
    ) -> Self {
        let mut parts = match self.payload {
            Payload::Multipart(parts) => parts,
            _ => Vec::new(),
        };
        parts.push(Attachment {
            name,
            part,
            metadata,
        });
        self.payload = Payload::Multipart(parts);
        self
    }

    pub fn with_attributes(
        mut self,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.attributes.extend(attributes);
        self
    }

    pub fn with_middleware(
        mut self,
        middleware: impl for<'a> Fn(
            reqwest::Request,
            super::Next<'a>,
        ) -> crate::http::HttpFuture<'a, Result<Response>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.middleware.push(Arc::new(middleware));
        self
    }

    pub fn with_request_middleware(
        self,
        middleware: impl Fn(&mut reqwest::Request) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.with_middleware(move |mut request, next| match middleware(&mut request) {
            Ok(()) => next.run(request),
            Err(error) => Box::pin(async move { Err(error) }),
        })
    }

    pub fn with_response_middleware(
        self,
        middleware: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        let middleware = Arc::new(middleware);
        self.with_middleware(move |request, next| {
            let middleware = middleware.clone();
            Box::pin(async move {
                let response = next.run(request).await?;
                let stats = response.transfer_stats.clone();
                let mut response = middleware(response)?;
                if response.transfer_stats.is_none() {
                    response.transfer_stats = stats;
                }
                Ok(response)
            })
        })
    }

    pub fn before_sending(
        mut self,
        callback: impl Fn(&mut reqwest::Request) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.before_sending.push(Box::new(callback));
        self
    }

    pub fn after_response(
        mut self,
        callback: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.after_response.push(Box::new(callback));
        self
    }

    /// 指定HTTP请求的Unix套接字传输；不改变工厂或其他请求。
    pub fn unix_socket(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.unix_socket = Some(path.into());
        self
    }
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = duration;
        self
    }

    /// 将请求固定到调用者已验证的 Socket 地址，保留 URL 的主机名和 TLS 校验。
    /// 固定请求禁止重定向、自定义 Host 和 Unix socket。每次实际发送都会复核目的地。
    pub fn pin_destination(
        mut self,
        url: &str,
        addresses: Vec<std::net::SocketAddr>,
    ) -> Result<Self> {
        self.destination = Some(PinnedDestination::new(url, addresses)?);
        Ok(self)
    }

    pub fn without_redirecting(mut self) -> Self {
        self.redirects = None;
        self
    }

    pub fn max_response_bytes(mut self, limit: usize) -> Self {
        self.body_limit = limit;
        self
    }

    pub fn retry(mut self, attempts: usize, delay: Duration) -> Self {
        self.retry.delays = RetryDelays::Fixed {
            remaining: attempts.saturating_sub(1),
            delay,
        };
        self.retry.throw = true;
        self
    }

    pub fn retry_delays(mut self, delays: impl Into<Vec<Duration>>) -> Self {
        self.retry.delays = RetryDelays::Backoff(delays.into().into_iter());
        self.retry.throw = true;
        self
    }

    pub fn retry_when(mut self, when: impl Fn(&Error) -> bool + Send + Sync + 'static) -> Self {
        self.retry.when = Some(RetryWhen::Inspect(Arc::new(when)));
        self
    }

    /// Decides whether to retry and configures the request that will be replayed.
    pub fn retry_when_request(
        mut self,
        when: impl Fn(&Error, &mut reqwest::Request) -> Result<bool> + Send + Sync + 'static,
    ) -> Self {
        self.retry.when = Some(RetryWhen::Modify(Box::new(when)));
        self
    }

    pub fn throw_on_retry_failure(mut self, throw: bool) -> Self {
        self.retry.throw = throw;
        self
    }

    pub fn throw(self) -> Self {
        self.throw_if(|_| true)
    }

    pub fn throw_if(
        mut self,
        condition: impl Fn(&Response) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.throw_condition = Some(Box::new(condition));
        self
    }

    pub async fn get(self, url: &str) -> Result<Response> {
        self.send(Method::GET, url).await
    }

    pub async fn head(self, url: &str) -> Result<Response> {
        self.send(Method::HEAD, url).await
    }

    pub async fn post(self, url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        self.execute(Method::POST, url, Some(data)).await
    }

    pub async fn put(self, url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        self.execute(Method::PUT, url, Some(data)).await
    }

    pub async fn patch(self, url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        self.execute(Method::PATCH, url, Some(data)).await
    }

    pub async fn delete(self, url: &str, data: &(impl Serialize + ?Sized)) -> Result<Response> {
        self.execute(Method::DELETE, url, Some(data)).await
    }

    pub async fn send(self, method: Method, url: &str) -> Result<Response> {
        self.execute(method, url, None::<&()>).await
    }

    pub async fn pool<'a>(
        &self,
        concurrency: NonZeroUsize,
        build: impl FnOnce(&mut Pool<'a>),
    ) -> BatchResults {
        self.factory.pool(concurrency, build).await
    }

    pub async fn pool_ordered<'a>(&self, build: impl FnOnce(&mut Pool<'a>)) -> BatchResults {
        self.factory.pool_ordered(build).await
    }

    pub fn batch<'a>(&self, build: impl FnOnce(&mut super::Batch<'a>)) -> super::Batch<'a> {
        self.factory.batch(build)
    }

    async fn execute(
        self,
        method: Method,
        url: &str,
        data: Option<&(impl Serialize + ?Sized)>,
    ) -> Result<Response> {
        if self.invalid_header {
            return Err(Error::InvalidRequest);
        }
        if self.timeout.is_zero() || self.connect_timeout.is_zero() {
            return Err(Error::Configuration);
        }
        if (self.tls_ca.is_some() || self.tls_identity.is_some() || self.pinned_proxy.is_some())
            && (!self.verify_tls
                || self.tls_ca.as_ref().is_some_and(Vec::is_empty)
                || !self
                    .destination
                    .as_ref()
                    .is_some_and(PinnedDestination::is_https))
        {
            return Err(Error::Configuration);
        }
        let client = if let Some(destination) = &self.destination {
            if self.unix_socket.is_some() {
                return Err(Error::Configuration);
            }
            build_pinned_client(
                self.connect_timeout,
                self.verify_tls,
                destination,
                self.tls_ca,
                self.tls_identity,
                self.pinned_proxy,
            )?
        } else if self.unix_socket.is_some() {
            build_client_with_socket(
                self.connect_timeout,
                self.verify_tls,
                self.unix_socket.as_deref(),
            )?
        } else if self.connect_timeout == CONNECT_TIMEOUT && self.verify_tls {
            self.factory.client.clone()
        } else {
            build_client(self.connect_timeout, self.verify_tls)?
        };
        let target = uri_template::expand(self.base_url.as_deref(), url, &self.uri_parameters)?;
        let base = self.base_url.as_deref().map(uri_template::escape_braces);
        let mut url = parse_url(base.as_deref(), &target)?;
        if let Some(destination) = &self.destination {
            destination.check_url(&url)?;
        }
        let mut query_override = None;
        if !self.query.is_empty() {
            let original_query = url
                .query()
                .map(|query| RecordedRequestDataSource::Query(Arc::from(query)));
            let mut encoder = url::form_urlencoded::Serializer::new(String::new());
            encoder.extend_pairs(self.query);
            let query = encoder
                .finish()
                .replace('+', "%20")
                .replace('*', "%2A")
                .replace("%7E", "~");
            url.set_query(Some(&query));
            if method == Method::GET {
                query_override = Some((url.clone(), original_query));
            }
        }
        let mut request = client.request(method, url).headers(self.headers);
        let mut multipart = None;
        let mut diagnostic_source = None;
        request = match (self.payload, data) {
            (Payload::Json, Some(data)) => {
                diagnostic_source = Some(RecordedRequestDataSource::Json);
                request.json(data)
            }
            (Payload::Form, Some(data)) => {
                diagnostic_source = Some(RecordedRequestDataSource::Form(None));
                if !self.redact_payload
                    && (self.factory.events.is_some() || self.factory.is_recording())
                {
                    // 只执行一次应用的 Serialize，使用同一份数据编码表单并记录字段类型。
                    let captured = serde_save::save(data).map_err(|_| Error::InvalidRequest)?;
                    diagnostic_source =
                        Some(RecordedRequestDataSource::Form(form_fields(&captured)));
                    request.form(&captured)
                } else {
                    request.form(data)
                }
            }
            (Payload::Raw(body), _) => request.body(body),
            (Payload::Multipart(parts), data) => {
                let mut form = Form::new();
                let mut fields = BTreeMap::new();
                if let Some(data) = data {
                    let value = serde_json::to_value(data).map_err(Error::InvalidJson)?;
                    let serde_json::Value::Object(data_fields) = value else {
                        return Err(Error::InvalidRequest);
                    };
                    for (name, value) in data_fields {
                        let text = match &value {
                            serde_json::Value::String(value) => value.clone(),
                            serde_json::Value::Number(value) => value.to_string(),
                            serde_json::Value::Bool(value) => {
                                if *value { "1" } else { "" }.to_owned()
                            }
                            serde_json::Value::Null => String::new(),
                            _ => return Err(Error::InvalidRequest),
                        };
                        form = form.text(name.clone(), text);
                        fields.insert(name, RecordedMultipartField::Value(value));
                    }
                }
                // Laravel 先发送普通字段，然后按登记顺序追加附件。
                for Attachment {
                    name,
                    part,
                    metadata,
                } in parts
                {
                    fields.insert(name.clone(), metadata);
                    form = form.part(name, part);
                }
                let content_type = HeaderValue::from_str(&format!(
                    "multipart/form-data; boundary={}",
                    form.boundary(),
                ))
                .map_err(|_| Error::InvalidRequest)?;
                multipart = Some((Arc::new(fields), content_type));
                request.multipart(form)
            }
            _ => request,
        };
        let request = request.build().map_err(|_| Error::InvalidRequest)?;
        let form_body = if matches!(
            diagnostic_source,
            Some(RecordedRequestDataSource::Form(Some(_)))
        ) {
            request
                .try_clone()
                .and_then(|mut request| request.body_mut().take())
        } else {
            None
        };
        let Retry {
            mut delays,
            when,
            throw,
        } = self.retry;
        let throw_on_retry_failure = delays.len() > 0 && throw;
        let mut replay = if delays.len() == 0 {
            None
        } else {
            Some(request.try_clone().ok_or(Error::UnreplayableRequest)?)
        };
        let transport = Transport {
            factory: &self.factory,
            middleware: &self.middleware,
            client: &client,
            before_sending: &self.before_sending,
            attributes: &self.attributes,
            multipart,
            query_override,
            diagnostic_source,
            form_body,
            redact_payload: self.redact_payload,
            redact_response: self.redact_response,
            digest: self.digest.as_ref(),
            digest_origin: request.url().origin(),
            max_redirects: self.redirects.filter(|_| self.destination.is_none()),
            destination: self.destination.as_ref(),
            timeout: self.timeout,
            body_limit: self.body_limit,
            sink: self.sink.as_ref(),
        };
        let mut request = request;
        loop {
            let result = transport.send(request).await;
            let result = result.and_then(|response| {
                let stats = response.transfer_stats.clone();
                let mut response = self
                    .after_response
                    .iter()
                    .try_fold(response, |response, callback| callback(response))?;
                if response.transfer_stats.is_none() {
                    response.transfer_stats = stats;
                }
                Ok(response)
            });
            let error = match result {
                Ok(response)
                    if response.failed()
                        && (throw_on_retry_failure
                            || delays.len() > 0
                            || self.throw_condition.is_some()) =>
                {
                    Error::RequestFailed(Box::new(response))
                }
                Ok(response) => return Ok(response),
                Err(error) => error,
            };
            let retryable = matches!(
                error,
                Error::Connection | Error::Timeout | Error::RequestFailed(_)
            ) && match &when {
                Some(condition) => condition.permits(&error, replay.as_mut())?,
                None => true,
            };
            if retryable && let Some(delay) = delays.next() {
                tokio::time::sleep(delay).await;
                request = if delays.len() == 0 {
                    replay.take().ok_or(Error::UnreplayableRequest)?
                } else {
                    replay
                        .as_ref()
                        .and_then(reqwest::Request::try_clone)
                        .ok_or(Error::UnreplayableRequest)?
                };
                continue;
            }
            return match error {
                Error::RequestFailed(response)
                    if !throw_on_retry_failure
                        && !self
                            .throw_condition
                            .as_ref()
                            .is_some_and(|test| test(&response)) =>
                {
                    Ok(*response)
                }
                error => Err(error),
            };
        }
    }
}

fn form_fields(captured: &serde_save::Save<'static>) -> Option<Arc<serde_json::Value>> {
    use serde_json::{Map, Value};

    let value = serde_json::to_value(captured).ok()?;
    let value = match value {
        Value::Array(entries) => {
            let mut fields = Map::new();
            for entry in entries {
                let Value::Array(pair) = entry else {
                    return None;
                };
                let [key, value] = <[_; 2]>::try_from(pair).ok()?;
                let key = match key {
                    Value::String(key) => key,
                    key => key.to_string(),
                };
                fields.insert(key, value);
            }
            Value::Object(fields)
        }
        value => value,
    };
    Some(Arc::new(value))
}

fn parse_url(base: Option<&str>, url: &str) -> Result<url::Url> {
    let url = match (url::Url::parse(url), base) {
        (Ok(url), _) => url,
        (Err(url::ParseError::RelativeUrlWithoutBase), Some(base)) => url::Url::parse(&format!(
            "{}/{}",
            base.trim_end_matches('/'),
            url.trim_start_matches('/')
        ))
        .map_err(|_| Error::InvalidRequest)?,
        _ => return Err(Error::InvalidRequest),
    };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::InvalidRequest);
    }
    Ok(url)
}
