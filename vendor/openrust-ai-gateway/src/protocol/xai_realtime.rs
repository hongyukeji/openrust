use std::{sync::Arc, time::Duration};

use axum::extract::{
    FromRequestParts,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use futures_util::{SinkExt, StreamExt};
use openrust::{
    http::{
        HttpFuture, Request, Response, StatusCode,
        client::{WebSocket as Upstream, WebSocketMessage},
    },
    support::{Str, facades::Facade},
};
use openrust_ai::{AiError, TextEndpoint};
use serde_json::{Map, Value, json};

use super::{
    Admission, Call, Gateway, HttpResult, Key, Protocol, account_state_unavailable, admit,
    authorize, implicit_model, live::close_upstream, xai_custom_voices, xai_speech,
    xai_transcription::duration_ms,
};
use crate::{gateway::AttemptAdmission, invocation::CallIdentity};

const HTTP: Protocol = Protocol::OpenAi;
const ENDPOINT: TextEndpoint = TextEndpoint::XaiRealtime;
const DEADLINE: Duration = Duration::from_secs(60 * 60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn handshake(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(connect(request))
}

async fn connect(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, HTTP.token(request), HTTP).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let query = match request.query(None) {
        Ok(Some(Value::Object(query))) if query.keys().all(|key| key == "model") => query,
        Ok(Some(Value::Array(query))) if query.is_empty() => Map::new(),
        _ => {
            return HTTP.error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Realtime 只接收 model 查询参数",
            );
        }
    };
    let name = match query.get("model") {
        Some(Value::String(name)) if !name.is_empty() => name,
        None => match implicit_model(&gateway, &key, ENDPOINT) {
            Ok(name) => name,
            Err((status, code, message)) => return HTTP.error(status, code, message),
        },
        _ => {
            return HTTP.error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Realtime model 无效",
            );
        }
    };
    let Some(model) = gateway.models.get(name).filter(|_| key.allows_model(name)) else {
        return not_found();
    };
    if !model.supports_for(ENDPOINT, &key) {
        return not_found();
    }
    let Ok(upgrade) = WebSocketUpgrade::from_request_parts(request.parts_mut(), &()).await else {
        return HTTP.error(
            StatusCode::UPGRADE_REQUIRED,
            "websocket_upgrade_required",
            "请使用 WebSocket 连接 Realtime",
        );
    };
    if let Some(response) = admit(&key, HTTP).await? {
        return Ok(response);
    }
    let request_id = Str::ulid().to_string();
    let mut budget_blocked = false;
    let mut rejected = None;
    for (target, _) in model.targets_for(ENDPOINT, &key) {
        let Some(adapter) = target.provider.xai_realtime_gateway() else {
            continue;
        };
        if key.live_has_budget_or_quota(target) {
            budget_blocked = true;
            continue;
        }
        let lease = if let Some(account) = &target.account {
            let cache = gateway.account_cache_store.as_deref();
            if let Some(cooldown) = &account.cooldown {
                match cooldown.remaining(cache).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => {}
                    Err(_) => return account_state_unavailable(HTTP),
                }
            }
            match account.acquire_for(cache, DEADLINE.as_secs() + 60).await {
                Ok(Admission::Allowed(lease)) => lease,
                Ok(Admission::Busy) => continue,
                Err(_) => return account_state_unavailable(HTTP),
            }
        } else {
            None
        };
        let Ok(mut calls) = gateway
            .usage(
                &key,
                name,
                target,
                (ENDPOINT, ENDPOINT),
                CallIdentity {
                    request_id: &request_id,
                    invocation: None,
                },
                AttemptAdmission {
                    input_characters: None,
                    quota: None,
                    budget: Default::default(),
                    count: 1,
                    charges: None,
                    resource_id: None,
                    lease,
                    operation: "realtime",
                    hide_chat_usage: false,
                },
            )
            .await
        else {
            return usage_unavailable();
        };
        let mut call = calls.pop().expect("Realtime 只登记一次握手");
        if !call.begin_dispatch().await {
            return usage_unavailable();
        }
        let mut upstream = match adapter.connect_xai_realtime(&target.upstream_model).await {
            Ok(upstream) => upstream,
            Err(error) => {
                call.outcome("upstream_error");
                if let AiError::ProviderStatus(code) = error
                    && let Ok(status) = StatusCode::from_u16(code)
                {
                    call.status(status);
                }
                if !call.settle().await {
                    return usage_unavailable();
                }
                call.release().await;
                if matches!(
                    error,
                    AiError::MissingCredential | AiError::ProviderStatus(401 | 403 | 429)
                ) {
                    rejected = Some(error);
                    continue;
                }
                return upstream_error(error);
            }
        };
        call.status(StatusCode::SWITCHING_PROTOCOLS);
        call.outcome("incomplete_stream");
        if !call.persist_observation().await {
            close_upstream(&mut upstream).await;
            return usage_unavailable();
        }
        let session = Session {
            public_model: name.to_owned(),
            provider: target.provider_name.clone(),
            gateway: gateway.clone(),
            key: key.clone(),
        };
        return Ok(Response::from(
            upgrade
                .max_frame_size(MAX_BYTES)
                .max_message_size(MAX_BYTES)
                .max_write_buffer_size(2 * MAX_BYTES)
                .on_upgrade(move |mut client| async move {
                    let end = tokio::time::timeout(
                        DEADLINE,
                        bridge(&mut client, &mut upstream, session, &mut call),
                    )
                    .await
                    .unwrap_or("session_expired");
                    call.outcome(end);
                    let settled = call.settle().await;
                    close_upstream(&mut upstream).await;
                    call.release().await;
                    if !settled || end == "usage_unavailable" {
                        let _ = send_client(
                            &mut client,
                            error_event("usage_unavailable", "无法保存实时会话用量"),
                        )
                        .await;
                    }
                    let _ = send_client(&mut client, Message::Close(None)).await;
                }),
        ));
    }
    if let Some(error) = rejected {
        upstream_error(error)
    } else if budget_blocked {
        HTTP.error(
            StatusCode::BAD_REQUEST,
            "budget_unbounded",
            "当前预算或配额无法预占 Realtime 会话用量",
        )
    } else {
        account_state_unavailable(HTTP)
    }
}

