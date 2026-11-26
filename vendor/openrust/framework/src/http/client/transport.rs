use std::{collections::BTreeMap, sync::Arc, time::Duration};

use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use indexmap::IndexMap;
use reqwest::Request;
use serde_json::{Map, Value};
use tokio::time::Instant;
use url::Position;

use super::{
    Error, Factory, RecordedMultipartField, RecordedRequest, RecordedRequestDataSource, Response,
    Result,
    digest::DigestCredentials,
    events::{ConnectionFailed, ConnectionFailure, RequestSending, ResponseReceived},
    pending_request::RequestMiddleware,
};

pub(super) struct MultipartObservation {
    fields: Arc<BTreeMap<String, RecordedMultipartField>>,
    order: Arc<[String]>,
    content_type: HeaderValue,
    descriptors: Arc<[Value]>,
}

impl MultipartObservation {
    pub(super) fn new(
        fields: IndexMap<String, RecordedMultipartField>,
        content_type: HeaderValue,
        descriptors: Vec<Value>,
    ) -> Self {
        Self {
            order: fields.keys().cloned().collect(),
            fields: Arc::new(fields.into_iter().collect()),
            content_type,
            descriptors: descriptors.into(),
        }
    }

    fn record(&self, request: &mut RecordedRequest) {
        request.multipart = Some(Arc::clone(&self.fields));
        request.diagnostic_source = Some(RecordedRequestDataSource::Multipart(Arc::clone(
            &self.order,
        )));
    }

    fn record_event(&self, request: &mut RecordedRequest) {
        let multipart = request
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("multipart"));
        if multipart {
            self.record(request);
        } else if !self.descriptors.is_empty() {
            request.diagnostic_source = Some(RecordedRequestDataSource::MultipartDescriptors(
                Arc::clone(&self.descriptors),
            ));
        }
    }
}

pub(super) struct Transport<'a> {
    pub middleware: &'a [std::sync::Arc<super::Middleware>],
    pub factory: &'a Factory,
    pub client: &'a reqwest::Client,
    pub before_sending: &'a [Box<RequestMiddleware>],
    pub attributes: &'a Map<String, Value>,
    pub multipart: Option<MultipartObservation>,
    pub query_override: Option<(url::Url, Option<RecordedRequestDataSource>)>,
    pub diagnostic_source: Option<RecordedRequestDataSource>,
    pub form_body: Option<reqwest::Body>,
    pub redact_payload: bool,
    pub redact_response: bool,
    pub digest: Option<&'a DigestCredentials>,
    pub digest_origin: url::Origin,
    pub max_redirects: Option<usize>,
    pub destination: Option<&'a super::destination::PinnedDestination>,
    pub timeout: Duration,
    pub body_limit: usize,
    pub sink: Option<&'a super::Sink>,
    pub stream: bool,
}

