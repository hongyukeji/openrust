use std::{
    collections::VecDeque,
    future::Future,
    num::NonZeroUsize,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use http::{HeaderMap, Method};
use url::Url;

use crate::{
    contracts::events::{Dispatcher as _, Event},
    events::{Dispatcher, EventContext},
    support::Str,
};

use super::{
    Batch, Error, Middleware, Next, PendingRequest, Pool, Response, Result, transport::Incoming,
};

pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const BODY_LIMIT: usize = usize::MAX;

type Stub = dyn Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync;

/// The application-local client pool and fake registry.
#[derive(Clone)]
pub struct Factory {
    pub(super) client: reqwest::Client,
    testing: Arc<Mutex<Testing>>,
    middleware: Arc<Mutex<Vec<Arc<Middleware>>>>,
    pub(super) events: Option<Arc<Dispatcher>>,
}

#[derive(Default)]
struct Testing {
    stubs: Vec<Arc<Stub>>,
    recording: bool,
    prevent_stray: bool,
    allowed_stray_urls: Arc<[String]>,
    recorded: Vec<(RecordedRequest, Option<Response>)>,
}

#[derive(Clone)]
pub struct RecordedRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub attributes: serde_json::Map<String, serde_json::Value>,
    /// 诊断载荷的来源。
    pub diagnostic_source: Option<RecordedRequestDataSource>,
    /// 构建器提供的 multipart 字段快照。不会读取正文流。
    pub multipart: Option<Arc<std::collections::BTreeMap<String, RecordedMultipartField>>>,
}

#[derive(Clone)]
pub enum RecordedRequestDataSource {
    Query(Arc<str>),
    Form(Option<Arc<serde_json::Value>>),
    Json,
    Payload(Arc<serde_json::Value>),
}

#[derive(Clone)]
pub enum RecordedMultipartField {
    Value(serde_json::Value),
    Attachment {
        filename: Option<String>,
        length: Option<u64>,
        headers: HeaderMap,
    },
}

impl RecordedRequest {
    /// Decodes buffered JSON or form fields and query parameters without reading a stream.
    pub fn data(&self) -> Option<serde_json::Value> {
        use serde_json::Value;
        let query =
            crate::http::request::query::parse(self.url.query().unwrap_or_default()).ok()?;
        let media = self.media_type();
        let body = self.decode_body_fields(
            media.eq_ignore_ascii_case("application/x-www-form-urlencoded"),
            media.eq_ignore_ascii_case("application/json") || media.ends_with("+json"),
        );
        match (body, query) {
            (Some(Value::Object(mut body)), Value::Object(query)) => {
                for (key, value) in query {
                    body.entry(key).or_insert(value);
                }
                Some(body.into())
            }
            (Some(body), _) => Some(body),
            (_, query) => Some(query),
        }
    }

    /// 按出站诊断的规则读取 JSON、表单或 GET 参数，不读取正文流。
    pub fn diagnostic_data(&self) -> serde_json::Value {
        let content_types = self.headers.get_all(http::header::CONTENT_TYPE);
        let is_form = content_types
            .iter()
            .any(|value| value == "application/x-www-form-urlencoded");
        let is_json = content_types
            .iter()
            .next()
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("json"));
        let data = match &self.diagnostic_source {
            Some(RecordedRequestDataSource::Query(query)) => {
                crate::http::request::query::parse(query).ok()
            }
            Some(RecordedRequestDataSource::Form(fields)) => fields
                .as_deref()
                .cloned()
                .or_else(|| self.decode_body_fields(true, false)),
            Some(RecordedRequestDataSource::Json) => self.decode_body_fields(false, true),
            Some(RecordedRequestDataSource::Payload(payload)) => Some(payload.as_ref().clone()),
            None => self.decode_body_fields(is_form, is_json),
        };
        match data {
            Some(serde_json::Value::Object(fields)) if fields.is_empty() => serde_json::json!([]),
            Some(data) if !data.is_null() => data,
            _ => serde_json::json!([]),
        }
    }

    fn media_type(&self) -> &str {
        self.headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_default()
            .trim()
    }

    fn decode_body_fields(&self, form: bool, json: bool) -> Option<serde_json::Value> {
        let body = self.body.as_deref().filter(|body| !body.is_empty())?;
        if form {
            std::str::from_utf8(body)
                .ok()
                .and_then(|body| crate::http::request::query::parse_form(body).ok())
        } else if json {
            serde_json::from_slice(body).ok()
        } else {
            None
        }
    }
}

