//! 由实际连接与可信代理确定请求的安全传输属性。
use crate::Result;
use axum::extract::Request;

/// 原生 TLS 适配器在进入应用 Router 前写入的可信传输属性。
///
/// 它是实际 TLS 回退状态；来自可信代理的协议头按 Symfony 规则优先。
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

/// 受信宿主声明此请求来自 Laravel Cloud；普通 HTTP 头不能设置此标记。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CloudIngress;

pub(crate) fn is_secure(request: &Request) -> Result<bool> {
    request
        .extensions()
        .get::<crate::http::proxy::ProxySnapshot>()
        .cloned()
        .unwrap_or_else(|| {
            crate::http::proxy::ProxySnapshot::capture(
                request.headers(),
                request.uri(),
                request.extensions(),
                request
                    .extensions()
                    .get::<crate::foundation::ApplicationState>()
                    .map(|state| &state.config().server),
            )
        })
        .context()?
        .secure()
}
