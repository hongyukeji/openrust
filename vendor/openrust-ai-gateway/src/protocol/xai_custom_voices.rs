use std::{path::Path, sync::atomic::Ordering};

use http::{HeaderValue, header};
use openrust::{
    cache::limiters::ConcurrencyLease,
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client},
    routing::{Router, RoutingError},
    support::{Str, facades::Facade},
};
use openrust_ai::{AiError, TextEndpoint, XaiCustomVoiceRequest as Operation};
use serde_json::{Map, Value, json};

use super::{
    Admission, Gateway, HttpResult, Key, Protocol, Target, account_state_unavailable, admit,
    authorize, request_too_large, xai_speech,
    xai_transcription::{self, audio_container, multipart, multipart_error, spool_audio},
};
use crate::{session_affinity, usage::xai_custom_voices::Voice};

const PROTOCOL: Protocol = Protocol::OpenAi;
const SPEECH: TextEndpoint = TextEndpoint::XaiTextToSpeech;
const METADATA: &[&str] = &[
    "name",
    "description",
    "gender",
    "accent",
    "age",
    "language",
    "use_case",
    "tone",
];

pub(super) fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/custom-voices", list_route)?;
    router
        .post("/custom-voices", create_route)?
        .streaming_body(xai_transcription::MAX_BODY_BYTES);
    router.get("/custom-voices/{id}", get_route)?;
    router.patch("/custom-voices/{id}", update_route)?;
    router.delete("/custom-voices/{id}", delete_route)?;
    router.get("/custom-voices/{id}/audio", audio_route)?;
    Ok(())
}

fn list_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(list(request))
}

fn create_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(create(request))
}

fn get_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Get))
}

fn update_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Update))
}

fn delete_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Delete))
}

fn audio_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, Action::Audio))
}

#[derive(Clone, Copy)]
enum Action {
    Get,
    Update,
    Audio,
    Delete,
}

