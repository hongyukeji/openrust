use http::{HeaderValue, Method};
use openrust::http::{
    HttpFuture,
    client::{WebSocket, WebSocketConfig},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult,
    connection::Connection,
    text::{transport_error, validate},
};

pub(crate) const MAX_SIGNAL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LiveProtocol {
    Codex,
    OpenAi,
}

pub struct LiveCall {
    pub id: String,
    pub sdp: String,
}

/// 复用已有具名连接创建 WebRTC 呼叫，随后打开同一上游账号的控制通道。
pub trait LiveGateway: Send + Sync {
    fn protocol(&self) -> LiveProtocol;
    fn live_scope(&self) -> AiResult<String>;
    fn create_live_call<'a>(
        &'a self,
        sdp: &'a str,
        session: &'a Value,
    ) -> HttpFuture<'a, AiResult<LiveCall>>;
    fn connect_live_sideband<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<WebSocket>>;
    fn hangup_live_call<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<()>>;
}

impl LiveGateway for Connection {
    fn protocol(&self) -> LiveProtocol {
        self.live_protocol().expect("Live capability was checked")
    }

    fn live_scope(&self) -> AiResult<String> {
        self.request_namespace(match self.protocol() {
            LiveProtocol::Codex => "codex-live",
            LiveProtocol::OpenAi => "openai-live",
        })
    }

    fn create_live_call<'a>(
        &'a self,
        sdp: &'a str,
        session: &'a Value,
    ) -> HttpFuture<'a, AiResult<LiveCall>> {
        Box::pin(async move {
            if !valid_sdp(sdp) || !session.is_object() {
                return Err(AiError::InvalidPrompt);
            }
            let request = self.live_request()?;
            let (id, sdp) = match self.protocol() {
                LiveProtocol::Codex => {
                    let response = validate(
                        request
                            .accept(HeaderValue::from_static("application/sdp"))
                            .post(
                                "realtime/calls?intent=quicksilver&architecture=avas",
                                &json!({"sdp":sdp,"session":session}),
                            )
                            .await,
                    )?;
                    let id = response
                        .header("location")
                        .and_then(|value| value.to_str().ok())
                        .and_then(location_id)
                        .ok_or(AiError::InvalidResponse)?;
                    let body = response.body().map_err(|_| AiError::InvalidResponse)?;
                    let sdp = std::str::from_utf8(&body).map_err(|_| AiError::InvalidResponse)?;
                    (id, sdp.to_owned())
                }
                LiveProtocol::OpenAi => {
                    let response = validate(request.post("live/sessions", &json!({
                        "session": session, "transport": { "type": "webrtc", "sdp": sdp },
                    })).await)?;
                    let data: Value = response.json().map_err(|_| AiError::InvalidResponse)?;
                    let id = data["session"]["id"]
                        .as_str()
                        .ok_or(AiError::InvalidResponse)?;
                    let transport = &data["transport"];
                    if transport["type"] != "webrtc" {
                        return Err(AiError::InvalidResponse);
                    }
                    let sdp = transport["sdp"].as_str().ok_or(AiError::InvalidResponse)?;
                    (id.to_owned(), sdp.to_owned())
                }
            };
            if !valid_id(&id) || !valid_sdp(&sdp) {
                return Err(AiError::InvalidResponse);
            }
            Ok(LiveCall { id, sdp })
        })
    }

    fn connect_live_sideband<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<WebSocket>> {
        Box::pin(async move {
            if !valid_id(id) {
                return Err(AiError::InvalidPrompt);
            }
            let path = match self.protocol() {
                LiveProtocol::Codex => id.to_owned(),
                LiveProtocol::OpenAi => format!("live/sessions/{id}/attach"),
            };
            let config = WebSocketConfig::default()
                .max_frame_size(Some(MAX_SIGNAL_BYTES))
                .max_message_size(Some(MAX_SIGNAL_BYTES))
                .max_write_buffer_size(2 * MAX_SIGNAL_BYTES);
            self.live_request()?
                .connect_websocket(&path, config)
                .await
                .map_err(transport_error)
        })
    }

    fn hangup_live_call<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<()>> {
        Box::pin(async move {
            if self.protocol() != LiveProtocol::OpenAi {
                return Err(AiError::EndpointNotSupported);
            }
            if !valid_id(id) {
                return Err(AiError::InvalidPrompt);
            }
            validate(
                self.live_request()?
                    .send(Method::POST, &format!("live/sessions/{id}/hangup"))
                    .await,
            )?;
            Ok(())
        })
    }
}

fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn location_id(value: &str) -> Option<String> {
    let location = value.parse::<http::Uri>().ok()?;
    let id = location.path().trim_end_matches('/').rsplit_once('/')?.1;
    // Location 只提供资源标识。后续连接仍访问已配置的供应商地址。
    (id != "codex" && valid_id(id)).then(|| id.to_owned())
}

fn valid_sdp(sdp: &str) -> bool {
    sdp.len() <= MAX_SIGNAL_BYTES && sdp.lines().next() == Some("v=0") && !sdp.contains('\0')
}
