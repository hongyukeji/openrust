use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use axum::{
    body::to_bytes,
    extract::{
        FromRequestParts,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code},
    },
};
use eventsource_stream::Eventsource;
use futures_util::{StreamExt, stream::FuturesUnordered};
use http::{HeaderMap, HeaderValue, StatusCode, header};
use openrust::{
    foundation::Application,
    http::{Body, Request, Response},
    support::facades::{App, Facade},
};
use serde_json::{Map, Value};
use tokio::sync::mpsc;

use super::{Gateway, HttpResult, Protocol, authorize, error_body, responses};

mod replay;
use replay::{Checkpoint, replay_input};

const MAX_MESSAGE: usize = 2 * 1024 * 1024;
const MAX_STREAMS: usize = 4;
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

type StreamId = Option<String>;

enum History {
    Keep,
    Replace(Option<Checkpoint>),
}

enum Frame {
    Event(Message),
    Finished {
        lane: StreamId,
        message: Message,
        history: History,
    },
}

async fn serve(
    socket: &mut WebSocket,
    app: Arc<Application>,
    headers: HeaderMap,
) -> Result<(), ()> {
    let (sender, mut receiver) = mpsc::channel(MAX_STREAMS);
    let mut running = FuturesUnordered::new();
    let mut active = HashSet::new();
    let mut checkpoints = VecDeque::new();
    let app = app.as_ref();
    let headers = &headers;
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(incoming) = incoming else { return Ok(()); };
                match incoming.map_err(|_| ())? {
                    Message::Text(text) => {
                        let mut turn = match Turn::parse(text.as_str()) {
                            Ok(turn) => turn,
                            Err(error) => {
                                write_error(socket, StatusCode::BAD_REQUEST, error.code,
                                    error.message, error.lane.as_deref()).await?;
                                continue;
                            }
                        };
                        if active.contains(&turn.lane) {
                            write_error(socket, StatusCode::CONFLICT, "response_in_progress",
                                "请等待当前通道的响应结束后再发送", turn.lane.as_deref()).await?;
                            continue;
                        }
                        if active.len() == MAX_STREAMS {
                            write_error(socket, StatusCode::TOO_MANY_REQUESTS, "too_many_streams",
                                "此连接的活动通道已达上限", turn.lane.as_deref()).await?;
                            continue;
                        }
                        if let Err(code) = turn.resume(&mut checkpoints) {
                            let message = match code {
                                "unsupported_continuation_input" => "续接仅支持文本消息与客户端函数工具",
                                "continuation_too_large" => "续接上下文超出连接限制",
                                _ => "未找到当前连接上的响应，请重发完整 input",
                            };
                            write_error(socket, StatusCode::BAD_REQUEST, code, message, turn.lane.as_deref()).await?;
                            continue;
                        }
                        active.insert(turn.lane.clone());
                        let sender = sender.clone();
                        running.push(async move {
                            let lane = turn.lane.clone();
                            let (message, history) = run(app, headers, turn, &sender).await?;
                            sender.send(Frame::Finished {lane, message, history}).await.map_err(|_| ())
                        });
                    }
                    Message::Binary(_) => return unsupported_frame(socket).await,
                    Message::Close(_) => return Ok(()),
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            finished = running.next(), if !running.is_empty() => {
                if let Some(result) = finished { result?; }
            }
            outbound = receiver.recv() => {
                let Some(outbound) = outbound else { return Ok(()); };
                let message = match outbound {
                    Frame::Event(message) => message,
                    Frame::Finished { lane, message, history } => {
                        active.remove(&lane);
                        if let History::Replace(checkpoint) = history {
                            checkpoints.retain(|(stored, _)| stored != &lane);
                            if let Some(checkpoint) = checkpoint {
                                if checkpoints.len() == MAX_STREAMS {
                                    checkpoints.pop_front();
                                }
                                checkpoints.push_back((lane, checkpoint));
                            }
                        }
                        message
                    }
                };
                send(socket, message).await?;
            }
        }
    }
}