async fn create(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let name = match request.query(Some("model")) {
        Ok(Some(Value::String(name))) if !name.is_empty() => name,
        Ok(None) => match xai_speech::implicit_model(&gateway, &key, SPEECH) {
            Ok(name) => name.to_owned(),
            Err((status, code, message)) => return PROTOCOL.error(status, code, message),
        },
        _ => return invalid("model 无效"),
    };
    let Some((_, target, scope)) = targets(&gateway, &key).find(|(model, _, _)| *model == name)
    else {
        return not_found();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let (metadata, audio) = match reference_audio(request, &gateway.upload_directory).await {
        Ok(upload) => upload,
        Err(HttpError::PayloadTooLarge) => return request_too_large(PROTOCOL),
        Err(HttpError::Internal { .. }) => return unavailable(),
        Err(_) => return invalid("需要一个有效的 multipart 参考音频及原生字段"),
    };
    let lease = match acquire(&gateway, target).await {
        Ok(lease) => lease,
        Err(response) => return response,
    };
    let mut voice = Voice {
        id: Str::random(8).to_ascii_lowercase(),
        position: Str::ulid().to_string(),
        owner: owner(&key),
        scope,
        upstream_id: String::new(),
    };
    if store.reserve_xai_voice(&voice).await.is_err() {
        return unavailable();
    }
    let upstream = match send(
        target,
        Operation::Create {
            metadata: &metadata,
            audio: Box::new(audio),
        },
    )
    .await
    {
        Ok(upstream) => upstream,
        Err(response) => return response,
    };
    if let Some(lease) = lease {
        let _ = lease.release().await;
    }
    if !upstream.successful() {
        if upstream.status().is_client_error() && store.delete_xai_voice(&voice).await.is_err() {
            return unavailable();
        }
        return upstream_error(upstream.status());
    }
    let Ok(content) = upstream.json::<Value>() else {
        return malformed();
    };
    let Some(upstream_id) = content["voice_id"].as_str().filter(|id| valid_id(id)) else {
        return malformed();
    };
    voice.upstream_id = upstream_id.to_owned();
    let Some(content) = present(&voice, content) else {
        return malformed();
    };
    if store.attach_xai_voice(&voice).await.is_err() {
        return unavailable();
    }
    json_response(upstream.status(), content)
}

async fn manage(request: &mut Request, action: Action) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let Some(id) = request.route("id").filter(|id| valid_id(id)) else {
        return not_found();
    };
    let voice = match load(&gateway, &key, id).await {
        Ok(voice) => voice,
        Err(response) => return response,
    };
    let Some((_, target, _)) = targets(&gateway, &key).find(|(_, _, scope)| *scope == voice.scope)
    else {
        return not_found();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let id = &voice.upstream_id;
    let metadata;
    let operation = match action {
        Action::Get => Operation::Get { id },
        Action::Update => {
            metadata = match update_metadata(request).await {
                Ok(metadata) => metadata,
                Err(response) => return response,
            };
            Operation::Update {
                id,
                metadata: &metadata,
            }
        }
        Action::Delete => Operation::Delete { id },
        Action::Audio => Operation::Audio { id },
    };
    let lease = match acquire(&gateway, target).await {
        Ok(lease) => lease,
        Err(response) => return response,
    };
    let upstream = match send(target, operation).await {
        Ok(upstream) => upstream,
        Err(response) => return response,
    };
    if matches!(action, Action::Delete) && (upstream.successful() || upstream.not_found()) {
        if upstream.successful()
            && !upstream
                .json::<Value>()
                .is_ok_and(|data| data["deleted"] == true)
        {
            return malformed();
        }
        let store = gateway.usage_store.as_ref().expect("已读取音色存储");
        if store.delete_xai_voice(&voice).await.is_err() {
            return unavailable();
        }
        return if upstream.not_found() {
            not_found()
        } else {
            json_response(upstream.status(), json!({"deleted":true}))
        };
    }
    if !upstream.successful() {
        return upstream_error(upstream.status());
    }
    if matches!(action, Action::Audio) {
        return download(upstream, lease);
    }
    let Some(content) = upstream
        .json::<Value>()
        .ok()
        .and_then(|value| present(&voice, value))
    else {
        return malformed();
    };
    json_response(upstream.status(), content)
}

async fn list(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let Some(store) = &gateway.usage_store else {
        return unavailable();
    };
    let limit = match request.query(Some("limit")) {
        Ok(None) => 100,
        Ok(Some(Value::String(value))) => match value.parse::<u64>() {
            Ok(limit @ 1..=1000) => limit,
            _ => return invalid("limit 须为 1 至 1000"),
        },
        _ => return invalid("limit 无效"),
    };
    let cursor = match request.query(Some("pagination_token")) {
        Ok(None) => None,
        Ok(Some(Value::String(value)))
            if value.len() == 26
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()) =>
        {
            Some(value)
        }
        _ => return invalid("pagination_token 无效"),
    };
    let candidates: Vec<_> = targets(&gateway, &key).collect();
    if candidates.is_empty() {
        return not_found();
    }
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let scopes = candidates
        .iter()
        .map(|(_, _, scope)| scope.as_str().into())
        .collect();
    let mut voices = match store
        .xai_voice_page(&owner(&key), scopes, cursor.as_deref(), limit + 1)
        .await
    {
        Ok(voices) => voices,
        Err(_) => return unavailable(),
    };
    let next = if voices.len() as u64 > limit {
        voices.pop();
        voices.last().map(|voice| voice.position.clone())
    } else {
        None
    };
    let mut items = Vec::new();
    for voice in voices {
        let Some((_, target, _)) = candidates
            .iter()
            .find(|(_, _, scope)| *scope == voice.scope)
        else {
            continue;
        };
        let lease = match acquire(&gateway, target).await {
            Ok(lease) => lease,
            Err(response) => return response,
        };
        let upstream = match send(
            target,
            Operation::Get {
                id: &voice.upstream_id,
            },
        )
        .await
        {
            Ok(upstream) => upstream,
            Err(response) => return response,
        };
        if let Some(lease) = lease {
            let _ = lease.release().await;
        }
        if upstream.not_found() {
            continue;
        }
        if !upstream.successful() {
            return upstream_error(upstream.status());
        }
        let Some(content) = upstream
            .json::<Value>()
            .ok()
            .and_then(|value| present(&voice, value))
        else {
            return malformed();
        };
        items.push(content);
    }
    json_response(
        StatusCode::OK,
        json!({"voices":items,"pagination_token":next}),
    )
}