/// An ordered queue of fake responses, shared by callers of the matching URL.
#[derive(Clone, Default)]
pub struct ResponseSequence(Arc<Mutex<Sequence>>);

#[derive(Default)]
struct Sequence {
    responses: VecDeque<Result<Response>>,
    fallback: Option<Response>,
}

impl ResponseSequence {
    pub fn push(&self, response: Result<Response>) -> &Self {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .responses
            .push_back(response);
        self
    }

    pub fn when_empty(&self, response: Response) -> &Self {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fallback = Some(response);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .responses
            .is_empty()
    }

    fn next(&self) -> Result<Response> {
        let mut sequence = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sequence
            .responses
            .pop_front()
            .unwrap_or_else(|| sequence.fallback.clone().ok_or(Error::SequenceExhausted))
    }
}

impl From<&reqwest::Request> for RecordedRequest {
    fn from(request: &reqwest::Request) -> Self {
        Self {
            method: request.method().clone(),
            url: request.url().clone(),
            headers: request.headers().clone(),
            body: request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .map(<[u8]>::to_vec),
            attributes: Default::default(),
            diagnostic_source: if request.method() == Method::GET && request.body().is_none() {
                request
                    .url()
                    .query()
                    .map(|query| RecordedRequestDataSource::Query(Arc::from(query)))
            } else {
                None
            },
            multipart: None,
        }
    }
}

pub(super) fn build_client(connect_timeout: Duration, verify_tls: bool) -> Result<reqwest::Client> {
    build_client_with_socket(connect_timeout, verify_tls, None)
}

pub(super) fn build_client_with_socket(
    connect_timeout: Duration,
    verify_tls: bool,
    socket: Option<&std::path::Path>,
) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .tls_danger_accept_invalid_certs(!verify_tls)
        .tls_danger_accept_invalid_hostnames(!verify_tls)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .retry(reqwest::retry::never());
    #[cfg(unix)]
    let builder = match socket {
        Some(path) => builder.unix_socket(path),
        None => builder,
    };
    #[cfg(not(unix))]
    if socket.is_some() {
        return Err(Error::Configuration);
    }
    builder.build().map_err(|_| Error::Configuration)
}

tokio::task_local! {static WITHOUT_GLOBAL:Vec<usize>;}

