//! HTTP 与 gRPC 共用的进程就绪状态及请求取消信号。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::Cancellation;

/// 进程服务已绑定 listener 且尚未进入停机阶段时为就绪。
#[derive(Clone)]
pub struct Readiness {
    ready: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Cancellation>>,
}

impl Default for Readiness {
    fn default() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(Mutex::new(Cancellation::new())),
        }
    }
}

impl Readiness {
    /// 返回当前进程是否可接收新请求。
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// 更新进程就绪状态。
    pub(crate) fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::Release);
    }

    /// 开始接收请求并为本轮服务生命周期重置取消信号。
    pub(crate) fn begin_listening(&self) {
        *self
            .cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Cancellation::new();
        self.set_ready(true);
    }

    /// 停机超时后取消仍在运行的请求。
    pub(crate) fn cancel_requests(&self) {
        self.cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancel();
    }

    /// 创建一个由服务生命周期取消信号驱动的请求令牌。
    pub(crate) fn request_cancellation(&self) -> Cancellation {
        Cancellation::child(
            &self
                .cancellation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}