struct Session {
    public_model: String,
    provider: String,
    gateway: Arc<Gateway>,
    key: Arc<Key>,
}

impl Session {
    async fn client_event(&self, text: &str) -> Option<String> {
        let mut event = serde_json::from_str::<Value>(text).ok()?;
        let event_type = event["type"].as_str()?;
        if event.get("conversation_id").is_some() {
            return None;
        }
        match event_type {
            "session.update" => {
                let settings = event.get_mut("session")?.as_object_mut()?;
                self.check_settings(settings).await?;
            }
            "response.create" => {
                if event.get("response").is_some() {
                    let settings = event.get_mut("response")?.as_object_mut()?;
                    self.check_settings(settings).await?;
                }
            }
            "response.cancel"
            | "input_audio_buffer.append"
            | "input_audio_buffer.commit"
            | "input_audio_buffer.clear"
            | "conversation.item.create"
            | "conversation.item.delete"
            | "conversation.item.truncate" => {}
            _ => return None,
        }
        Some(event.to_string())
    }

    async fn check_settings(&self, settings: &mut Map<String, Value>) -> Option<()> {
        if settings.contains_key("conversation_id")
            || settings
                .get("resumption")
                .is_some_and(|value| value != &json!({"enabled":false}))
            || settings
                .remove("model")
                .is_some_and(|value| value.as_str() != Some(&self.public_model))
        {
            return None;
        }
        if let Some(tools) = settings.get("tools") {
            let model = self.gateway.models.get(&self.public_model)?;
            if !tools
                .as_array()?
                .iter()
                .all(|tool| match tool["type"].as_str() {
                    Some("function") => true,
                    Some("web_search" | "x_search" | "file_search") => {
                        model.allows_server_tool(tool["type"].as_str().unwrap_or_default(), tool)
                    }
                    _ => false,
                })
            {
                return None;
            }
        }
        if let Some(voice) = settings.get_mut("voice") {
            let id = voice.as_str()?;
            if !xai_speech::builtin_voice(id) {
                if !xai_custom_voices::valid_id(id) {
                    return None;
                }
                let record = xai_custom_voices::load(&self.gateway, &self.key, id)
                    .await
                    .ok()?;
                let allowed = self
                    .gateway
                    .models
                    .iter()
                    .filter(|(name, _)| self.key.allows_model(name))
                    .any(|(name, model)| {
                        model
                            .targets_for(TextEndpoint::XaiTextToSpeech, &self.key)
                            .any(|(target, _)| {
                                target.provider_name == self.provider
                                    && xai_custom_voices::scope(name, target).as_ref()
                                        == Some(&record.scope)
                            })
                    });
                if !allowed {
                    return None;
                }
                *voice = record.upstream_id.into();
            }
        }
        Some(())
    }
}