impl Transport<'_> {
    pub(super) async fn send(&self, mut request: Request) -> Result<Response> {
        let has_events = self.factory.events.is_some();
        let observe_payload = has_events && !self.redact_payload;
        let event_source = observe_payload
            .then(|| self.event_source(&request))
            .flatten();
        let started_at = Instant::now();
        let stream_deadline = self
            .stream
            .then(|| {
                started_at
                    .checked_add(self.timeout)
                    .ok_or(Error::Configuration)
            })
            .transpose()?;
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
                    sending = has_events.then(|| {
                        let mut snapshot = self.snapshot(&request);
                        snapshot.diagnostic_source.clone_from(&event_source);
                        if observe_payload && let Some(multipart) = &self.multipart {
                            multipart.record_event(&mut snapshot);
                        }
                        snapshot
                    });
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
                    if let Some(destination) = self.destination {
                        destination.check_request(&request)?;
                    }
                    sent = (recording || has_events).then(|| self.snapshot(&request));
                    redirect = self
                        .max_redirects
                        .is_some()
                        .then(|| Redirect::from(&request));
                    replay = (authentications < 2
                        && self.digest.is_some()
                        && self.digest_origin == request.url().origin()
                        && (authentications > 0
                            || !request.headers().contains_key(header::AUTHORIZATION)))
                    .then(|| request.try_clone())
                    .flatten();
                    let transfer_started = Instant::now();
                    let sink = self.sink.map(super::Sink::writer);
                    if let Some(sink) = &sink {
                        tokio::time::timeout(remaining, sink.validate())
                            .await
                            .unwrap_or(Err(Error::Timeout))?;
                    }
                    let remaining = remaining.saturating_sub(transfer_started.elapsed());
                    let (mut response, network) = self
                        .factory
                        .dispatch(self.client, request, remaining, |incoming| async {
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
                            incoming
                                .finish(self.body_limit, following, sink, stream_deadline)
                                .await
                        })
                        .await?;
                    let measured = super::TransferStats {
                        duration: if network {
                            transfer_started.elapsed()
                        } else {
                            Duration::ZERO
                        },
                        network,
                    };
                    stats = Some(measured.clone());
                    response.transfer_stats = Some(measured);
                    Ok(response)
                })
            });
            let result = super::middleware::run(self.middleware, request, next).await;
            let result = result.map(|mut response| {
                if response.transfer_stats.is_none() {
                    response.transfer_stats = stats;
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
                        && let Some(mut request) = sent
                        && let Some(exception) = ConnectionFailure::from_error(&error)
                    {
                        // 连接失败使用异常携带的实际请求，只有媒体类型允许时才解码正文。
                        request.diagnostic_source = None;
                        request.multipart = None;
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
                    if Some(redirects) == self.max_redirects {
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

    fn event_source(&self, request: &Request) -> Option<RecordedRequestDataSource> {
        use RecordedRequestDataSource::{Form, Json, Payload, Query};

        // 原始构建参数随请求发送与响应事件保留，重定向只替换方法和地址等线上字段。
        match &self.diagnostic_source {
            Some(Form(Some(fields))) => Some(Payload(Arc::clone(fields))),
            Some(Json) => {
                let body = request.body()?.as_bytes()?;
                serde_json::from_slice(body)
                    .ok()
                    .map(|data| Payload(Arc::new(data)))
            }
            _ if request.method() == Method::GET && request.body().is_none() => {
                if let Some((url, original)) = &self.query_override
                    && request.url() == url
                {
                    original.clone()
                } else {
                    request.url().query().map(|query| Query(Arc::from(query)))
                }
            }
            _ => None,
        }
    }

    fn observed_response(&self, response: &Response) -> Response {
        if self.redact_response || (self.stream && response.successful()) {
            let headers = if self.redact_response {
                HeaderMap::new()
            } else {
                response.headers().clone()
            };
            let mut redacted = Response::new(response.status(), headers, Vec::new());
            redacted.transfer_stats.clone_from(&response.transfer_stats);
            redacted
        } else {
            response.clone()
        }
    }

    fn snapshot(&self, request: &Request) -> RecordedRequest {
        let mut snapshot = RecordedRequest::from(request);
        // HTTP 传输从目标地址补足 Host，诊断快照也保留同一主机与端口。
        if !snapshot.headers.contains_key(header::HOST)
            && request.url().has_host()
            && let Ok(host) =
                HeaderValue::from_str(&request.url()[Position::BeforeHost..Position::AfterPort])
        {
            snapshot.headers.insert(header::HOST, host);
        }
        if request.method() == Method::GET
            && request.body().is_none()
            && let Some((url, original_query)) = &self.query_override
            && request.url() == url
        {
            snapshot.diagnostic_source.clone_from(original_query);
        } else if snapshot.body.is_some() && request.url().origin() == self.digest_origin {
            snapshot.diagnostic_source = match &self.diagnostic_source {
                Some(RecordedRequestDataSource::Form(Some(_)))
                    if self.form_body.as_ref().and_then(reqwest::Body::as_bytes)
                        != snapshot.body.as_deref() =>
                {
                    Some(RecordedRequestDataSource::Form(None))
                }
                source => source.clone(),
            };
        }
        snapshot.attributes.clone_from(self.attributes);
        if self.redact_payload {
            snapshot.url.set_query(None);
            snapshot.diagnostic_source = None;
            snapshot.body = None;
            snapshot.headers.remove(header::AUTHORIZATION);
            let sensitive = snapshot
                .headers
                .iter()
                .filter(|(_, value)| value.is_sensitive())
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            for name in sensitive {
                snapshot.headers.remove(name);
            }
        } else if let Some(multipart) = &self.multipart
            && request.body().is_some_and(|body| body.as_bytes().is_none())
            && request.headers().get(header::CONTENT_TYPE) == Some(&multipart.content_type)
            && request.url().origin() == self.digest_origin
        {
            multipart.record(&mut snapshot);
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
        if !status.is_redirection() {
            return None;
        }
        if status <= StatusCode::SEE_OTHER {
            if !matches!(
                *request.method(),
                Method::GET | Method::HEAD | Method::OPTIONS
            ) {
                *request.method_mut() = Method::GET;
            }
            *request.body_mut() = None;
            for name in [header::CONTENT_LENGTH, header::TRANSFER_ENCODING] {
                request.headers_mut().remove(name);
            }
        } else if !self.replayable {
            return None;
        }
        let location = headers.get(header::LOCATION)?.to_str().ok()?;
        let mut target = request.url().join(location).ok()?;
        if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
            return None;
        }
        let _ = target.set_password(None);
        let _ = target.set_username("");
        request.headers_mut().remove(header::REFERER);
        request.headers_mut().remove(header::HOST);
        if target.origin() != request.url().origin() {
            for name in [
                header::AUTHORIZATION,
                header::COOKIE,
                header::PROXY_AUTHORIZATION,
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

    async fn finish(
        self,
        limit: usize,
        following: bool,
        sink: Option<super::sink::Writer>,
        stream_deadline: Option<Instant>,
    ) -> Result<Response> {
        match self {
            Self::Network(mut response) if following && sink.is_none() => {
                let status = response.status();
                let headers = std::mem::take(response.headers_mut());
                Ok(Response::new(status, headers, Vec::new()))
            }
            Self::Network(response) => {
                if let Some(deadline) = stream_deadline.filter(|_| response.status().is_success()) {
                    Response::from_transport_stream(response, limit, deadline)
                } else {
                    Response::from_transport(response, limit, sink).await
                }
            }
            Self::Fake(response) => {
                if !(following && sink.is_none()) && response.body_len()? > limit as u64 {
                    return Err(Error::BodyTooLarge);
                }
                if let Some(mut sink) = sink {
                    sink.write(&response.body()?).await?;
                    let _ = sink.finish().await?;
                }
                Ok(response)
            }
        }
    }
}
