use std::{
    collections::VecDeque,
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

use super::{Batch, Error, PendingRequest, Pool, Response, Result, transport::Incoming};

pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const BODY_LIMIT: usize = usize::MAX;

type Stub = dyn Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync;

/// The application-local client pool and fake registry.
#[derive(Clone)]
pub struct Factory {
    pub(super) client: reqwest::Client,
    testing: Arc<Mutex<Testing>>,
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
}

/// An ordered queue of fake responses, shared by callers of the matching URL.
#[derive(Clone, Default)]
pub struct ResponseSequence(Arc<Mutex<Sequence>>);

#[derive(Default)]
struct Sequence {
    responses: VecDeque<Result<Response>>,
    fallback: Option<Response>,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's fake response sequence API"
)]
impl ResponseSequence {
    pub fn push(&self, response: Result<Response>) -> &Self {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .responses
            .push_back(response);
        self
    }

    pub fn whenEmpty(&self, response: Response) -> &Self {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fallback = Some(response);
        self
    }

    pub fn isEmpty(&self) -> bool {
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
        }
    }
}

pub(super) fn build_client(connect_timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Error::Configuration)
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP factory API")]
impl Factory {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: build_client(CONNECT_TIMEOUT)?,
            testing: Arc::default(),
            events: None,
        })
    }

    pub fn withDispatcher(mut self, dispatcher: Arc<Dispatcher>) -> Self {
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

    pub fn pendingRequest(&self) -> PendingRequest {
        PendingRequest::new(self.clone())
    }

    pub fn batch<'a>(&self, build: impl FnOnce(&mut Batch<'a>)) -> Batch<'a> {
        let mut batch = Batch::new(self.clone());
        build(&mut batch);
        batch
    }

    pub async fn get(&self, url: &str) -> Result<Response> {
        self.pendingRequest().get(url).await
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
    pub async fn poolOrdered<'a>(
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

    pub fn stubUrl(&self, pattern: impl Into<String>, response: Response) {
        let pattern = Str::start(&pattern.into(), "*");
        self.fake(move |request| {
            Str::is(&[&pattern], request.url().as_str(), false).then(|| Ok(response.clone()))
        });
    }

    pub fn fakeSequence(&self, pattern: impl Into<String>) -> ResponseSequence {
        let sequence = ResponseSequence::default();
        let pending = sequence.clone();
        let pattern = Str::start(&pattern.into(), "*");
        self.fake(move |request| {
            Str::is(&[&pattern], request.url().as_str(), false).then(|| pending.next())
        });
        sequence
    }

    pub fn preventStrayRequests(&self, prevent: bool) {
        self.testing().prevent_stray = prevent;
    }

    /// Replaces the URL patterns exempt from stray-request prevention.
    pub fn allowStrayRequests(&self, patterns: impl IntoIterator<Item = impl Into<String>>) {
        let patterns = patterns.into_iter().map(Into::into).collect();
        self.testing().allowed_stray_urls = patterns;
    }

    pub fn preventingStrayRequests(&self) -> bool {
        self.testing().prevent_stray
    }

    pub fn isAllowedRequestUrl(&self, url: &str) -> bool {
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

    pub fn withUrlParameters(
        &self,
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> PendingRequest {
        self.pendingRequest().withUrlParameters(parameters)
    }

    pub fn recorded(&self) -> Vec<(RecordedRequest, Option<Response>)> {
        self.testing().recorded.clone()
    }

    #[track_caller]
    pub fn assertSent(&self, predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool) {
        assert!(
            self.recorded()
                .iter()
                .any(|(request, response)| predicate(request, response.as_ref())),
            "no recorded HTTP request matched"
        );
    }

    #[track_caller]
    pub fn assertNotSent(&self, predicate: impl Fn(&RecordedRequest, Option<&Response>) -> bool) {
        assert!(
            !self
                .recorded()
                .iter()
                .any(|(request, response)| predicate(request, response.as_ref())),
            "a recorded HTTP request matched"
        );
    }

    #[track_caller]
    pub fn assertSentCount(&self, expected: usize) {
        assert_eq!(
            self.testing().recorded.len(),
            expected,
            "unexpected recorded HTTP request count"
        );
    }

    #[track_caller]
    pub fn assertNothingSent(&self) {
        self.assertSentCount(0);
    }

    fn testing(&self) -> MutexGuard<'_, Testing> {
        self.testing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) async fn dispatch(
        &self,
        client: &reqwest::Client,
        request: reqwest::Request,
    ) -> Result<Incoming> {
        let stubs = self.testing().stubs.clone();
        match stubs.iter().find_map(|callback| callback(&request)) {
            Some(result) => result.map(Incoming::Fake),
            None if !self.isAllowedRequestUrl(request.url().as_str()) => Err(Error::StrayRequest),
            None => client
                .execute(request)
                .await
                .map(Incoming::Network)
                .map_err(super::transport_error),
        }
    }
}
