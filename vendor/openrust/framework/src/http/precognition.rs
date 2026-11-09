//! 单请求 Precognition 确认状态与可选 middleware。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    extract::Request,
    http::{Extensions, HeaderMap, HeaderValue, header::VARY},
    middleware::Next,
    response::Response,
};

/// 请求私有的可信确认位；克隆的提取器与外层 Session 共用同一状态。
#[derive(Clone, Default)]
pub(crate) struct PrecognitionState(Arc<AtomicBool>);

impl PrecognitionState {
    pub(crate) fn is_confirmed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub(crate) fn ensure_state(extensions: &mut Extensions) -> PrecognitionState {
    if let Some(state) = extensions.get::<PrecognitionState>() {
        return state.clone();
    }
    let state = PrecognitionState::default();
    extensions.insert(state.clone());
    state
}

pub(crate) fn confirmed(extensions: &Extensions) -> bool {
    extensions
        .get::<PrecognitionState>()
        .is_some_and(PrecognitionState::is_confirmed)
}

/// `None` 表示未给出 Validate-Only；`Some("")` 表示存在但值为空。
pub(crate) fn validate_only_header(headers: &HeaderMap) -> Option<&HeaderValue> {
    headers.get("precognition-validate-only")
}

fn attempting(headers: &HeaderMap) -> bool {
    headers
        .get("precognition")
        .is_some_and(|value| value.as_bytes() == b"true")
}

/// 可选预检 middleware；只确认精确的 `Precognition: true`。
///
/// 普通请求仍追加 `Vary: Precognition`。已确认请求在下游返回后追加
/// `Precognition: true`；成功头由受管理 dispatcher 或验证阶段产生，
/// 不从响应状态推测。须在应用路由中显式装配此 middleware。
pub async fn handle_precognitive_requests(mut request: Request, next: Next) -> Response {
    let precognitive = attempting(request.headers());
    let state = ensure_state(request.extensions_mut());
    if precognitive {
        state.0.store(true, Ordering::Release);
    }
    let mut response = next.run(request).await;
    if precognitive {
        response
            .headers_mut()
            .insert("precognition", HeaderValue::from_static("true"));
    }
    append_vary(response.headers_mut());
    response
}

fn append_vary(headers: &mut HeaderMap) {
    let values = headers.get_all(VARY);
    let existing: Option<Vec<_>> = values.iter().map(|value| value.to_str().ok()).collect();
    if let Some(existing) = existing {
        let joined = existing.join(", ");
        let joined = if joined.is_empty() {
            "Precognition".to_owned()
        } else {
            format!("{joined}, Precognition")
        };
        if let Ok(value) = HeaderValue::from_str(&joined) {
            headers.insert(VARY, value);
            return;
        }
    }
    headers.append(VARY, HeaderValue::from_static("Precognition"));
}