pub(super) async fn load(gateway: &Gateway, key: &Key, id: &str) -> Result<Voice, HttpResult> {
    let store = gateway.usage_store.as_ref().ok_or_else(unavailable)?;
    match store.xai_voice(&owner(key), id).await {
        Ok(Some(voice)) => Ok(voice),
        Ok(None) => Err(not_found()),
        Err(_) => Err(unavailable()),
    }
}

fn targets<'a>(
    gateway: &'a Gateway,
    key: &'a Key,
) -> impl Iterator<Item = (&'a str, &'a Target, String)> {
    gateway
        .models
        .iter()
        .filter(|(model, _)| key.allows_model(model))
        .flat_map(move |(name, model)| {
            model
                .targets_for(SPEECH, key)
                .filter_map(move |(target, _)| {
                    scope(name, target).map(|scope| (name.as_str(), target, scope))
                })
        })
}

pub(super) fn scope(model: &str, target: &Target) -> Option<String> {
    let scope = target
        .provider
        .xai_custom_voice_gateway()?
        .voice_scope()
        .ok()?;
    Some(session_affinity::key(&[
        model,
        &target.affinity_key,
        &scope,
    ]))
}

fn owner(key: &Key) -> String {
    key.usage_owner().invocation_scope("xai-custom-voices")
}

async fn acquire(
    gateway: &Gateway,
    target: &Target,
) -> Result<Option<ConcurrencyLease>, HttpResult> {
    let Some(account) = &target.account else {
        return Ok(None);
    };
    let store = gateway.account_cache_store.as_deref();
    if let Some(cooldown) = &account.cooldown
        && !matches!(cooldown.remaining(store).await, Ok(None))
    {
        return Err(account_state_unavailable(PROTOCOL));
    }
    match account.acquire(store).await {
        Ok(Admission::Allowed(lease)) => Ok(lease),
        _ => Err(account_state_unavailable(PROTOCOL)),
    }
}

async fn send(target: &Target, operation: Operation<'_>) -> Result<client::Response, HttpResult> {
    target
        .provider
        .xai_custom_voice_gateway()
        .ok_or_else(not_found)?
        .send_voice(operation)
        .await
        .map_err(|error| match error {
            AiError::Timeout => PROTOCOL.error(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_timeout",
                "上游音色请求超时",
            ),
            _ => malformed(),
        })
}

async fn reference_audio(
    request: &mut Request,
    directory: &Path,
) -> Result<(Value, client::Part), HttpError> {
    let (mut form, unread) = multipart(request)?;
    let mut metadata = Map::new();
    let mut audio = None;
    while let Some(field) = form.next_field().await.map_err(multipart_error)? {
        unread.store(0, Ordering::Relaxed);
        let name = field.name().ok_or(HttpError::InvalidRequest)?.to_owned();
        if name == "file"
            && audio.is_none()
            && field.file_name().is_some_and(|name| !name.is_empty())
        {
            audio = Some(spool_audio(field, directory, &unread).await?);
            continue;
        }
        if field.file_name().is_some()
            || !METADATA.contains(&name.as_str())
            || metadata.contains_key(&name)
        {
            return Err(HttpError::InvalidRequest);
        }
        let bytes = field.bytes().await.map_err(multipart_error)?;
        let value = std::str::from_utf8(&bytes)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(HttpError::InvalidRequest)?;
        metadata.insert(name, value.into());
    }
    let (file, prefix, length) = audio.ok_or(HttpError::InvalidRequest)?;
    let (extension, mime) = audio_container(prefix).ok_or(HttpError::InvalidRequest)?;
    let audio = client::Part::stream_with_length(file, length)
        .file_name(format!("audio.{extension}"))
        .mime_str(mime)
        .map_err(|_| HttpError::InvalidRequest)?;
    Ok((Value::Object(metadata), audio))
}

