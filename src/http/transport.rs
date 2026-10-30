//! 由实际连接与可信代理确定请求的安全传输属性。
use std::net::SocketAddr;

use crate::Result;
use axum::extract::{Request, connect_info::ConnectInfo};

/// 原生 TLS 适配器在进入应用 Router 前写入的可信传输属性。
///
/// 它优先于代理头和 URI，不应由客户端输入或未受信任的请求头构造。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestSecurity(bool);
impl RequestSecurity {
    /// 由连接适配器声明实际安全状态。
    pub const fn new(secure: bool) -> Self {
        Self(secure)
    }
    /// 当前传输是否安全。
    pub const fn is_secure(self) -> bool {
        self.0
    }
}

pub(crate) fn is_secure(request: &Request) -> Result<bool> {
    is_secure_parts(request.headers(), request.extensions())
}

pub(crate) fn is_secure_parts(
    headers: &axum::http::HeaderMap,
    extensions: &axum::http::Extensions,
) -> Result<bool> {
    if let Some(security) = extensions.get::<RequestSecurity>() {
        return Ok(security.is_secure());
    }
    let peer = extensions.get::<ConnectInfo<SocketAddr>>();
    let state = extensions.get::<crate::core::AppState>();
    if let (Some(peer), Some(state)) = (peer, state)
        && super::client_ip::trusts_peer(peer.0.ip(), &state.config().server.trusted_proxy_ranges)?
        && let Some(value) = headers.get("x-forwarded-proto")
    {
        // 与 HeaderBag 首物理值、可信值列表首 token 的语义一致。
        let value = std::str::from_utf8(value.as_bytes()).unwrap_or_default();
        let first = value
            .split(',')
            .next()
            .unwrap_or_default()
            .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
            .to_ascii_lowercase();
        return Ok(matches!(first.as_str(), "https" | "on" | "ssl" | "1"));
    }
    Ok(false)
}
