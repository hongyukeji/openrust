use std::sync::Mutex;

use http::{HeaderMap, HeaderValue, StatusCode, header};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
    },
};

use super::{Error, Response, Result, transport_error};

pub use tokio_tungstenite::tungstenite::{Message as WebSocketMessage, protocol::WebSocketConfig};

pub type WebSocket = WebSocketStream<reqwest::Upgraded>;

pub(super) struct Upgrade {
    key: String,
    stream: Mutex<Option<reqwest::Upgraded>>,
}

impl Upgrade {
    pub(super) fn new() -> Self {
        Self {
            key: generate_key(),
            stream: Mutex::new(None),
        }
    }

    pub(super) fn request_headers(&self, headers: &mut HeaderMap) {
        headers.remove(header::SEC_WEBSOCKET_PROTOCOL);
        headers.remove(header::SEC_WEBSOCKET_EXTENSIONS);
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
        headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(
            header::SEC_WEBSOCKET_VERSION,
            HeaderValue::from_static("13"),
        );
        headers.insert(
            header::SEC_WEBSOCKET_KEY,
            HeaderValue::from_str(&self.key).expect("WebSocket key is Base64"),
        );
    }

    pub(super) async fn accept(&self, mut response: reqwest::Response) -> Result<Response> {
        let headers = std::mem::take(response.headers_mut());
        let accept = derive_accept_key(self.key.as_bytes());
        let mut challenge = headers.get_all(header::SEC_WEBSOCKET_ACCEPT).iter();
        if !has_token(&headers, header::UPGRADE, "websocket")
            || !has_token(&headers, header::CONNECTION, "upgrade")
            || challenge
                .next()
                .is_none_or(|value| value.as_bytes() != accept.as_bytes())
            || challenge.next().is_some()
            || headers.contains_key(header::SEC_WEBSOCKET_PROTOCOL)
            || headers.contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
        {
            return Err(Error::InvalidWebSocketUpgrade);
        }

        let stream = response.upgrade().await.map_err(transport_error)?;
        *self
            .stream
            .lock()
            .map_err(|_| Error::InvalidWebSocketUpgrade)? = Some(stream);
        Ok(Response::new(
            StatusCode::SWITCHING_PROTOCOLS,
            headers,
            Vec::new(),
        ))
    }

    pub(super) async fn open(
        self,
        response: Response,
        config: WebSocketConfig,
    ) -> Result<WebSocket> {
        if response.status() != StatusCode::SWITCHING_PROTOCOLS {
            return Err(Error::RequestFailed(Box::new(response)));
        }
        let stream = self
            .stream
            .into_inner()
            .map_err(|_| Error::InvalidWebSocketUpgrade)?
            .ok_or(Error::InvalidWebSocketUpgrade)?;
        Ok(WebSocketStream::from_raw_socket(stream, Role::Client, Some(config)).await)
    }
}

fn has_token(headers: &HeaderMap, name: http::HeaderName, token: &str) -> bool {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim().eq_ignore_ascii_case(token))
}
