//! Events emitted by the application HTTP kernel.

use std::{collections::HashMap, net::IpAddr, time::Duration};

use crate::{
    container::ParameterValue,
    contracts::{events::Event, session::Session as _},
    http::{Request, RequestParts, Response, ResponseParts},
};

/// The completed request's metadata and server context, before transport sends its body.
///
/// Streaming bodies are never polled or buffered to create this event. Metadata and
/// the attribute map are detached snapshots; shared values retain their normal ownership.
pub struct RequestHandled {
    pub request: RequestParts,
    pub response: ResponseParts,
    /// Shared bytes only for responses whose content is already buffered.
    pub response_content: Option<axum::body::Bytes>,
    /// 原生重定向的目标地址。
    pub redirect_target: Option<String>,
    pub attributes: HashMap<String, ParameterValue>,
    pub route_name: Option<String>,
    /// 使用框架信任代理规则解析后的客户端地址。
    pub ip_address: Option<IpAddr>,
    /// 已加载的会话快照。观察者应自行脱敏后再持久化。
    pub session: Option<serde_json::Value>,
    /// Already-decoded input; an unread body is not consumed for observation.
    pub payload: Option<serde_json::Value>,
    /// Unix milliseconds captured when the kernel begins handling the request.
    pub request_started_at: i64,
    /// Elapsed time measured by a monotonic clock.
    pub duration: Duration,
}

impl RequestHandled {
    pub const NAME: &'static str = r"OpenRust\Foundation\Http\Events\RequestHandled";

    /// 已渲染根模板的实际来源及显式绑定。可序列化对象使用其只读投影。
    pub fn rendered_view(
        &self,
    ) -> Option<(&std::path::Path, serde_json::Map<String, serde_json::Value>)> {
        let rendered = self
            .response
            .extensions
            .get::<std::sync::Arc<crate::view::RenderedView>>()?;
        (!rendered.source_path.as_os_str().is_empty())
            .then(|| (rendered.source_path.as_path(), rendered.diagnostic_data()))
    }

    pub(super) fn new(
        request: &Request,
        response: &Response,
        request_started_at: i64,
        duration: Duration,
    ) -> Self {
        let mut parts = request.parts().clone();
        parts.uri = request.uri().clone();
        Self {
            request: parts,
            response: response.parts().clone(),
            response_content: response.get_content().cloned(),
            redirect_target: response.redirect_target().map(str::to_owned),
            attributes: request.attributes().snapshot(),
            route_name: request.route_name.clone(),
            ip_address: request.ip(),
            session: request
                .session()
                .ok()
                .map(|session| serde_json::to_value(session.all()).expect("会话数据可序列化")),
            payload: request.input_snapshot(),
            request_started_at,
            duration,
        }
    }
}

impl Event for RequestHandled {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// Metadata captured after entering a request scope and before global middleware.
/// The body remains owned by the request and is never polled for this event.
pub struct RequestReceived {
    pub request: RequestParts,
}
impl RequestReceived {
    pub const NAME: &'static str = r"OpenRust\Foundation\Http\Events\RequestReceived";
    pub(super) fn new(request: &Request) -> Self {
        let mut parts = request.parts().clone();
        parts.uri = request.uri().clone();
        Self { request: parts }
    }
}
impl Event for RequestReceived {
    fn name(&self) -> &str {
        Self::NAME
    }
}
