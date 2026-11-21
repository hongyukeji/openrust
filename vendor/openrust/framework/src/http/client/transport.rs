use std::time::Duration;

use http::{HeaderMap, Method, StatusCode, header};
use reqwest::Request;
use serde_json::{Map, Value};
use tokio::time::Instant;

use super::{
    Error, Factory, RecordedRequest, Response, Result,
    digest::DigestCredentials,
    events::{ConnectionFailed, ConnectionFailure, RequestSending, ResponseReceived},
    pending_request::RequestMiddleware,
};

pub(super) struct Transport<'a> {
    pub middleware: &'a [std::sync::Arc<super::Middleware>],
    pub factory: &'a Factory,
    pub client: &'a reqwest::Client,
    pub before_sending: &'a [Box<RequestMiddleware>],
    pub attributes: &'a Map<String, Value>,
    pub redact_payload: bool,
    pub redact_response: bool,
    pub digest: Option<&'a DigestCredentials>,
    pub digest_origin: url::Origin,
    pub max_redirects: usize,
    pub timeout: Duration,
    pub body_limit: usize,
}

impl Transport<'_> {
    pub(super) async fn send(&self, mut request: Request) -> Result<Response> {
        let has_events = self.factory.events.is_some();
        let started_at = Instant::now();
        let mut redirects = 0;
        let mut authentications = 0;
        loop {
            let mut sending = None;
            let mut sent = None;
            let mut redirect = None;
            let mut replay = None;
            let mut stats = None;
            let recording = self.factory.is_recording();
            let remaining = self.timeout.saturating_sub(started_at.elapsed());
            let next = super::Next::new(|mut request: Request| {
                Box::pin(async {
                    sending = has_events.then(|| self.snapshot(&request));
                    if let Some(request) = &sending {
                        self.factory
                            .emit(RequestSending {
                                request: request.clone(),
                            })
                            .await?;
                    }
                    for callback in self.before_sending {
                        callback(&mut request)?;
                    }
                    sent = (recording || has_events).then(|| self.snapshot(&request));
                    redirect = (self.max_redirects > 0).then(|| Redirect::from(&request));
                    replay = (authentications < 2
                        && self.digest.is_some()
                        && self.digest_origin == request.url().origin()
                        && (authentications > 0
                            || !request.headers().contains_key(header::AUTHORIZATION)))
                    .then(|| request.try_clone())
                    .flatten();
                    let transfer_started = Instant::now();
                    let (mut response, network) = tokio::time::timeout(remaining, async {
                        let incoming = self.factory.dispatch(self.client, request).await?;
                        let network = matches!(&incoming, Incoming::Network(_));
                        // Redirect/authentication bodies remain unbuffered, as in the original transport.
                        let following = (incoming.status() == StatusCode::UNAUTHORIZED
                            && replay
                                .as_ref()
                                .and_then(Request::try_clone)
                                .and_then(|request| {
                                    self.digest?.authenticate(
                                        request,
                                        incoming.headers(),
                                        authentications > 0,
                                    )
                                })
                                .is_some())
                            || redirect.as_ref().is_some_and(|redirect| {
                                redirect.can_follow(incoming.status(), incoming.headers())
                            });
                        Ok((incoming.finish(self.body_limit, following).await?, network))
                    })
                    .await
                    .unwrap_or(Err(Error::Timeout))?;
                    let measured = super::TransferStats {
                        duration: if network {
                            transfer_started.elapsed()
                        } else {
                            Duration::ZERO
                        },
                        network,
                    };
                    stats = Some(measured.clone());
                    response.transferStats = Some(measured);
                    Ok(response)
                })
            });
            let result = super::middleware::run(self.middleware, request, next).await;
            let result = result.map(|mut response| {
                if response.transferStats.is_none() {
                    response.transferStats = stats;
                }
                response
            });
            if recording
                && !matches!(result, Err(Error::StrayRequest))
                && let Some(sent) = &sent
            {
                self.factory.record(
                    sent.clone(),
                    result
                        .as_ref()
                        .ok()
                        .map(|response| self.observed_response(response)),
                );
            }
            let response = match result {
                Ok(response) => response,
                Err(error) => {
                    if has_events
                        && let Some(request) = sent
                        && let Some(exception) = ConnectionFailure::from_error(&error)
                    {
                        self.factory
                            .emit(ConnectionFailed { request, exception })
                            .await?;
                    }
                    return Err(error);
                }
            };
            let authenticated = replay
                .filter(|_| response.status() == StatusCode::UNAUTHORIZED)
                .and_then(|request| {
                    self.digest?
                        .authenticate(request, response.headers(), authentications > 0)
                });
            let is_authenticated = authenticated.is_some();
            let next = authenticated.or_else(|| {
                redirect.and_then(|redirect| redirect.follow(response.status(), response.headers()))
            });
            if let Some(next) = next {
                if is_authenticated {
                    authentications += 1;
                } else {
                    if redirects == self.max_redirects {
                        return Err(Error::RedirectLimit);
                    }
                    redirects += 1;
                    authentications = 0;
                }
                request = next;
                continue;
            }
            if let Some(request) = sending {
                self.factory
                    .emit(ResponseReceived {
                        request,
                        response: self.observed_response(&response),
                    })
                    .await?;
            }
            return Ok(response);
        }
    }

    fn observed_response(&self, response: &Response) -> Response {
        if self.redact_response {
            let mut redacted = Response::new(response.status(), http::HeaderMap::new(), Vec::new());
            redacted.transferStats.clone_from(&response.transferStats);
            redacted
        } else {
            response.clone()
        }
    }

    fn snapshot(&self, request: &Request) -> RecordedRequest {
        let mut snapshot = RecordedRequest::from(request);
        snapshot.attributes.clone_from(self.attributes);
        if self.redact_payload {
            snapshot.url.set_query(None);
            snapshot.body = None;
            snapshot.headers.remove(header::AUTHORIZATION);
        }
        snapshot
    }
}

