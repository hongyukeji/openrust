use std::{sync::Arc, time::Duration};

use axum::{
    body::to_bytes,
    extract::{
        FromRequestParts,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code},
    },
};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use openrust::{
    foundation::Application,
    http::{Body, Request, Response},
    support::facades::{App, Facade},
};
use serde_json::{Map, Value};

use super::{Gateway, HttpResult, Protocol, authorize, error_body, responses};

mod replay;
use replay::{Checkpoint, replay_input};

const MAX_MESSAGE: usize = 2 * 1024 * 1024;
const CONNECTION_MAX_AGE: Duration = Duration::from_secs(60 * 60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn handshake(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let protocol = Protocol::OpenAi;
    let token = protocol.token(request);
    if let Err(response) = authorize(&gateway, token, protocol).await {
        return response;
    }
    let mut headers = HeaderMap::new();
    for name in [
        header::AUTHORIZATION,
        http::HeaderName::from_static("x-session-id"),
    ] {
        for value in request.headers().get_all(&name) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let Ok(upgrade) = WebSocketUpgrade::from_request_parts(request.parts_mut(), &()).await else {
        return protocol.error(
            StatusCode::UPGRADE_REQUIRED,
            "websocket_upgrade_required",
            "请使用 WebSocket 连接 Responses",
        );
    };
    let app = App::get_facade_root()?;
    Ok(Response::from(
        upgrade
            .max_message_size(MAX_MESSAGE)
            .max_frame_size(MAX_MESSAGE)
            .max_write_buffer_size(MAX_MESSAGE * 2)
            .on_upgrade(move |mut socket| async move {
                if tokio::time::timeout(CONNECTION_MAX_AGE, serve(&mut socket, app, headers))
                    .await
                    .is_err()
                {
                    let _ = write_error(
                        &mut socket,
                        StatusCode::BAD_REQUEST,
                        "websocket_connection_limit_reached",
                        "请重新建立 WebSocket 连接",
                        None,
                    )
                    .await;
                    let _ = send(
                        &mut socket,
                        Message::Close(Some(CloseFrame {
                            code: close_code::NORMAL,
                            reason: "连接时限已到".into(),
                        })),
                    )
                    .await;
                }
            }),
    ))
}

async fn serve(
    socket: &mut WebSocket,
    app: Arc<Application>,
    headers: HeaderMap,
) -> Result<(), ()> {
    let mut checkpoint = None;
    while let Some(message) = socket.recv().await {
        match message.map_err(|_| ())? {
            Message::Text(text) => match Turn::parse(text.as_str()) {
                Ok(turn) => run(socket, &app, &headers, turn, &mut checkpoint).await?,
                Err(error) => {
                    write_error(
                        socket,
                        StatusCode::BAD_REQUEST,
                        error.code,
                        error.message,
                        error.lane.as_deref(),
                    )
                    .await?
                }
            },
            Message::Binary(_) => return unsupported_frame(socket).await,
            Message::Close(_) => return Ok(()),
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }
    Ok(())
}

struct Turn {
    body: Map<String, Value>,
    lane: Option<String>,
    previous: Option<String>,
}

struct InvalidTurn {
    code: &'static str,
    message: &'static str,
    lane: Option<String>,
}

impl Turn {
    fn parse(text: &str) -> Result<Self, InvalidTurn> {
        let mut body =
            serde_json::from_str::<Map<String, Value>>(text).map_err(|_| InvalidTurn {
                code: "invalid_json",
                message: "WebSocket 请求必须是 JSON 对象",
                lane: None,
            })?;
        let lane = match body.remove("stream_id") {
            None => None,
            Some(Value::String(lane))
                if !lane.is_empty()
                    && lane.len() <= 256
                    && lane.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    }) =>
            {
                Some(lane)
            }
            _ => {
                return Err(InvalidTurn {
                    code: "invalid_stream_id",
                    message: "stream_id 无效",
                    lane: None,
                });
            }
        };
        let fail = |code, message| InvalidTurn {
            code,
            message,
            lane: lane.clone(),
        };
        if body.remove("type").as_ref().and_then(Value::as_str) != Some("response.create") {
            return Err(fail(
                "unsupported_event",
                "此 WebSocket 接收 response.create 事件",
            ));
        }
        if body
            .remove("generate")
            .is_some_and(|value| value != Value::Bool(true) && !value.is_null())
        {
            return Err(fail("unsupported_parameter", "当前连接未启用上下文预热"));
        }
        let previous = match body.remove("previous_response_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => Some(id),
            _ => {
                return Err(fail(
                    "invalid_previous_response_id",
                    "previous_response_id 无效",
                ));
            }
        };
        body.insert("stream".into(), Value::Bool(true));
        Ok(Self {
            body,
            lane,
            previous,
        })
    }
}

async fn run(
    socket: &mut WebSocket,
    app: &Application,
    headers: &HeaderMap,
    mut turn: Turn,
    checkpoint: &mut Option<Checkpoint>,
) -> Result<(), ()> {
    if let Some(previous) = &turn.previous {
        let resumed = checkpoint
            .as_ref()
            .ok_or("previous_response_not_found")
            .and_then(|checkpoint| {
                checkpoint.resume(previous, turn.lane.as_deref(), &mut turn.body)
            });
        if let Err(code) = resumed {
            let message = match code {
                "unsupported_continuation_input" => "续接仅支持文本消息与客户端函数工具",
                "continuation_too_large" => "续接上下文超出连接限制",
                _ => "未找到当前连接上的响应，请重发完整 input",
            };
            return write_error(
                socket,
                StatusCode::BAD_REQUEST,
                code,
                message,
                turn.lane.as_deref(),
            )
            .await;
        }
        // 当前续接消耗检查点，失败后由客户端重新发送完整输入。
        checkpoint.take();
    }
    let mut history = turn
        .body
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| {
            replay_input(turn.body.get("input")).map(|input| (model.to_owned(), input))
        });
    let mut raw = http::Request::builder()
        .method(http::Method::POST)
        .uri("/v1/responses")
        .body(Body::from(Value::Object(turn.body).to_string()))
        .map_err(|_| ())?;
    raw.headers_mut().clone_from(headers);
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    let pending = app.scope(responses(&mut request));
    tokio::pin!(pending);
    let response = loop {
        tokio::select! {
            incoming = socket.recv() => control(socket, incoming, turn.lane.as_deref()).await?,
            result = &mut pending => break result,
        }
    };
    let response = match response {
        Ok(response) => response,
        Err(_) => {
            return write_error(
                socket,
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "无法执行 Responses 请求",
                turn.lane.as_deref(),
            )
            .await;
        }
    };
    let (parts, body) = response.into_parts();
    if !parts.status.is_success() {
        let value = to_bytes(body, MAX_MESSAGE)
            .await
            .ok()
            .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
            .unwrap_or_default();
        let error = &value["error"];
        return forward_error(socket, parts.status, error, turn.lane.as_deref()).await;
    }
    let mut events = body.into_data_stream().eventsource();
    loop {
        tokio::select! {
            incoming = socket.recv() => control(socket, incoming, turn.lane.as_deref()).await?,
            event = events.next() => {
                let Some(Ok(event)) = event else {
                    return write_error(socket, StatusCode::BAD_GATEWAY, "incomplete_stream", "上游未完成 Responses 流", turn.lane.as_deref()).await;
                };
                if event.data == "[DONE]" {
                    return write_error(socket, StatusCode::BAD_GATEWAY, "incomplete_stream", "上游缺少 Responses 终止事件", turn.lane.as_deref()).await;
                }
                let Ok(mut data) = serde_json::from_str::<Map<String, Value>>(&event.data) else {
                    return write_error(socket, StatusCode::BAD_GATEWAY, "invalid_upstream_response", "Responses 事件格式无效", turn.lane.as_deref()).await;
                };
                if let Some(error) = data.get("error").filter(|error| !error.is_null()) {
                    return forward_error(socket, StatusCode::BAD_GATEWAY, error, turn.lane.as_deref()).await;
                }
                let Some(kind) = data.get("type").and_then(Value::as_str) else {
                    return write_error(socket, StatusCode::BAD_GATEWAY, "invalid_upstream_response", "Responses 事件缺少类型", turn.lane.as_deref()).await;
                };
                let is_terminal = matches!(kind,
                    "response.completed" | "response.incomplete" | "response.failed");
                if kind == "response.completed" {
                    *checkpoint = history.take().and_then(|(model, input)| {
                        Checkpoint::from_complete(data.get("response").unwrap_or(&Value::Null), turn.lane.clone(), model, input)
                    });
                }
                if let Some(lane) = &turn.lane {
                    data.insert("stream_id".into(), Value::String(lane.clone()));
                } else {
                    data.remove("stream_id");
                }
                send(socket, Message::Text(Value::Object(data).to_string().into())).await?;
                if is_terminal {
                    return Ok(());
                }
            }
        }
    }
}