async fn update_metadata(request: &mut Request) -> Result<Value, HttpResult> {
    let body = match request.body_bytes().await {
        Ok(body) => body,
        Err(HttpError::PayloadTooLarge) => return Err(request_too_large(PROTOCOL)),
        Err(_) => return Err(invalid("无法读取音色元数据")),
    };
    let data: Map<String, Value> =
        serde_json::from_slice(&body).map_err(|_| invalid("需要 JSON 对象"))?;
    if !data.iter().all(|(key, value)| {
        METADATA.contains(&key.as_str())
            && (value.is_null()
                || value
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 1024))
    }) {
        return Err(invalid("音色元数据须为已支持的非空字符串或 null"));
    }
    Ok(Value::Object(data))
}

fn present(voice: &Voice, value: Value) -> Option<Value> {
    if value["voice_id"].as_str()? != voice.upstream_id || !value["created_at"].is_string() {
        return None;
    }
    let mut data = value.as_object()?.clone();
    data.retain(|key, value| {
        (METADATA.contains(&key.as_str()) && (value.is_null() || value.is_string()))
            || key == "created_at"
    });
    data.insert("voice_id".into(), voice.id.as_str().into());
    Some(Value::Object(data))
}

fn download(upstream: client::Response, lease: Option<ConcurrencyLease>) -> HttpResult {
    let Some(mime) = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .filter(|value| {
            value.to_str().ok().is_some_and(|value| {
                let mime = value
                    .split(';')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .to_ascii_lowercase();
                mime.starts_with("audio/")
                    || matches!(
                        mime.as_str(),
                        "video/mp4"
                            | "video/x-matroska"
                            | "video/webm"
                            | "video/quicktime"
                            | "video/x-msvideo"
                            | "application/ogg"
                    )
            })
        })
    else {
        return malformed();
    };
    let status = upstream.status();
    let Ok(stream) = upstream.into_stream() else {
        return malformed();
    };
    use futures_util::StreamExt as _;
    let stream = async_stream::stream! {
        let mut stream = stream;
        let _lease = lease;
        while let Some(chunk) = stream.next().await {
            yield chunk;
        }
    };
    Ok(Response::stream(status, stream)
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .header(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        )
        .header(header::CONTENT_TYPE, mime))
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 8
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn json_response(status: StatusCode, value: Value) -> HttpResult {
    Ok(Response::json(status, value)?
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
}

pub(super) fn not_found() -> HttpResult {
    PROTOCOL.error(StatusCode::NOT_FOUND, "voice_not_found", "音色不可用")
}

fn invalid(message: &'static str) -> HttpResult {
    PROTOCOL.error(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn unavailable() -> HttpResult {
    PROTOCOL.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "voice_unavailable",
        "音色存储暂不可用",
    )
}

fn malformed() -> HttpResult {
    PROTOCOL.error(
        StatusCode::BAD_GATEWAY,
        "invalid_upstream_response",
        "上游音色响应不可用",
    )
}

fn upstream_error(status: StatusCode) -> HttpResult {
    if status == StatusCode::NOT_FOUND {
        return not_found();
    }
    let status = match status {
        StatusCode::BAD_REQUEST
        | StatusCode::FORBIDDEN
        | StatusCode::CONFLICT
        | StatusCode::PAYLOAD_TOO_LARGE
        | StatusCode::UNPROCESSABLE_ENTITY
        | StatusCode::TOO_MANY_REQUESTS => status,
        _ => StatusCode::BAD_GATEWAY,
    };
    PROTOCOL.error(status, "upstream_error", "上游拒绝音色操作")
}