struct Redirect {
    request: Request,
    replayable: bool,
}

impl From<&Request> for Redirect {
    fn from(request: &Request) -> Self {
        let replay = request.try_clone();
        Self {
            replayable: replay.is_some(),
            request: replay.unwrap_or_else(|| {
                let mut skeleton = Request::new(request.method().clone(), request.url().clone());
                *skeleton.version_mut() = request.version();
                skeleton.headers_mut().clone_from(request.headers());
                skeleton
            }),
        }
    }
}

impl Redirect {
    fn can_follow(&self, status: StatusCode, headers: &HeaderMap) -> bool {
        let mut candidate = Self::from(&self.request);
        candidate.replayable = self.replayable;
        candidate.follow(status, headers).is_some()
    }

    fn follow(self, status: StatusCode, headers: &HeaderMap) -> Option<Request> {
        let mut request = self.request;
        match status {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER => {
                if request.method() != Method::HEAD {
                    *request.method_mut() = Method::GET;
                }
                *request.body_mut() = None;
                for name in [
                    header::CONTENT_LENGTH,
                    header::CONTENT_TYPE,
                    header::TRANSFER_ENCODING,
                ] {
                    request.headers_mut().remove(name);
                }
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT if self.replayable => {}
            _ => return None,
        }
        let location = headers.get(header::LOCATION)?.to_str().ok()?;
        let mut target = request.url().join(location).ok()?;
        if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
            return None;
        }
        let _ = target.set_password(None);
        let _ = target.set_username("");
        if target.origin() != request.url().origin() {
            for name in [
                header::AUTHORIZATION,
                header::COOKIE,
                header::PROXY_AUTHORIZATION,
                header::HOST,
            ] {
                request.headers_mut().remove(name);
            }
            request.headers_mut().remove("cookie2");
        }
        if request
            .headers()
            .get(header::AUTHORIZATION)
            .is_some_and(|auth| {
                auth.as_bytes()
                    .get(..7)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"digest "))
            })
        {
            request.headers_mut().remove(header::AUTHORIZATION);
        }
        *request.url_mut() = target;
        Some(request)
    }
}

pub(super) enum Incoming {
    Fake(Response),
    Network(reqwest::Response),
}

impl Incoming {
    fn status(&self) -> StatusCode {
        match self {
            Self::Fake(response) => response.status(),
            Self::Network(response) => response.status(),
        }
    }

    fn headers(&self) -> &HeaderMap {
        match self {
            Self::Fake(response) => response.headers(),
            Self::Network(response) => response.headers(),
        }
    }

    async fn finish(self, limit: usize, following: bool) -> Result<Response> {
        match self {
            Self::Network(mut response) if following => {
                let status = response.status();
                let headers = std::mem::take(response.headers_mut());
                Ok(Response::new(status, headers, Vec::new()))
            }
            Self::Network(response) => Response::from_transport(response, limit).await,
            Self::Fake(response) if following || response.body().len() <= limit => Ok(response),
            Self::Fake(_) => Err(Error::BodyTooLarge),
        }
    }
}