struct Turn {
    body: Map<String, Value>,
    lane: StreamId,
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
    fn resume(
        &mut self,
        checkpoints: &mut VecDeque<(StreamId, Checkpoint)>,
    ) -> Result<(), &'static str> {
        let Some(previous) = &self.previous else {
            return Ok(());
        };
        let mut matching = checkpoints
            .iter()
            .enumerate()
            .filter(|(_, (_, checkpoint))| checkpoint.matches(previous));
        let own = matching.clone().find(|(_, (lane, _))| lane == &self.lane);
        let (index, (lane, checkpoint)) = own
            .or_else(|| {
                let only = matching.next()?;
                matching.next().is_none().then_some(only)
            })
            .ok_or("previous_response_not_found")?;
        checkpoint.resume(&mut self.body)?;
        if lane == &self.lane {
            // 续接消耗本通道的检查点。分叉保留来源通道。
            checkpoints.remove(index);
        }
        Ok(())
    }
}

async fn run(
    app: &Application,
    headers: &HeaderMap,
    turn: Turn,
    sender: &mpsc::Sender<Frame>,
) -> Result<(Message, History), ()> {
    let mut history = turn
        .body
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| {
            replay_input(turn.body.get("input")).map(|input| (model.to_owned(), input))
        });
    let lane = turn.lane.as_deref();
    let fail = |code, message| {
        Ok((
            error_frame(StatusCode::BAD_GATEWAY, code, message, lane),
            History::Keep,
        ))
    };
    let mut raw = http::Request::builder()
        .method(http::Method::POST)
        .uri("/v1/responses")
        .body(Body::from(Value::Object(turn.body).to_string()))
        .map_err(|_| ())?;
    raw.headers_mut().clone_from(headers);
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    let Ok(response) = app.scope(responses(&mut request)).await else {
        return fail("upstream_error", "无法执行 Responses 请求");
    };
    let (parts, body) = response.into_parts();
    if !parts.status.is_success() {
        let value = to_bytes(body, MAX_MESSAGE)
            .await
            .ok()
            .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
            .unwrap_or_default();
        return Ok((
            forward_error(parts.status, &value["error"], lane),
            History::Keep,
        ));
    }
    let mut events = body.into_data_stream().eventsource();
    while let Some(Ok(event)) = events.next().await {
        if event.data == "[DONE]" {
            return fail("incomplete_stream", "上游缺少 Responses 终止事件");
        }
        let Ok(mut data) = serde_json::from_str::<Map<String, Value>>(&event.data) else {
            return fail("invalid_upstream_response", "Responses 事件格式无效");
        };
        if let Some(error) = data.get("error").filter(|error| !error.is_null()) {
            return Ok((
                forward_error(StatusCode::BAD_GATEWAY, error, lane),
                History::Keep,
            ));
        }
        let Some(kind) = data.get("type").and_then(Value::as_str) else {
            return fail("invalid_upstream_response", "Responses 事件缺少类型");
        };
        let terminal = match kind {
            "response.completed" => Some(History::Replace(history.take().and_then(
                |(model, input)| {
                    Checkpoint::from_complete(
                        data.get("response").unwrap_or(&Value::Null),
                        model,
                        input,
                    )
                },
            ))),
            "response.incomplete" | "response.failed" => Some(History::Keep),
            _ => None,
        };
        if let Some(lane) = lane {
            data.insert("stream_id".into(), Value::String(lane.into()));
        } else {
            data.remove("stream_id");
        }
        let message = Message::Text(Value::Object(data).to_string().into());
        if let Some(terminal) = terminal {
            return Ok((message, terminal));
        }
        sender.send(Frame::Event(message)).await.map_err(|_| ())?;
    }
    fail("incomplete_stream", "上游未完成 Responses 流")
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

fn forward_error(status: StatusCode, error: &Value, lane: Option<&str>) -> Message {
    error_frame(
        status,
        error["code"].as_str().unwrap_or("upstream_error"),
        error["message"].as_str().unwrap_or("Responses 请求未完成"),
        lane,
    )
}

async fn write_error(
    socket: &mut WebSocket,
    status: StatusCode,
    code: &str,
    message: &str,
    lane: Option<&str>,
) -> Result<(), ()> {
    send(socket, error_frame(status, code, message, lane)).await
}

fn error_frame(status: StatusCode, code: &str, message: &str, lane: Option<&str>) -> Message {
    let mut event = error_body(code, message);
    event["type"] = "error".into();
    event["status"] = status.as_u16().into();
    if let Some(lane) = lane {
        event["stream_id"] = lane.into();
    }
    Message::Text(event.to_string().into())
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}
