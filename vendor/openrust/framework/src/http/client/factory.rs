use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use http::{HeaderMap, Method};
use url::Url;

use crate::support::Str;

use super::{Error, PendingRequest, Pool, Response, Result};

pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const BODY_LIMIT: usize = usize::MAX;

type Stub = dyn Fn(&reqwest::Request) -> Option<Result<Response>> + Send + Sync;

/// The application-local client pool and fake registry.
#[derive(Clone)]
pub struct Factory {
    pub(super) client: reqwest::Client,
    testing: Arc<Mutex<Testing>>,
}

#[derive(Default)]
struct Testing {
    stubs: Vec<Arc<Stub>>,
    recording: bool,
    prevent_stray: bool,
    recorded: Vec<(RecordedRequest, Option<Response>)>,
}

#[derive(Clone)]
pub struct RecordedRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
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
        }
    }
}

pub(super) fn build_client(connect_timeout: Duration, redirects: usize) -> Result<reqwest::Client> {
    let policy = if redirects == 0 {
        reqwest::redirect::Policy::none()
    } else {
        reqwest::redirect::Policy::limited(redirects)
    };
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .redirect(policy)
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Error::Configuration)
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP factory API")]
impl Factory {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: build_client(CONNECT_TIMEOUT, 5)?,
            testing: Arc::default(),
        })
    }

    pub fn pendingRequest(&self) -> PendingRequest {
        PendingRequest::new(self.clone())
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
        pool.run(concurrency).await
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
        limit: usize,
    ) -> Result<Response> {
        let (stubs, prevent_stray, record) = {
            let testing = self.testing();
            (
                testing.stubs.clone(),
                testing.prevent_stray,
                testing.recording,
            )
        };
        let recorded = record.then(|| RecordedRequest::from(&request));
        let result = match stubs.iter().find_map(|callback| callback(&request)) {
            Some(Ok(response)) if response.body().len() > limit => Err(Error::BodyTooLarge),
            Some(result) => result,
            None if prevent_stray => return Err(Error::StrayRequest),
            None => {
                match client
                    .execute(request)
                    .await
                    .map_err(super::transport_error)
                {
                    Ok(response) => Response::from_transport(response, limit).await,
                    Err(error) => Err(error),
                }
            }
        };
        if let Some(recorded) = recorded {
            self.testing()
                .recorded
                .push((recorded, result.as_ref().ok().cloned()));
        }
        result
    }
}
