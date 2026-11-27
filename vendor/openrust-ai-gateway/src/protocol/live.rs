use std::time::Duration;

use axum::extract::{
    FromRequestParts,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use openrust::{
    http::{
        Body, HttpError, HttpFuture, Request, Response, StatusCode,
        client::{WebSocket as Upstream, WebSocketMessage},
    },
    support::{Str, facades::Facade},
};
use openrust_ai::{AiError, LiveGateway, LiveProtocol, TextEndpoint};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Admission, Gateway, HttpResult, Key, Protocol, Target, account_state_unavailable, admit,
    authorize, request_too_large,
};
use crate::{
    managed_keys::now,
    session_affinity,
    usage::{UsageStore, live_calls::LiveCall},
};

const HTTP: Protocol = Protocol::OpenAi;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const SESSION_SECONDS: i64 = 3600;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(8);
const RENEW_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
enum Entry {
    Versioned,
    Codex,
}

impl Entry {
    fn permits(self, adapter: &dyn LiveGateway) -> bool {
        match self {
            Self::Versioned => true,
            Self::Codex => adapter.protocol() == LiveProtocol::Codex,
        }
    }

    fn sideband_path(self) -> &'static str {
        match self {
            Self::Versioned => "/v1/live/",
            Self::Codex => "/backend-api/codex/",
        }
    }
}

pub(super) fn create(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(open_call(request, Entry::Versioned))
}

pub(super) fn create_codex(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(open_call(request, Entry::Codex))
}

pub(super) fn sideband(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(upgrade(request, Entry::Versioned))
}

pub(super) fn sideband_codex(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(upgrade(request, Entry::Codex))
}

async fn open_call(request: &mut Request, entry: Entry) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, HTTP.token(request), HTTP).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let mut input = match parse_offer(request).await {
        Ok(offer) => offer,
        Err(response) => return response,
    };
    let Some(session) = input.session.as_object_mut() else {
        return invalid("session 须为 JSON 对象");
    };
    let name = match session.get("model") {
        Some(Value::String(model)) if !model.is_empty() => model.clone(),
        None => {
            let mut models = gateway
                .models
                .keys()
                .filter(|model| targets(&gateway, &key, model, entry).next().is_some());
            let Some(model) = models.next() else {
                return not_found();
            };
            if models.next().is_some() {
                return invalid("存在多个 Live 模型，须指定 session.model");
            }
            model.clone()
        }
        _ => return invalid("session.model 无效"),
    };
    let Some((target, adapter, scope)) = targets(&gateway, &key, &name, entry).next() else {
        return not_found();
    };
    if key.live_has_budget_or_quota(target) {
        return budget_unbounded();
    }
    if let Some(response) = admit(&key, HTTP).await? {
        return Ok(response);
    }
    let lease = if let Some(account) = &target.account {
        let cache = gateway.account_cache_store.as_deref();
        if let Some(cooldown) = &account.cooldown
            && !matches!(cooldown.remaining(cache).await, Ok(None))
        {
            return account_state_unavailable(HTTP);
        }
        match account.acquire(cache).await {
            Ok(Admission::Allowed(lease)) => lease,
            _ => return account_state_unavailable(HTTP),
        }
    } else {
        None
    };
    let call = LiveCall {
        id: Str::ulid().to_string(),
        owner: owner(&key),
        model: name,
        scope,
        upstream_id: None,
        expires_at: now() + SESSION_SECONDS,
    };
    if store.reserve_live_call(&call).await.is_err() {
        return unavailable();
    }
    session.insert("model".into(), target.upstream_model.as_str().into());
    let upstream = adapter.create_live_call(&input.sdp, &input.session).await;
    if let Some(lease) = lease {
        let _ = lease.release().await;
    }
    let created = match upstream {
        Ok(created) => created,
        Err(error) => {
            if (matches!(
                error,
                AiError::InvalidPrompt
                    | AiError::InvalidConfiguration(_)
                    | AiError::MissingCredential
            ) || matches!(error, AiError::ProviderStatus(400..=499)))
                && store.forget_pending_live_call(&call).await.is_err()
            {
                return unavailable();
            }
            return upstream_error(error);
        }
    };
    if store.attach_live_call(&call, &created.id).await.is_err() {
        if adapter.protocol() == LiveProtocol::OpenAi {
            let _ =
                tokio::time::timeout(SOCKET_TIMEOUT, adapter.hangup_live_call(&created.id)).await;
        }
        return unavailable();
    }
    let location = HeaderValue::from_str(&format!("{}{}", entry.sideband_path(), call.id))
        .expect("Live control path contains ASCII");
    Ok(Response::new(StatusCode::OK, Body::from(created.sdp))
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/sdp"),
        )
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .header(header::LOCATION, location))
}