impl Factory {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: build_client(CONNECT_TIMEOUT, true)?,
            testing: Arc::default(),
            middleware: Arc::default(),
            events: None,
        })
    }

    pub fn with_dispatcher(mut self, dispatcher: Arc<Dispatcher>) -> Self {
        self.events = Some(dispatcher);
        self
    }

    pub(super) fn is_recording(&self) -> bool {
        self.testing().recording
    }

    pub(super) fn record(&self, request: RecordedRequest, response: Option<Response>) {
        self.testing().recorded.push((request, response));
    }

    pub(super) async fn emit(&self, event: impl Event) -> Result<()> {
        if let Some(dispatcher) = &self.events {
            dispatcher
                .dispatch(Arc::new(event), &mut EventContext::new())
                .await
                .map_err(Error::Event)?;
        }
        Ok(())
    }

    /// Registers a wrapper for subsequently created pending requests in this factory.
    pub fn global_middleware(
        &self,
        middleware: impl for<'a> Fn(
            reqwest::Request,
            Next<'a>,
        ) -> crate::http::HttpFuture<'a, Result<Response>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        self.middleware
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(middleware));
        self
    }

    pub fn global_request_middleware(
        &self,
        middleware: impl Fn(&mut reqwest::Request) -> Result<()> + Send + Sync + 'static,
    ) -> &Self {
        self.global_middleware(move |mut request, next| match middleware(&mut request) {
            Ok(()) => next.run(request),
            Err(error) => Box::pin(async move { Err(error) }),
        })
    }

    pub fn global_response_middleware(
        &self,
        middleware: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> &Self {
        let middleware = Arc::new(middleware);
        self.global_middleware(move |request, next| {
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

    pub(super) fn middleware(&self) -> Vec<Arc<Middleware>> {
        if WITHOUT_GLOBAL
            .try_with(|owners| owners.contains(&self.identity()))
            .unwrap_or(false)
        {
            return vec![];
        }
        self.middleware
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn identity(&self) -> usize {
        Arc::as_ptr(&self.testing).addr()
    }
    /// 只在当前调用中创建无全局配置的请求；Fake、记录和事件仍共用原工厂。
    pub fn without_global_configuration<T>(&self, callback: impl FnOnce(&Self) -> T) -> T {
        let mut owners = WITHOUT_GLOBAL.try_with(Clone::clone).unwrap_or_default();
        owners.push(self.identity());
        WITHOUT_GLOBAL.sync_scope(owners, || callback(self))
    }
    pub async fn without_global_configuration_async<T>(
        &self,
        operation: impl std::future::Future<Output = T>,
    ) -> T {
        let mut owners = WITHOUT_GLOBAL.try_with(Clone::clone).unwrap_or_default();
        owners.push(self.identity());
        WITHOUT_GLOBAL.scope(owners, operation).await
    }
    pub fn with_user_agent(&self, user_agent: &str) -> PendingRequest {
        self.pending_request().with_user_agent(user_agent)
    }

    pub fn without_verifying(&self) -> PendingRequest {
        self.pending_request().without_verifying()
    }

    pub fn sink(&self, sink: impl Into<super::Sink>) -> PendingRequest {
        self.pending_request().sink(sink)
    }

    pub fn pending_request(&self) -> PendingRequest {
        PendingRequest::new(self.clone())
    }

    pub fn batch<'a>(&self, build: impl FnOnce(&mut Batch<'a>)) -> Batch<'a> {
        let mut batch = Batch::new(self.clone());
        build(&mut batch);
        batch
    }

    pub async fn get(&self, url: &str) -> Result<Response> {
        self.pending_request().get(url).await
    }

    pub async fn pool<'a>(
        &self,
        concurrency: NonZeroUsize,
        build: impl FnOnce(&mut Pool<'a>),
    ) -> indexmap::IndexMap<String, Result<Response>> {
        let mut pool = Pool::new(self.clone());
        build(&mut pool);
        pool.run_with(concurrency, false, |_, _| {}).await
    }

    /// Starts all requests and returns their results in registration order.
    pub async fn pool_ordered<'a>(
        &self,
        build: impl FnOnce(&mut Pool<'a>),
    ) -> indexmap::IndexMap<String, Result<Response>> {
        let mut pool = Pool::new(self.clone());
        build(&mut pool);
        pool.run_with(NonZeroUsize::MAX, true, |_, _| {}).await
    }

    pub fn fake(
        &self,
        callback: impl Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync + 'static,
    ) {
        let mut testing = self.testing();
        testing.recording = true;
        testing.recorded.clear();
        testing.stubs.push(Arc::new(callback));
    }

    pub fn stub_url(&self, pattern: impl Into<String>, response: Response) {
        let pattern = Str::start(&pattern.into(), "*");
        self.fake(move |request| {
            Str::is(&[&pattern], request.url().as_str(), false).then(|| Ok(response.clone()))
        });
    }

    pub fn fake_sequence(&self, pattern: impl Into<String>) -> ResponseSequence {
        let sequence = ResponseSequence::default();
        let pending = sequence.clone();
        let pattern = Str::start(&pattern.into(), "*");
        self.fake(move |request| {
            Str::is(&[&pattern], request.url().as_str(), false).then(|| pending.next())
        });
        sequence
    }

    pub fn prevent_stray_requests(&self, prevent: bool) {
        self.testing().prevent_stray = prevent;
    }

    /// Replaces the URL patterns exempt from stray-request prevention.
    pub fn allow_stray_requests(&self, patterns: impl IntoIterator<Item = impl Into<String>>) {
        let patterns = patterns.into_iter().map(Into::into).collect();
        self.testing().allowed_stray_urls = patterns;
    }

    pub fn preventing_stray_requests(&self) -> bool {
        self.testing().prevent_stray
    }

    pub fn is_allowed_request_url(&self, url: &str) -> bool {
        let patterns = {
            let testing = self.testing();
            if !testing.prevent_stray {
                return true;
            }
            testing.allowed_stray_urls.clone()
        };
        patterns
            .iter()
            .any(|pattern| Str::is(&[pattern], url, false))
    }

    pub fn with_url_parameters(
        &self,
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> PendingRequest {
        self.pending_request().with_url_parameters(parameters)
    }

    pub fn recorded(&self) -> Vec<(RecordedRequest, Option<Response>)> {
        self.testing().recorded.clone()
    }

    #[track_caller]
    pub fn assert_sent(&self, predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool) {
        assert!(
            self.recorded()
                .iter()
                .any(|(request, response)| predicate(request, response.as_ref())),
            "no recorded HTTP request matched"
        );
    }

    #[track_caller]
    pub fn assert_not_sent(&self, predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool) {
        assert!(
            !self
                .recorded()
                .iter()
                .any(|(request, response)| predicate(request, response.as_ref())),
            "a recorded HTTP request matched"
        );
    }

    #[track_caller]
    pub fn assert_sent_count(&self, expected: usize) {
        assert_eq!(
            self.testing().recorded.len(),
            expected,
            "unexpected recorded HTTP request count"
        );
    }

    #[track_caller]
    pub fn assert_nothing_sent(&self) {
        self.assert_sent_count(0);
    }

    fn testing(&self) -> MutexGuard<'_, Testing> {
        self.testing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) async fn dispatch<F: Future<Output = Result<Response>>>(
        &self,
        client: &reqwest::Client,
        mut request: reqwest::Request,
        remaining: Duration,
        finish: impl FnOnce(Incoming) -> F,
    ) -> Result<(Response, bool)> {
        let stubs = self.testing().stubs.clone();
        match stubs.iter().find_map(|callback| callback(&request)) {
            Some(result) => tokio::time::timeout(remaining, finish(Incoming::Fake(result?)))
                .await
                .unwrap_or(Err(Error::Timeout))
                .map(|response| (response, false)),
            None if !self.is_allowed_request_url(request.url().as_str()) => {
                Err(Error::StrayRequest)
            }
            None => {
                use crate::foundation::observability::{self, Operation, Outcome};
                let owner = self.events.as_ref().and_then(|events| events.owner());
                let method = request.method().clone();
                let origin = request.url().origin();
                let observation =
                    owner
                        .as_ref()
                        .map_or_else(observability::Execution::default, |owner| {
                            observability::begin(
                                owner,
                                Operation::HttpClient {
                                    method: &method,
                                    origin: &origin,
                                    headers: request.headers_mut(),
                                },
                            )
                        });
                let transfer = tokio::time::timeout(remaining, async move {
                    let response = client.execute(request).await.map_err(|error| {
                        let kind = if error.is_timeout() {
                            "timeout"
                        } else if error.is_connect() {
                            "connection"
                        } else {
                            "transport"
                        };
                        (super::transport_error(error), kind)
                    })?;
                    finish(Incoming::Network(response)).await.map_err(|error| {
                        let kind = match &error {
                            Error::Timeout => "timeout",
                            Error::BodyTooLarge => "body_too_large",
                            Error::Connection | Error::ResponseBody(_) => "response_body",
                            Error::Sink(_) => "sink",
                            _ => "transport",
                        };
                        (error, kind)
                    })
                });
                observation
                    .run(
                        async { transfer.await.unwrap_or(Err((Error::Timeout, "timeout"))) },
                        |result| {
                            Outcome::HttpClient(match result {
                                Ok(response) => Ok(response.status()),
                                Err((_, kind)) => Err(kind),
                            })
                        },
                    )
                    .await
                    .map(|response| (response, true))
                    .map_err(|(error, _)| error)
            }
        }
    }
}
