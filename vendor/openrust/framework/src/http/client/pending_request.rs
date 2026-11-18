use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use http::{HeaderMap, HeaderValue, Method, header};
use serde::Serialize;

use super::digest::DigestCredentials;
use super::factory::{BODY_LIMIT, CONNECT_TIMEOUT, TIMEOUT, build_client};
use super::transport::Transport;
use super::uri_template;
use super::{BatchResults, Error, Factory, Form, Pool, Response, Result};

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
    Multipart(Form),
}

/// Builder for one outbound request.
pub struct PendingRequest {
    factory: Factory,
    base_url: Option<String>,
    headers: HeaderMap,
    invalid_header: bool,
    digest: Option<DigestCredentials>,
    query: indexmap::IndexMap<String, String>,
    uri_parameters: serde_json::Map<String, serde_json::Value>,
    attributes: serde_json::Map<String, serde_json::Value>,
    redact_payload: bool,
    connect_timeout: Duration,
    timeout: Duration,
    redirects: usize,
    body_limit: usize,
    payload: Payload,
    retry: Retry,
    throw_condition: Option<Box<ThrowCondition>>,
    before_sending: Vec<Box<RequestMiddleware>>,
    after_response: Vec<Box<ResponseMiddleware>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's pending HTTP request API")]
impl PendingRequest {
    pub(super) fn new(factory: Factory) -> Self {
        Self {
            factory,
            base_url: None,
            headers: HeaderMap::new(),
            invalid_header: false,
            digest: None,
            query: indexmap::IndexMap::new(),
            uri_parameters: Default::default(),
            attributes: serde_json::Map::new(),
            redact_payload: false,
            connect_timeout: CONNECT_TIMEOUT,
            timeout: TIMEOUT,
            redirects: 5,
            body_limit: BODY_LIMIT,
            payload: Payload::Json,
            retry: Retry::default(),
            throw_condition: None,
            before_sending: Vec::new(),
            after_response: Vec::new(),
        }
    }

    pub fn baseUrl(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn withHeaders(mut self, headers: HeaderMap) -> Self {
        for name in headers.keys() {
            for value in headers.get_all(name) {
                self.headers.append(name, value.clone());
            }
        }
        self
    }

    pub fn replaceHeaders(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self.invalid_header = false;
        self
    }

    pub fn accept(mut self, content_type: HeaderValue) -> Self {
        self.headers.insert(header::ACCEPT, content_type);
        self
    }

    pub fn acceptJson(self) -> Self {
        self.accept(HeaderValue::from_static("application/json"))
    }

    pub fn withToken(self, token: &str) -> Self {
        self.authorization(format!("Bearer {token}"))
    }

    pub fn withBasicAuth(self, username: &str, password: &str) -> Self {
        use base64::{Engine, engine::general_purpose::STANDARD};
        self.authorization(format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        ))
    }

    pub fn withDigestAuth(mut self, username: &str, password: &str) -> Self {
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

    pub fn withUrlParameters(
        mut self,
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.uri_parameters.extend(parameters);
        self
    }

    pub(crate) fn redact_payload(mut self) -> Self {
        self.redact_payload = true;
        self
    }

    pub fn withQueryParameters(
        mut self,
        parameters: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.query.extend(parameters);
        self
    }

    pub fn asForm(mut self) -> Self {
        self.payload = Payload::Form;
        self
    }

    pub fn asJson(mut self) -> Self {
        self.payload = Payload::Json;
        self
    }

    pub fn withBody(mut self, body: impl Into<reqwest::Body>, content_type: HeaderValue) -> Self {
        self.payload = Payload::Raw(body.into());
        self.headers.insert(header::CONTENT_TYPE, content_type);
        self
    }

    pub fn attach(mut self, name: impl Into<String>, part: super::Part) -> Self {
        let form = match self.payload {
            Payload::Multipart(form) => form,
            _ => Form::new(),
        };
        self.payload = Payload::Multipart(form.part(name.into(), part));
        self
    }

    pub fn withAttributes(
        mut self,
        attributes: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.attributes.extend(attributes);
        self
    }

    pub fn beforeSending(
        mut self,
        callback: impl Fn(&mut reqwest::Request) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.before_sending.push(Box::new(callback));
        self
    }

    pub fn afterResponse(
        mut self,
        callback: impl Fn(Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.after_response.push(Box::new(callback));
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    pub fn connectTimeout(mut self, duration: Duration) -> Self {
        self.connect_timeout = duration;
        self
    }

    pub fn withoutRedirecting(mut self) -> Self {
        self.redirects = 0;
        self
    }

    pub fn maxResponseBytes(mut self, limit: usize) -> Self {
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

    pub fn retryDelays(mut self, delays: impl Into<Vec<Duration>>) -> Self {
        self.retry.delays = RetryDelays::Backoff(delays.into().into_iter());
        self.retry.throw = true;
        self
    }

    pub fn retryWhen(mut self, when: impl Fn(&Error) -> bool + Send + Sync + 'static) -> Self {
        self.retry.when = Some(RetryWhen::Inspect(Arc::new(when)));
        self
    }

    /// Decides whether to retry and configures the request that will be replayed.
    pub fn retryWhenRequest(
        mut self,
        when: impl Fn(&Error, &mut reqwest::Request) -> Result<bool> + Send + Sync + 'static,
    ) -> Self {
        self.retry.when = Some(RetryWhen::Modify(Box::new(when)));
        self
    }

    pub fn throwOnRetryFailure(mut self, throw: bool) -> Self {
        self.retry.throw = throw;
        self
    }

    pub fn throw(self) -> Self {
        self.throwIf(|_| true)
    }

    pub fn throwIf(
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

    pub async fn poolOrdered<'a>(&self, build: impl FnOnce(&mut Pool<'a>)) -> BatchResults {
        self.factory.poolOrdered(build).await
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
        let client = if self.connect_timeout == CONNECT_TIMEOUT {
            self.factory.client.clone()
        } else {
            build_client(self.connect_timeout)?
        };
        let target = uri_template::expand(self.base_url.as_deref(), url, &self.uri_parameters)?;
        let base = self.base_url.as_deref().map(uri_template::escape_braces);
        let mut url = parse_url(base.as_deref(), &target)?;
        if !self.query.is_empty() {
            let mut encoder = url::form_urlencoded::Serializer::new(String::new());
            encoder.extend_pairs(self.query);
            let query = encoder
                .finish()
                .replace('+', "%20")
                .replace('*', "%2A")
                .replace("%7E", "~");
            url.set_query(Some(&query));
        }
        let mut request = client.request(method, url).headers(self.headers);
        request = match (self.payload, data) {
            (Payload::Json, Some(data)) => request.json(data),
            (Payload::Form, Some(data)) => request.form(data),
            (Payload::Raw(body), _) => request.body(body),
            (Payload::Multipart(form), _) => request.multipart(form),
            _ => request,
        };
        let request = request.build().map_err(|_| Error::InvalidRequest)?;
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
            client: &client,
            before_sending: &self.before_sending,
            attributes: &self.attributes,
            redact_payload: self.redact_payload,
            digest: self.digest.as_ref(),
            digest_origin: request.url().origin(),
            max_redirects: self.redirects,
            timeout: self.timeout,
            body_limit: self.body_limit,
        };
        let mut request = request;
        loop {
            let result = transport.send(request).await;
            let result = result.and_then(|response| {
                self.after_response
                    .iter()
                    .try_fold(response, |response, callback| callback(response))
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
