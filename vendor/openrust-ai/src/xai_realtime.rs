use openrust::http::{
    HttpFuture,
    client::{WebSocket, WebSocketConfig},
};

use crate::{AiError, AiResult, connection::Connection, text::transport_error};

/// 使用具名 xAI 连接建立语音对话 WebSocket。
pub trait XaiRealtimeGateway: Send + Sync {
    fn connect_xai_realtime<'a>(&'a self, model: &'a str) -> HttpFuture<'a, AiResult<WebSocket>>;
}

impl XaiRealtimeGateway for Connection {
    fn connect_xai_realtime<'a>(&'a self, model: &'a str) -> HttpFuture<'a, AiResult<WebSocket>> {
        Box::pin(async move {
            if model.trim().is_empty() || model.len() > 255 || model.chars().any(char::is_control) {
                return Err(AiError::InvalidPrompt);
            }
            let limit = crate::live::MAX_SIGNAL_BYTES;
            self.request(None)?
                .max_response_bytes(limit)
                .with_query_parameters([("model".into(), model.to_owned())])
                .connect_websocket(
                    "realtime",
                    WebSocketConfig::default()
                        .max_frame_size(Some(limit))
                        .max_message_size(Some(limit))
                        .max_write_buffer_size(2 * limit),
                )
                .await
                .map_err(transport_error)
        })
    }
}