async fn upgrade(request: &mut Request, entry: Entry) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, HTTP.token(request), HTTP).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let Some(id) = request.route("call_id").filter(|id| {
        id.len() == 26
            && id
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    }) else {
        return not_found();
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let call = match store.live_call(&owner(&key), id).await {
        Ok(Some(call)) => call,
        Ok(None) => return not_found(),
        Err(_) => return unavailable(),
    };
    let Some((target, adapter, _)) =
        targets(&gateway, &key, &call.model, entry).find(|(_, _, scope)| *scope == call.scope)
    else {
        return not_found();
    };
    if key.live_has_budget_or_quota(target) {
        return budget_unbounded();
    }
    let Ok(upgrade) = WebSocketUpgrade::from_request_parts(request.parts_mut(), &()).await else {
        return HTTP.error(
            StatusCode::UPGRADE_REQUIRED,
            "websocket_upgrade_required",
            "请使用 WebSocket 连接 Sideband",
        );
    };
    if let Some(response) = admit(&key, HTTP).await? {
        return Ok(response);
    }
    let controller = Str::ulid().to_string();
    match store.claim_live_control(&call, &controller).await {
        Ok(true) => {}
        Ok(false) => {
            return HTTP.error(
                StatusCode::CONFLICT,
                "controller_busy",
                "Live 会话已有控制连接",
            );
        }
        Err(_) => return unavailable(),
    }
    let control = Control {
        call,
        controller,
        store: store.clone(),
    };
    let id = control
        .call
        .upstream_id
        .as_deref()
        .expect("Live 资源已有上游 ID");
    let upstream =
        match tokio::time::timeout(SOCKET_TIMEOUT, adapter.connect_live_sideband(id)).await {
            Ok(Ok(upstream)) => upstream,
            result => {
                let error = result.map_or(AiError::Timeout, |result| {
                    result.expect_err("upstream is an error")
                });
                control
                    .finish(matches!(error, AiError::ProviderStatus(404 | 410)))
                    .await;
                return upstream_error(error);
            }
        };
    let failure = control.clone();
    let provider = target.provider.clone();
    let upstream_model = target.upstream_model.clone();
    Ok(Response::from(
        upgrade
            .max_frame_size(MAX_BYTES)
            .max_message_size(MAX_BYTES)
            .max_write_buffer_size(2 * MAX_BYTES)
            .on_failed_upgrade(move |_| {
                tokio::spawn(failure.finish(false));
            })
            .on_upgrade(move |mut socket| async move {
                let mut upstream = upstream;
                let outcome = proxy(&mut socket, &mut upstream, &control, &upstream_model).await;
                if outcome == Outcome::Expired
                    && let Some(adapter) = provider.live_gateway()
                {
                    let id = control
                        .call
                        .upstream_id
                        .as_deref()
                        .expect("Live 资源已有上游 ID");
                    match adapter.protocol() {
                        LiveProtocol::Codex => {
                            let command =
                                WebSocketMessage::Text(r#"{"type":"session.close"}"#.into());
                            let _ =
                                tokio::time::timeout(SOCKET_TIMEOUT, upstream.send(command)).await;
                        }
                        LiveProtocol::OpenAi => {
                            let _ =
                                tokio::time::timeout(SOCKET_TIMEOUT, adapter.hangup_live_call(id))
                                    .await;
                        }
                    }
                }
                let _ = tokio::time::timeout(SOCKET_TIMEOUT, upstream.close(None)).await;
                let _ =
                    tokio::time::timeout(SOCKET_TIMEOUT, socket.send(Message::Close(None))).await;
                control.finish(outcome != Outcome::Disconnected).await;
            }),
    ))
}

#[derive(Clone)]
struct Control {
    call: LiveCall,
    controller: String,
    store: UsageStore,
}

impl Control {
    async fn finish(self, closed: bool) {
        let _ = self
            .store
            .release_live_control(&self.call, &self.controller, closed)
            .await;
    }
}

#[derive(PartialEq, Eq)]
enum Outcome {
    Disconnected,
    Ended,
    Expired,
}

async fn proxy(
    socket: &mut WebSocket,
    upstream: &mut Upstream,
    control: &Control,
    model: &str,
) -> Outcome {
    let seconds = control.call.expires_at.saturating_sub(now()).max(0) as u64;
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut renew =
        tokio::time::interval_at(tokio::time::Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
    loop {
        tokio::select! {
            biased;
            () = &mut deadline => return Outcome::Expired,
            _ = renew.tick() => {
                if !matches!(tokio::time::timeout(SOCKET_TIMEOUT, control.store.renew_live_control(&control.call, &control.controller)).await, Ok(Ok(true))) {
                    return Outcome::Disconnected;
                }
            }
            from_upstream = upstream.next() => {
                let (message, ended) = match from_upstream {
                    Some(Ok(WebSocketMessage::Text(text))) => {
                        let ended = serde_json::from_str::<Value>(text.as_str()).ok().is_some_and(|event|
                            matches!(event["type"].as_str(), Some("session.closed" | "session.ended")));
                        (Message::Text(text.as_str().into()), ended)
                    }
                    Some(Ok(WebSocketMessage::Binary(bytes))) => (Message::Binary(bytes), false),
                    Some(Ok(WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) | WebSocketMessage::Frame(_))) => continue,
                    _ => return Outcome::Disconnected,
                };
                let delivered = tokio::time::timeout(SOCKET_TIMEOUT, socket.send(message)).await;
                if ended { return Outcome::Ended; }
                if !matches!(delivered, Ok(Ok(()))) { return Outcome::Disconnected; }
            }
            from_client = socket.recv() => {
                let message = match from_client {
                    Some(Ok(Message::Text(text))) => {
                        let Some(text) = control_event(text.as_str(), &control.call.model, model) else {
                            let event = Message::Text(r#"{"type":"error","error":{"code":"invalid_session_model","message":"Live 会话的模型固定为创建时的模型"}}"#.into());
                            if !matches!(tokio::time::timeout(SOCKET_TIMEOUT, socket.send(event)).await, Ok(Ok(()))) {
                                return Outcome::Disconnected;
                            }
                            continue;
                        };
                        WebSocketMessage::Text(text.into())
                    }
                    Some(Ok(Message::Binary(bytes))) => WebSocketMessage::Binary(bytes),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => return Outcome::Disconnected,
                };
                if !matches!(tokio::time::timeout(SOCKET_TIMEOUT, upstream.send(message)).await, Ok(Ok(()))) {
                    return Outcome::Disconnected;
                }
            }
        }
    }
}

fn control_event(text: &str, public: &str, upstream: &str) -> Option<String> {
    if let Ok(mut event) = serde_json::from_str::<Value>(text)
        && event["type"] == "session.update"
        && let Some(session) = event.get_mut("session").and_then(Value::as_object_mut)
        && let Some(model) = session.get_mut("model")
    {
        if model.as_str() != Some(public) {
            return None;
        }
        *model = upstream.into();
        return Some(event.to_string());
    }
    Some(text.to_owned())
}

fn targets<'a>(
    gateway: &'a Gateway,
    key: &'a Key,
    name: &str,
    entry: Entry,
) -> impl Iterator<Item = (&'a Target, &'a dyn LiveGateway, String)> + use<'a> {
    gateway
        .models
        .get(name)
        .filter(|_| key.allows_model(name))
        .into_iter()
        .flat_map(|model| model.targets_for(TextEndpoint::Live, key))
        .filter_map(move |(target, _)| {
            let adapter = target
                .provider
                .live_gateway()
                .filter(|adapter| entry.permits(*adapter))?;
            let scope = adapter.live_scope().ok()?;
            Some((
                target,
                adapter,
                session_affinity::key(&[entry.sideband_path(), &target.affinity_key, &scope]),
            ))
        })
}

