//! HTTP 与 gRPC 共用的进程就绪状态及请求取消信号。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::Cancellation;
#[cfg(feature = "grpc")]
use super::{Error, Result};

#[derive(Default)]
struct ListenerState {
    expected: usize,
    active: usize,
}

/// 进程服务已绑定 listener 且尚未进入停机阶段时为就绪。
#[derive(Clone)]
pub struct Readiness {
    ready: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Cancellation>>,
    listeners: Arc<Mutex<ListenerState>>,
}

impl Default for Readiness {
    fn default() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(Mutex::new(Cancellation::new())),
            listeners: Arc::new(Mutex::new(ListenerState {
                expected: 1,
                active: 0,
            })),
        }
    }
}

impl Readiness {
    /// 返回当前进程是否可接收新请求。
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// 为联合服务设置必须同时运行的 listener 数量。
    #[cfg(feature = "grpc")]
    pub(crate) fn expect_listeners(&self, count: usize) -> Result<()> {
        if !(1..=16).contains(&count) {
            return Err(Error::custom(
                "readiness_listener_count_invalid",
                "就绪状态要求的 listener 数量必须在 1..=16 之间",
            ));
        }
        let mut listeners = self
            .listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if listeners.active != 0 {
            return Err(Error::custom(
                "readiness_already_listening",
                "服务正在运行时不能修改就绪 listener 数量",
            ));
        }
        listeners.expected = count;
        self.ready.store(false, Ordering::Release);
        Ok(())
    }

    /// 开始接收请求并为本轮服务生命周期重置取消信号。
    pub(crate) fn begin_listening(&self) -> ReadinessListenerGuard {
        let mut listeners = self
            .listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if listeners.active == 0 {
            *self
                .cancellation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Cancellation::new();
        }
        listeners.active += 1;
        self.ready
            .store(listeners.active >= listeners.expected, Ordering::Release);
        ReadinessListenerGuard {
            readiness: self.clone(),
        }
    }

    /// 停止将本进程报告为可接收新流量。
    pub(crate) fn stop_accepting(&self) {
        self.ready.store(false, Ordering::Release);
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

/// listener 运行期间持有的就绪租约；退出或被 abort 时自动撤销。
pub(crate) struct ReadinessListenerGuard {
    readiness: Readiness,
}

impl Drop for ReadinessListenerGuard {
    fn drop(&mut self) {
        let mut listeners = self
            .readiness
            .listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        listeners.active = listeners.active.saturating_sub(1);
        self.readiness
            .ready
            .store(listeners.active >= listeners.expected, Ordering::Release);
    }
}

#[cfg(test)]
#[cfg(feature = "grpc")]
mod tests {
    use super::Readiness;

    #[test]
    fn readiness_waits_for_each_expected_listener_and_clears_when_one_stops()
    -> crate::core::Result<()> {
        let readiness = Readiness::default();
        readiness.expect_listeners(2)?;
        let first = readiness.begin_listening();
        assert!(!readiness.is_ready());
        let second = readiness.begin_listening();
        assert!(readiness.is_ready());
        drop(first);
        assert!(!readiness.is_ready());
        drop(second);
        assert!(!readiness.is_ready());
        Ok(())
    }
}