async fn bridge(
    client: &mut WebSocket,
    upstream: &mut Upstream,
    session: Session,
    call: &mut Call,
) -> &'static str {
    let mut billed = None;
    loop {
        tokio::select! {
            incoming = client.recv() => {
                let message = match incoming {
                    Some(Ok(Message::Text(text))) => match session.client_event(text.as_str()).await {
                        Some(text) => WebSocketMessage::Text(text.into()),
                        None => {
                            if !send_client(client, error_event("invalid_realtime_event", "该实时事件或资源不可用")).await {
                                return "client_disconnected";
                            }
                            continue;
                        }
                    },
                    Some(Ok(Message::Binary(bytes))) => WebSocketMessage::Binary(bytes),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => return "client_disconnected",
                };
                if !matches!(tokio::time::timeout(WRITE_TIMEOUT, upstream.send(message)).await, Ok(Ok(()))) {
                    return "incomplete_stream";
                }
            }
            incoming = upstream.next() => {
                let message = match incoming {
                    Some(Ok(WebSocketMessage::Text(text))) => {
                        let Ok(mut event) = serde_json::from_str::<Value>(text.as_str()) else {
                            return "invalid_upstream_response";
                        };
                        if event["type"] == "response.done"
                            && let Some(duration) = event_duration(&event)
                            && billed.is_none_or(|prior| duration > prior)
                        {
                            billed = Some(duration);
                            call.observe_realtime_duration(duration);
                            if !call.persist_observation().await { return "usage_unavailable"; }
                        }
                        if event["type"] == "error" {
                            error_event("upstream_error", "上游拒绝实时事件")
                        } else {
                            alias_event(&mut event, &session.public_model);
                            Message::Text(event.to_string().into())
                        }
                    }
                    Some(Ok(WebSocketMessage::Binary(bytes))) => Message::Binary(bytes),
                    Some(Ok(WebSocketMessage::Close(_))) => return "session_ended",
                    Some(Ok(_)) => continue,
                    _ => return "incomplete_stream",
                };
                if !send_client(client, message).await { return "client_disconnected"; }
            }
        }
    }
}

fn event_duration(event: &Value) -> Option<i64> {
    let nested = event.pointer("/response/usage/billable_audio_seconds");
    let top = event.pointer("/usage/billable_audio_seconds");
    match (nested, top) {
        (Some(a), Some(b)) if duration_ms(a) != duration_ms(b) => None,
        (Some(value), _) | (_, Some(value)) => duration_ms(value),
        _ => None,
    }
}

fn alias_event(event: &mut Value, alias: &str) {
    for path in ["/model", "/session/model", "/response/model"] {
        if let Some(value) = event.pointer_mut(path).filter(|value| value.is_string()) {
            *value = alias.into();
        }
    }
}

async fn send_client(client: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(WRITE_TIMEOUT, client.send(message)).await,
        Ok(Ok(()))
    )
}

fn error_event(code: &str, message: &str) -> Message {
    Message::Text(
        json!({"type":"error","error":{"code":code,"message":message}})
            .to_string()
            .into(),
    )
}

fn not_found() -> HttpResult {
    HTTP.error(
        StatusCode::NOT_FOUND,
        "model_not_found",
        "Realtime 模型不可用",
    )
}

fn usage_unavailable() -> HttpResult {
    HTTP.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_unavailable",
        "实时会话用量存储暂不可用",
    )
}

fn upstream_error(error: AiError) -> HttpResult {
    match error {
        AiError::Timeout => HTTP.error(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "上游实时会话握手超时",
        ),
        AiError::ProviderStatus(429) => HTTP.error(
            StatusCode::TOO_MANY_REQUESTS,
            "upstream_error",
            "上游实时会话达到限额",
        ),
        _ => HTTP.error(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            "上游实时会话握手失败",
        ),
    }
}