async fn control(
    socket: &mut WebSocket,
    incoming: Option<Result<Message, axum::Error>>,
    lane: Option<&str>,
) -> Result<(), ()> {
    match incoming {
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(()),
        Some(Ok(Message::Text(_))) => {
            write_error(
                socket,
                StatusCode::CONFLICT,
                "response_in_progress",
                "请等待当前响应结束后再发送",
                lane,
            )
            .await
        }
        Some(Ok(Message::Binary(_))) => {
            unsupported_frame(socket).await?;
            Err(())
        }
        _ => Err(()),
    }
}

async fn unsupported_frame(socket: &mut WebSocket) -> Result<(), ()> {
    send(
        socket,
        Message::Close(Some(CloseFrame {
            code: close_code::UNSUPPORTED,
            reason: "Responses 只接受文本消息".into(),
        })),
    )
    .await
}

async fn forward_error(
    socket: &mut WebSocket,
    status: StatusCode,
    error: &Value,
    lane: Option<&str>,
) -> Result<(), ()> {
    write_error(
        socket,
        status,
        error["code"].as_str().unwrap_or("upstream_error"),
        error["message"].as_str().unwrap_or("Responses 请求未完成"),
        lane,
    )
    .await
}

async fn write_error(
    socket: &mut WebSocket,
    status: StatusCode,
    code: &str,
    message: &str,
    lane: Option<&str>,
) -> Result<(), ()> {
    let mut event = error_body(code, message);
    event["type"] = "error".into();
    event["status"] = status.as_u16().into();
    if let Some(lane) = lane {
        event["stream_id"] = lane.into();
    }
    send(socket, Message::Text(event.to_string().into())).await
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}
