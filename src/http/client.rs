//! Shared outbound HTTP client with explicit resource limits.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use http::{HeaderMap, Method, StatusCode};
use serde::de::DeserializeOwned;

/// Request body type accepted by [`HttpRequest::body`].
pub use reqwest::Body as HttpBody;
/// Request type supplied to [`HttpTransport`] implementations.
pub use reqwest::Request as HttpTransportRequest;
/// Multipart form body accepted by [`HttpRequest::multipart`].
pub use reqwest::multipart::Form as MultipartForm;

/// Resource and timeout settings for [`HttpClient`].
#[derive(Clone, PartialEq, Eq)]
pub struct HttpClientConfig {
    /// Maximum time to establish a connection.
    pub connect_timeout: Duration,
    /// Maximum time for the complete request and response body.
    pub timeout: Duration,
    /// Maximum number of bytes buffered by [`HttpResponse::bytes`].
    pub max_response_bytes: usize,
    /// Optional explicit proxy URL. Environment proxy variables are ignored.
    pub proxy: Option<String>,
}

impl std::fmt::Debug for HttpClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpClientConfig")
            .field("connect_timeout", &self.connect_timeout)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("proxy", &self.proxy.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(30),
            max_response_bytes: 1024 * 1024,
            proxy: None,
        }
    }
}

/// Outbound HTTP client construction, transport, or bounded body error.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// A timeout or network/protocol failure occurred. Details are intentionally redacted.
    #[error("outbound HTTP transport failed")]
    Transport,
    /// The configured connection or total request timeout elapsed.
    #[error("outbound HTTP request timed out")]
    Timeout,
    /// The response body exceeded the configured limit.
    #[error("outbound HTTP response exceeded the configured body limit")]
    BodyTooLarge,
    /// A response body could not be decoded as JSON.
    #[error("outbound HTTP response contained invalid JSON")]
    InvalidJson,
    /// Client configuration is invalid.
    #[error("invalid outbound HTTP client configuration")]
    InvalidConfig,
    /// A request cannot be replayed because its body is a one-shot stream.
    #[error("outbound HTTP request body cannot be replayed")]
    UnreplayableRequest,
}

/// A transport that executes outbound requests. Implementations must not log request secrets.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Sends one request and returns a streaming response.
    async fn send(
        &self,
        request: HttpTransportRequest,
    ) -> Result<TransportResponse, HttpClientError>;
}

/// A response body source used by [`HttpTransport`].
#[async_trait]
pub trait HttpResponseBody: Send {
    /// Returns one body chunk or `None` after end of stream.
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError>;
}

/// A transport response before the framework applies its configured body limit.
pub struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Box<dyn HttpResponseBody>,
}

impl TransportResponse {
    /// Creates a response from metadata and a body source.
    pub fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: impl HttpResponseBody + 'static,
    ) -> Self {
        Self {
            status,
            headers,
            body: Box::new(body),
        }
    }

    /// Creates an in-memory response body, useful for deterministic fake transports.
    pub fn from_bytes(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self::new(status, headers, BytesResponseBody(Some(body)))
    }
}

struct BytesResponseBody(Option<Vec<u8>>);

#[async_trait]
impl HttpResponseBody for BytesResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        Ok(self.0.take())
    }
}

struct ReqwestTransport(reqwest::Client);

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn send(&self, request: reqwest::Request) -> Result<TransportResponse, HttpClientError> {
        let response = self
            .0
            .execute(request)
            .await
            .map_err(|error| map_transport_error(&error))?;
        let status = response.status();
        let headers = response.headers().clone();
        Ok(TransportResponse::new(
            status,
            headers,
            ReqwestResponseBody(response),
        ))
    }
}

struct ReqwestResponseBody(reqwest::Response);

#[async_trait]
impl HttpResponseBody for ReqwestResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        self.0
            .chunk()
            .await
            .map(|chunk| chunk.map(|chunk| chunk.to_vec()))
            .map_err(|error| map_transport_error(&error))
    }
}