fn owner(key: &Key) -> String {
    key.usage_owner().invocation_scope("live")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    sdp: String,
    session: Value,
}

async fn parse_offer(request: &mut Request) -> Result<Offer, HttpResult> {
    let media = request
        .header("content-type")
        .map_err(Err)?
        .unwrap_or_default();
    let boundary = multer::parse_boundary(media).ok();
    let is_json = media
        .split(';')
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"));
    let body = match request.body_bytes().await {
        Ok(body) if body.len() <= MAX_BYTES => body,
        Ok(_) | Err(HttpError::PayloadTooLarge) => return Err(request_too_large(HTTP)),
        Err(_) => return Err(invalid("无法读取 Live 请求")),
    };
    if is_json {
        return serde_json::from_slice(&body)
            .map_err(|_| invalid("Live 需要包含 sdp 和 session 的 JSON 对象"));
    }
    let Some(boundary) = boundary.filter(|boundary| (1..=70).contains(&boundary.len())) else {
        return Err(invalid("Live 需要 JSON 或 multipart 请求"));
    };
    let mut form = multer::Multipart::new(
        futures_util::stream::once(async { Ok::<_, HttpError>(body) }),
        boundary,
    );
    let malformed = || invalid("Live multipart 需要一组 sdp 和 session 字段");
    let mut sdp = None;
    let mut session = None;
    while let Some(field) = form.next_field().await.map_err(|_| malformed())? {
        match field.name() {
            Some("sdp") if sdp.is_none() => {
                sdp = Some(field.text().await.map_err(|_| malformed())?)
            }
            Some("session") if session.is_none() => {
                let bytes = field.bytes().await.map_err(|_| malformed())?;
                session = Some(serde_json::from_slice(&bytes).map_err(|_| malformed())?);
            }
            _ => return Err(malformed()),
        }
    }
    Ok(Offer {
        sdp: sdp.ok_or_else(malformed)?,
        session: session.ok_or_else(malformed)?,
    })
}

