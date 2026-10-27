//! HTTP 与 gRPC 共用的进程就绪状态及请求取消信号。

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::{Cancellation, Error, Result};

type ProbeFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;
type Probe = Arc<dyn Fn() -> ProbeFuture + Send + Sync + 'static>;

const MAX_READINESS_PROBES: usize = 64;
const MAX_CONCURRENT_READINESS_PROBES: usize = 16;
const READINESS_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

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
    probes: Arc<Mutex<std::collections::BTreeMap<String, Probe>>>,
    probe_gate: Arc<tokio::sync::Semaphore>,
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
            probes: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
            probe_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

impl Readiness {
    /// 返回当前进程是否可接收新请求。
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// 注册一个用于 HTTP 就绪检查的异步依赖探针。
    ///
    /// 探针名称须为 1–96 个小写 ASCII 字母、数字、点、短横线或下划线，且在进程内唯一。
    /// 每个进程最多注册 64 个探针。探针应执行有界、只读的健康检查；失败详情不会返回给请求方。
    ///
    /// # Errors
    ///
    /// 名称无效、重复或超过探针数量上限时返回错误。
    pub fn register_probe<F, Fut>(&self, name: impl Into<String>, probe: F) -> Result<()>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let name = name.into();
        if name.is_empty()
            || name.len() > 96
            || !name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(Error::custom(
                "readiness_probe_name_invalid",
                "就绪探针名称必须是 1–96 个小写 ASCII 字母、数字、点、短横线或下划线",
            ));
        }
        let mut probes = self
            .probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if probes.contains_key(&name) {
            return Err(Error::custom(
                "readiness_probe_duplicate",
                "就绪探针名称已注册",
            ));
        }
        if probes.len() >= MAX_READINESS_PROBES {
            return Err(Error::custom(
                "readiness_probe_capacity_exhausted",
                "就绪探针数量已达到上限",
            ));
        }
        let probe: Probe = Arc::new(move || Box::pin(probe()));
        probes.insert(name, probe);
        Ok(())
    }

    /// 并发检查已注册依赖；超时、失败或另一轮检查正在运行时返回 `false`。
    pub async fn dependencies_ready(&self) -> bool {
        let probes: Vec<_> = self
            .probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, probe)| (name.clone(), Arc::clone(probe)))
            .collect();
        if probes.is_empty() {
            return true;
        }
        let Ok(_permit) = self.probe_gate.clone().try_acquire_owned() else {
            return false;
        };
        let checks = async move {
            let mut probes = probes.into_iter();
            let mut healthy = true;
            loop {
                let batch: Vec<_> = probes
                    .by_ref()
                    .take(MAX_CONCURRENT_READINESS_PROBES)
                    .collect();
                if batch.is_empty() {
                    break;
                }
                let results = futures_util::future::join_all(
                    batch
                        .into_iter()
                        .map(|(name, probe)| async move { (name, probe().await) }),
                )
                .await;
                for (name, result) in results {
                    if let Err(error) = result {
                        tracing::warn!(
                            dependency = %name,
                            error_code = error.code(),
                            "依赖就绪探针失败"
                        );
                        healthy = false;
                    }
                }
            }
            healthy
        };
        if let Ok(healthy) = tokio::time::timeout(READINESS_PROBE_TIMEOUT, checks).await {
            healthy
        } else {
            tracing::warn!("依赖就绪探针超过 2 秒总时限");
            false
        }
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
mod tests {
    use super::{MAX_CONCURRENT_READINESS_PROBES, Readiness};
    use crate::core::Error;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::Duration;

    #[tokio::test]
    async fn dependency_probe_failure_recovers_without_exposing_error() -> crate::core::Result<()> {
        let readiness = Readiness::default();
        let healthy = Arc::new(AtomicBool::new(false));
        let probe_health = Arc::clone(&healthy);
        readiness.register_probe("database.primary", move || {
            let healthy = Arc::clone(&probe_health);
            async move {
                if healthy.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err(Error::custom("database_secret", "credential detail"))
                }
            }
        })?;

        assert!(!readiness.dependencies_ready().await);
        healthy.store(true, Ordering::Release);
        assert!(readiness.dependencies_ready().await);
        Ok(())
    }

    #[tokio::test]
    async fn dependency_probes_run_with_bounded_concurrency() -> crate::core::Result<()> {
        let readiness = Readiness::default();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        for index in 0..32 {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            readiness.register_probe(format!("probe.{index}"), move || {
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                async move {
                    let active_count = active.fetch_add(1, Ordering::AcqRel) + 1;
                    maximum.fetch_max(active_count, Ordering::AcqRel);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    active.fetch_sub(1, Ordering::AcqRel);
                    Ok(())
                }
            })?;
        }

        assert!(readiness.dependencies_ready().await);
        assert_eq!(
            maximum.load(Ordering::Acquire),
            MAX_CONCURRENT_READINESS_PROBES
        );
        assert_eq!(active.load(Ordering::Acquire), 0);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_readiness_calls_do_not_start_parallel_probe_rounds()
    -> crate::core::Result<()> {
        let readiness = Readiness::default();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let probe_started = Arc::clone(&started);
        let probe_release = Arc::clone(&release);
        readiness.register_probe("database.primary", move || {
            let started = Arc::clone(&probe_started);
            let release = Arc::clone(&probe_release);
            async move {
                started.notify_one();
                let permit = release
                    .acquire()
                    .await
                    .map_err(|error| Error::internal(std::io::Error::other(error)))?;
                permit.forget();
                Ok(())
            }
        })?;

        let first_readiness = readiness.clone();
        let first = tokio::spawn(async move { first_readiness.dependencies_ready().await });
        started.notified().await;
        assert!(!readiness.dependencies_ready().await);
        release.add_permits(1);
        assert!(first.await.map_err(Error::internal)?);
        Ok(())
    }

    #[test]
    fn dependency_probe_names_are_validated_and_unique() -> crate::core::Result<()> {
        let readiness = Readiness::default();
        assert_eq!(
            readiness
                .register_probe("Database.Primary", || async { Ok(()) })
                .err()
                .map(|error| error.code().to_owned()),
            Some("readiness_probe_name_invalid".to_owned())
        );
        readiness.register_probe("database.primary", || async { Ok(()) })?;
        assert_eq!(
            readiness
                .register_probe("database.primary", || async { Ok(()) })
                .err()
                .map(|error| error.code().to_owned()),
            Some("readiness_probe_duplicate".to_owned())
        );
        Ok(())
    }

    #[cfg(feature = "grpc")]
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
