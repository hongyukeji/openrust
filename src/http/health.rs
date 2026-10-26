//! 存活探针端点。

use crate::core::{ApiResponse, Cancellation};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::http::routes::Routes;

/// 当前 HTTP listener 的就绪状态。
#[derive(Clone)]
pub(crate) struct ServerReadiness {
    ready: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Cancellation>>,
}

impl Default for ServerReadiness {
    fn default() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(Mutex::new(Cancellation::new())),
        }
    }
}

impl ServerReadiness {
    pub(crate) fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::Release);
    }

    pub(crate) fn begin_listening(&self) {
        *self
            .cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Cancellation::new();
        self.set_ready(true);
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub(crate) fn cancel_requests(&self) {
        self.cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancel();
    }

    pub(crate) fn request_cancellation(&self) -> Cancellation {
        Cancellation::child(
            &self
                .cancellation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// `GET /health` 的响应正文。
#[derive(Debug, Serialize)]
pub struct Health {
    /// 进程能够响应时始终为 `"ok"`。
    pub status: &'static str,
}

/// 存活探针：只要进程能够处理请求就返回成功。依赖项检查应由能力 crate 提供的就绪探针端点负责。
pub async fn live() -> ApiResponse<Health> {
    ApiResponse::success(Health { status: "ok" })
}

/// 就绪探针：listener 已开始接收流量且未进入停机阶段时返回 200，否则返回 503。
pub async fn ready(State(state): State<crate::core::AppState>) -> Response {
    let is_ready = state
        .try_get::<ServerReadiness>()
        .is_ok_and(|readiness| readiness.is_ready());
    let mut response = ApiResponse::success(Health {
        status: if is_ready { "ready" } else { "not_ready" },
    })
    .into_response();
    if !is_ready {
        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    }
    response
}

/// 注册 `GET /health` 和 `GET /ready`。
pub fn routes(routes: Routes) -> Routes {
    routes.get("/health", live).get("/ready", ready)
}