fn not_found() -> HttpResult {
    HTTP.error(
        StatusCode::NOT_FOUND,
        "live_not_found",
        "Live 模型或会话不可用",
    )
}

fn unavailable() -> HttpResult {
    HTTP.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "live_unavailable",
        "Live 会话存储暂不可用",
    )
}

fn invalid(message: &'static str) -> HttpResult {
    HTTP.error(StatusCode::BAD_REQUEST, "invalid_live_request", message)
}

fn budget_unbounded() -> HttpResult {
    HTTP.error(
        StatusCode::BAD_REQUEST,
        "budget_unbounded",
        "当前预算或配额无法预占 Live 会话用量",
    )
}

fn upstream_error(error: AiError) -> HttpResult {
    match error {
        AiError::InvalidPrompt => invalid("Live SDP 或会话参数无效"),
        AiError::ProviderStatus(404 | 410) => not_found(),
        AiError::Timeout => HTTP.error(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "上游 Live 请求超时",
        ),
        AiError::MissingCredential | AiError::InvalidConfiguration(_) => HTTP.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "live_unavailable",
            "上游 Live 连接未就绪",
        ),
        AiError::ProviderStatus(code @ (400 | 403 | 409 | 422 | 429)) => HTTP.error(
            StatusCode::from_u16(code).expect("valid HTTP code"),
            "upstream_error",
            "上游拒绝 Live 请求",
        ),
        _ => HTTP.error(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            "上游 Live 请求失败",
        ),
    }
}