/// Explicit bounded retry policy for a request the caller declares idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u8,
    base_delay: Duration,
    max_delay: Duration,
}

impl RetryPolicy {
    /// Creates a policy with one to five attempts and capped exponential backoff.
    pub fn new(
        max_attempts: u8,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Result<Self, HttpClientError> {
        if !(1..=5).contains(&max_attempts) || base_delay > max_delay {
            return Err(HttpClientError::InvalidConfig);
        }
        Ok(Self {
            max_attempts,
            base_delay,
            max_delay,
        })
    }

    fn delay(self, retry_index: u32) -> Duration {
        self.base_delay
            .saturating_mul(2_u32.saturating_pow(retry_index))
            .min(self.max_delay)
    }
}

/// Reusable outbound client. Clones share the underlying connection pool.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    transport: Arc<dyn HttpTransport>,
    max_response_bytes: usize,
    timeout: Duration,
}

impl HttpClient {
    /// Creates a client with validated limits and automatic redirects disabled.
    pub fn new(config: HttpClientConfig) -> Result<Self, HttpClientError> {
        validate_config(&config)?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxy) = config.proxy {
            let proxy = reqwest::Proxy::all(proxy).map_err(|_| HttpClientError::InvalidConfig)?;
            builder = builder.proxy(proxy);
        }
        let inner = builder
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        Ok(Self {
            inner: inner.clone(),
            transport: Arc::new(ReqwestTransport(inner)),
            max_response_bytes: config.max_response_bytes,
            timeout: config.timeout,
        })
    }

    /// Creates a client using an injected transport, for test fakes or application transports.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "configuration is consumed at the client construction boundary"
    )]
    pub fn with_transport(
        config: HttpClientConfig,
        transport: impl HttpTransport + 'static,
    ) -> Result<Self, HttpClientError> {
        validate_config(&config)?;
        let inner = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| HttpClientError::InvalidConfig)?;
        Ok(Self {
            inner,
            transport: Arc::new(transport),
            max_response_bytes: config.max_response_bytes,
            timeout: config.timeout,
        })
    }

    /// Starts a request. Dropping the returned future cancels local waiting; it cannot undo a
    /// request already processed by the remote server.
    pub fn request(&self, method: Method, url: &str) -> HttpRequest {
        HttpRequest {
            inner: self.inner.request(method, url),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Starts a GET request.
    pub fn get(&self, url: &str) -> HttpRequest {
        self.request(Method::GET, url)
    }

    /// Starts a POST request.
    pub fn post(&self, url: &str) -> HttpRequest {
        self.request(Method::POST, url)
    }
}

fn map_transport_error(error: &reqwest::Error) -> HttpClientError {
    if error.is_timeout() {
        HttpClientError::Timeout
    } else {
        HttpClientError::Transport
    }
}

fn validate_config(config: &HttpClientConfig) -> Result<(), HttpClientError> {
    if config.connect_timeout.is_zero()
        || config.timeout.is_zero()
        || config.max_response_bytes == 0
    {
        return Err(HttpClientError::InvalidConfig);
    }
    if let Some(proxy) = &config.proxy {
        reqwest::Proxy::all(proxy).map_err(|_| HttpClientError::InvalidConfig)?;
    }
    Ok(())
}

/// An in-progress request with common body encodings.
pub struct HttpRequest {
    inner: reqwest::RequestBuilder,
    transport: Arc<dyn HttpTransport>,
    max_response_bytes: usize,
    timeout: Duration,
}

impl HttpRequest {
    /// Sets a JSON request body.
    #[must_use]
    pub fn json<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.json(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sets a URL-encoded form body.
    #[must_use]
    pub fn form<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.form(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sets a multipart request body.
    #[must_use]
    pub fn multipart(self, form: MultipartForm) -> Self {
        Self {
            inner: self.inner.multipart(form),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sets a raw request body, including an asynchronous streaming body.
    #[must_use]
    pub fn body(self, body: HttpBody) -> Self {
        Self {
            inner: self.inner.body(body),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sets request headers.
    #[must_use]
    pub fn headers(self, headers: HeaderMap) -> Self {
        Self {
            inner: self.inner.headers(headers),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Adds serialized query parameters to the request URL.
    #[must_use]
    pub fn query<T: serde::Serialize + ?Sized>(self, value: &T) -> Self {
        Self {
            inner: self.inner.query(value),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sets a bearer authorization value.
    #[must_use]
    pub fn bearer_auth(self, token: &str) -> Self {
        Self {
            inner: self.inner.bearer_auth(token),
            transport: Arc::clone(&self.transport),
            max_response_bytes: self.max_response_bytes,
            timeout: self.timeout,
        }
    }

    /// Sends the request and returns status and headers regardless of status class.
    pub async fn send(self) -> Result<HttpResponse, HttpClientError> {
        let transport = self.transport;
        let max_response_bytes = self.max_response_bytes;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let request = self
            .inner
            .build()
            .map_err(|error| map_transport_error(&error))?;
        let response = tokio::time::timeout_at(deadline, transport.send(request))
            .await
            .map_err(|_| HttpClientError::Timeout)??;
        HttpResponse::from_transport(response, max_response_bytes, deadline)
    }

    /// Declares that this operation is safe for the caller to replay.
    ///
    /// This is an assertion by the application; method names alone cannot establish that a
    /// particular remote operation is idempotent.
    #[must_use]
    pub fn idempotent(self) -> IdempotentHttpRequest {
        IdempotentHttpRequest(self)
    }
}

/// A request explicitly declared safe to retry by its caller.
pub struct IdempotentHttpRequest(HttpRequest);

impl IdempotentHttpRequest {
    /// Sends the request with bounded exponential backoff for transport failures and 5xx status.
    pub async fn send_with_retry(
        self,
        policy: RetryPolicy,
    ) -> Result<HttpResponse, HttpClientError> {
        let HttpRequest {
            inner,
            transport,
            max_response_bytes,
            timeout,
        } = self.0;
        let request = inner.build().map_err(|error| map_transport_error(&error))?;
        let deadline = tokio::time::Instant::now() + timeout;
        if policy.max_attempts == 1 {
            let response = tokio::time::timeout_at(deadline, transport.send(request))
                .await
                .map_err(|_| HttpClientError::Timeout)??;
            return HttpResponse::from_transport(response, max_response_bytes, deadline);
        }
        if policy.max_attempts > 1 && request.try_clone().is_none() {
            return Err(HttpClientError::UnreplayableRequest);
        }
        for attempt in 0..policy.max_attempts {
            let request = request
                .try_clone()
                .ok_or(HttpClientError::UnreplayableRequest)?;
            let response = tokio::time::timeout_at(deadline, transport.send(request))
                .await
                .map_err(|_| HttpClientError::Timeout)?;
            match response {
                Ok(response)
                    if attempt + 1 < policy.max_attempts
                        && matches!(
                            response.status,
                            StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT
                        ) =>
                {
                    drop(response);
                    tokio::time::timeout_at(
                        deadline,
                        tokio::time::sleep(policy.delay(u32::from(attempt))),
                    )
                    .await
                    .map_err(|_| HttpClientError::Timeout)?;
                }
                Ok(response) => {
                    return HttpResponse::from_transport(response, max_response_bytes, deadline);
                }
                Err(error)
                    if attempt + 1 < policy.max_attempts
                        && matches!(
                            error,
                            HttpClientError::Transport | HttpClientError::Timeout
                        ) =>
                {
                    tokio::time::timeout_at(
                        deadline,
                        tokio::time::sleep(policy.delay(u32::from(attempt))),
                    )
                    .await
                    .map_err(|_| HttpClientError::Timeout)?;
                }
                Err(error) => return Err(error),
            }
        }
        Err(HttpClientError::Transport)
    }
}

impl HttpResponse {
    fn from_transport(
        response: TransportResponse,
        max_response_bytes: usize,
        deadline: tokio::time::Instant,
    ) -> Result<Self, HttpClientError> {
        if let Some(content_length) = response.headers.get(http::header::CONTENT_LENGTH) {
            let content_length = content_length
                .to_str()
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or(HttpClientError::Transport)?;
            if content_length > max_response_bytes {
                return Err(HttpClientError::BodyTooLarge);
            }
        }
        Ok(Self {
            status: response.status,
            headers: response.headers,
            body: response.body,
            max_response_bytes,
            body_bytes_read: 0,
            deadline,
        })
    }
}

/// Response metadata and bounded body readers.
pub struct HttpResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Box<dyn HttpResponseBody>,
    max_response_bytes: usize,
    body_bytes_read: usize,
    deadline: tokio::time::Instant,
}

impl HttpResponse {
    /// HTTP status returned by the server.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Response headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Reads one response chunk, returning `None` at end of stream.
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
        let Some(chunk) = tokio::time::timeout_at(self.deadline, self.body.next_chunk())
            .await
            .map_err(|_| HttpClientError::Timeout)??
        else {
            return Ok(None);
        };
        let new_size = self.body_bytes_read.saturating_add(chunk.len());
        if new_size > self.max_response_bytes {
            return Err(HttpClientError::BodyTooLarge);
        }
        self.body_bytes_read = new_size;
        Ok(Some(chunk[..].to_vec()))
    }

    /// Reads a response body while enforcing the configured size limit.
    pub async fn bytes(mut self) -> Result<Vec<u8>, HttpClientError> {
        let mut body = Vec::new();
        while let Some(chunk) = self.next_chunk().await? {
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Reads and deserializes a JSON response within the configured size limit.
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, HttpClientError> {
        let body = self.bytes().await?;
        serde_json::from_slice(&body).map_err(|_| HttpClientError::InvalidJson)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::Json;
    use axum::routing::get;
    use serde::Deserialize;
    use tokio::net::TcpListener;

    use super::{
        HttpClient, HttpClientConfig, HttpClientError, HttpResponseBody, HttpTransport,
        RetryPolicy, TransportResponse,
    };

    struct FakeTransport {
        responses: Mutex<VecDeque<Result<http::StatusCode, HttpClientError>>>,
        requests: AtomicUsize,
        content_types: Mutex<Vec<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl HttpTransport for FakeTransport {
        async fn send(
            &self,
            request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            assert_eq!(request.url().host_str(), Some("fake.invalid"));
            self.requests.fetch_add(1, Ordering::SeqCst);
            let content_type = request
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            self.content_types
                .lock()
                .map_err(|_| HttpClientError::Transport)?
                .push(content_type);
            let status = self
                .responses
                .lock()
                .map_err(|_| HttpClientError::Transport)?
                .pop_front()
                .unwrap_or(Ok(http::StatusCode::OK))?;
            Ok(TransportResponse::from_bytes(
                status,
                http::HeaderMap::new(),
                br#"{"value":"fake"}"#.to_vec(),
            ))
        }
    }

    #[derive(Deserialize)]
    struct Message {
        value: String,
    }

    #[tokio::test]
    async fn local_http_json_and_body_limit() -> Result<(), Box<dyn std::error::Error>> {
        let app = axum::Router::new()
            .route(
                "/json",
                get(|| async { Json(serde_json::json!({"value": "ok"})) }),
            )
            .route(
                "/redirect",
                get(|| async { (http::StatusCode::FOUND, [(http::header::LOCATION, "/json")]) }),
            )
            .route("/large", get(|| async { "123456789" }));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = HttpClient::new(HttpClientConfig::default())?;

        let response = client.get(&format!("http://{address}/json")).send().await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        let redirect = client
            .get(&format!("http://{address}/redirect"))
            .send()
            .await?;
        assert_eq!(redirect.status(), http::StatusCode::FOUND);
        let message: Message = client
            .get(&format!("http://{address}/json"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(message.value, "ok");

        let mut streamed = client
            .get(&format!("http://{address}/large"))
            .send()
            .await?;
        assert!(streamed.next_chunk().await?.is_some());
        assert!(streamed.next_chunk().await?.is_none());

        let limited = HttpClient::new(HttpClientConfig {
            max_response_bytes: 4,
            ..HttpClientConfig::default()
        })?;
        assert!(matches!(
            limited.get(&format!("http://{address}/large")).send().await,
            Err(HttpClientError::BodyTooLarge)
        ));
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn invalid_timeout_configuration_is_rejected() {
        let config = HttpClientConfig {
            timeout: Duration::ZERO,
            ..HttpClientConfig::default()
        };
        assert!(matches!(
            HttpClient::new(config),
            Err(HttpClientError::InvalidConfig)
        ));
    }

    #[test]
    fn configuration_debug_redacts_proxy_credentials() {
        let config = HttpClientConfig {
            proxy: Some("http://user:secret@proxy.example".to_owned()),
            ..HttpClientConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("REDACTED"));
    }

    #[tokio::test]
    async fn total_timeout_is_enforced() -> Result<(), Box<dyn std::error::Error>> {
        let app = axum::Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                "late"
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let client = HttpClient::new(HttpClientConfig {
            timeout: Duration::from_millis(10),
            ..HttpClientConfig::default()
        })?;
        assert!(matches!(
            client.get(&format!("http://{address}/slow")).send().await,
            Err(HttpClientError::Timeout)
        ));
        server.abort();
        Ok(())
    }

    struct SlowTransport;

    #[async_trait::async_trait]
    impl HttpTransport for SlowTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(TransportResponse::from_bytes(
                http::StatusCode::OK,
                http::HeaderMap::new(),
                Vec::new(),
            ))
        }
    }

    #[tokio::test]
    async fn injected_transport_respects_the_configured_timeout() -> Result<(), HttpClientError> {
        let client = HttpClient::with_transport(
            HttpClientConfig {
                timeout: Duration::from_millis(10),
                ..HttpClientConfig::default()
            },
            SlowTransport,
        )?;
        assert!(matches!(
            client.get("http://fake.invalid/slow").send().await,
            Err(HttpClientError::Timeout)
        ));
        Ok(())
    }

    struct SlowResponseBody;

    #[async_trait::async_trait]
    impl HttpResponseBody for SlowResponseBody {
        async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpClientError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(None)
        }
    }

    struct SlowBodyTransport;

    #[async_trait::async_trait]
    impl HttpTransport for SlowBodyTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            Ok(TransportResponse::new(
                http::StatusCode::OK,
                http::HeaderMap::new(),
                SlowResponseBody,
            ))
        }
    }

    #[tokio::test]
    async fn total_timeout_covers_injected_response_body_reads() -> Result<(), HttpClientError> {
        let client = HttpClient::with_transport(
            HttpClientConfig {
                timeout: Duration::from_millis(10),
                ..HttpClientConfig::default()
            },
            SlowBodyTransport,
        )?;
        let mut response = client.get("http://fake.invalid/slow-body").send().await?;
        assert!(matches!(
            response.next_chunk().await,
            Err(HttpClientError::Timeout)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn fake_transport_never_opens_a_network_connection() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let response = client.get("http://fake.invalid/endpoint").send().await?;
        let message: Message = response.json().await?;
        assert_eq!(message.value, "fake");
        assert_eq!(transport.requests.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn fake_body_without_content_length_is_still_bounded() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig {
                max_response_bytes: 4,
                ..HttpClientConfig::default()
            },
            ArcTransport(transport),
        )?;
        let response = client.get("http://fake.invalid/large").send().await?;
        assert!(matches!(
            response.bytes().await,
            Err(HttpClientError::BodyTooLarge)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn fake_transport_accepts_form_and_multipart_requests() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(http::StatusCode::OK),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        client
            .post("http://fake.invalid/form")
            .form(&[("kind", "test")])
            .send()
            .await?;
        client
            .post("http://fake.invalid/multipart")
            .multipart(reqwest::multipart::Form::new().text("kind", "test"))
            .send()
            .await?;
        let content_types = transport
            .content_types
            .lock()
            .map_err(|_| HttpClientError::Transport)?;
        assert_eq!(content_types.len(), 2);
        assert!(
            content_types[0]
                .as_deref()
                .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"))
        );
        assert!(
            content_types[1]
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data"))
        );
        Ok(())
    }

    struct ArcTransport(Arc<FakeTransport>);

    #[async_trait::async_trait]
    impl HttpTransport for ArcTransport {
        async fn send(
            &self,
            request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            self.0.send(request).await
        }
    }

    #[tokio::test]
    async fn explicit_retry_is_bounded_for_idempotent_requests() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(http::StatusCode::SERVICE_UNAVAILABLE),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_retry_retries_transport_errors() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Err(HttpClientError::Transport),
                Ok(http::StatusCode::OK),
            ])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(2, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_retry_stops_at_the_attempt_limit() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(
                std::iter::repeat_with(|| Ok(http::StatusCode::SERVICE_UNAVAILABLE))
                    .take(5)
                    .collect(),
            ),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO)?;
        let response = client
            .get("http://fake.invalid/retry")
            .idempotent()
            .send_with_retry(policy)
            .await?;
        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(transport.requests.load(Ordering::SeqCst), 3);
        Ok(())
    }

    #[tokio::test]
    async fn one_shot_stream_body_is_not_replayed() -> Result<(), HttpClientError> {
        let transport = Arc::new(FakeTransport {
            responses: Mutex::new(VecDeque::from([Ok(http::StatusCode::OK)])),
            requests: AtomicUsize::new(0),
            content_types: Mutex::new(Vec::new()),
        });
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            ArcTransport(Arc::clone(&transport)),
        )?;
        let body = reqwest::Body::wrap_stream(tokio_stream::iter([Ok::<_, std::io::Error>(
            "streamed body".to_owned(),
        )]));
        let policy = RetryPolicy::new(2, Duration::ZERO, Duration::ZERO)?;
        assert!(matches!(
            client
                .post("http://fake.invalid/stream")
                .body(body)
                .idempotent()
                .send_with_retry(policy)
                .await,
            Err(HttpClientError::UnreplayableRequest)
        ));
        let body = reqwest::Body::wrap_stream(tokio_stream::iter([Ok::<_, std::io::Error>(
            "streamed body".to_owned(),
        )]));
        client
            .post("http://fake.invalid/stream")
            .body(body)
            .send()
            .await?;
        assert_eq!(transport.requests.load(Ordering::SeqCst), 1);
        Ok(())
    }

    struct CancellationObservedTransport {
        started: Arc<std::sync::atomic::AtomicBool>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    struct DropObserver(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for DropObserver {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl HttpTransport for CancellationObservedTransport {
        async fn send(
            &self,
            _request: reqwest::Request,
        ) -> Result<TransportResponse, HttpClientError> {
            self.started.store(true, Ordering::SeqCst);
            let _observer = DropObserver(Arc::clone(&self.dropped));
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn dropping_request_future_cancels_the_transport_future() -> Result<(), HttpClientError> {
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let client = HttpClient::with_transport(
            HttpClientConfig::default(),
            CancellationObservedTransport {
                started: Arc::clone(&started),
                dropped: Arc::clone(&dropped),
            },
        )?;
        let task =
            tokio::spawn(async move { client.get("http://fake.invalid/cancel").send().await });
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        task.abort();
        assert!(task.await.is_err_and(|error| error.is_cancelled()));
        assert!(dropped.load(Ordering::SeqCst));
        Ok(())
    }

    #[test]
    fn retry_policy_rejects_unbounded_or_inconsistent_values() {
        assert!(matches!(
            RetryPolicy::new(0, Duration::ZERO, Duration::ZERO),
            Err(HttpClientError::InvalidConfig)
        ));
        assert!(matches!(
            RetryPolicy::new(6, Duration::ZERO, Duration::ZERO),
            Err(HttpClientError::InvalidConfig)
        ));
        assert!(matches!(
            RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(1)),
            Err(HttpClientError::InvalidConfig)
        ));
    }
}
