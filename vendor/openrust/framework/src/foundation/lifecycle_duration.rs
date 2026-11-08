//! 内核入口计时与超时长处理器。

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::Instant;

use super::ApplicationState;
use super::lifecycle::disposal;
use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};
use crate::{BoxFuture, Result};

/// 内核生命周期处理器的触发阈值，比较时使用严格大于。
#[derive(Clone, Copy, Debug)]
pub enum LifecycleThreshold {
    /// 固定持续时间。
    Duration(Duration),
    /// 支持负值和小数的毫秒数。
    Milliseconds(f64),
    /// 登记时按 Unix 整秒差折算成持续时间，已到期时取零。
    At(DateTime<Utc>),
}

impl From<Duration> for LifecycleThreshold {
    fn from(value: Duration) -> Self {
        Self::Duration(value)
    }
}

impl LifecycleThreshold {
    fn normalized(self, now: DateTime<Utc>) -> Threshold {
        match self {
            Self::Duration(duration) => Threshold::Duration(duration),
            Self::Milliseconds(ms) => Threshold::Milliseconds(ms),
            Self::At(at) => Threshold::Duration(Duration::from_secs(
                (at.timestamp() - now.timestamp()).max(0).unsigned_abs(),
            )),
        }
    }
}

#[derive(Clone, Copy)]
enum Threshold {
    Duration(Duration),
    Milliseconds(f64),
}

impl Threshold {
    fn exceeded_by(self, elapsed: Duration) -> bool {
        match self {
            Self::Duration(threshold) => elapsed > threshold,
            Self::Milliseconds(threshold) => elapsed.as_secs_f64() * 1000.0 > threshold,
        }
    }
}

/// 请求的终止阶段可观察信息，正文已由服务器处理。
pub struct RequestLifecycle<'a> {
    /// 以当前应用时区显示的入口开始时间。
    pub started_at: &'a InputDate,
    /// 从入口到应用终止回调完成的单调耗时。
    pub elapsed: Duration,
    /// 请求头和请求共享的属性与路由。
    pub request: &'a http::Request<()>,
    /// 已交付响应的状态和头。
    pub response: &'a http::Response<()>,
}

/// 命令的终止阶段可观察信息。
pub struct CommandLifecycle<'a> {
    /// 以当前应用时区显示的入口开始时间。
    pub started_at: &'a InputDate,
    /// 从入口到应用终止回调完成的单调耗时。
    pub elapsed: Duration,
    /// 入口请求执行的命令名。
    pub name: &'a str,
    /// 入口传入的参数，保留子命令与帮助选项。
    pub arguments: &'a [String],
    /// 命令运行状态。成功为 0，失败为 1。
    pub status: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct EntryStarted {
    pub(crate) at: DateTime<Utc>,
    timer: Instant,
}

impl EntryStarted {
    pub(crate) fn now() -> Self {
        Self {
            at: Utc::now(),
            timer: Instant::now(),
        }
    }

    pub(crate) fn elapsed(self) -> Duration {
        self.timer.elapsed()
    }

    pub(crate) fn local(self, state: &ApplicationState) -> Result<InputDate> {
        let value = state.config_repository().get("app.timezone")?;
        let timezone = match value {
            None | Some(serde_json::Value::Null) => ResolvedTimezone::utc(),
            Some(serde_json::Value::String(name)) => {
                ResolvedTimezone::parse(&name).map_err(|_| {
                    crate::Error::custom("invalid_config", "app.timezone: 时区名称无效")
                })?
            }
            Some(_) => {
                return Err(crate::Error::custom(
                    "invalid_config",
                    "app.timezone 必须为字符串",
                ));
            }
        };
        let instant = CalendarInstant::new(self.at.timestamp(), self.at.timestamp_subsec_micros())?;
        Ok(InputDate::new(instant, timezone))
    }
}

pub(crate) type RequestDurationHook = dyn for<'a> Fn(ApplicationState, RequestLifecycle<'a>) -> BoxFuture<'a, Result<()>>
    + Send
    + Sync;
pub(crate) type CommandDurationHook = dyn for<'a> Fn(ApplicationState, CommandLifecycle<'a>) -> BoxFuture<'a, Result<()>>
    + Send
    + Sync;

type HandlerList<H> = Vec<(Threshold, Arc<H>)>;

pub(crate) struct DurationHandlers<H: ?Sized>(Mutex<HandlerList<H>>);

impl<H: ?Sized> Default for DurationHandlers<H> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

impl<H: ?Sized> DurationHandlers<H> {
    pub(crate) fn push(&self, threshold: LifecycleThreshold, handler: Arc<H>) {
        let threshold = threshold.normalized(Utc::now());
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((threshold, handler));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    pub(crate) fn matching(&self, elapsed: Duration) -> Vec<Arc<H>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(threshold, _)| threshold.exceeded_by(elapsed))
            .map(|(_, handler)| Arc::clone(handler))
            .collect()
    }

    pub(crate) fn take(&self) -> impl Iterator<Item = Arc<H>> + use<H> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .map(|(_, handler)| handler)
    }
}

impl<H: ?Sized> Drop for DurationHandlers<H> {
    fn drop(&mut self) {
        for handler in self.take() {
            if disposal::dispose(handler, None).seen {
                tracing::error!("内核时长处理器析构失败");
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct KernelDurationHandlers {
    pub(crate) requests: DurationHandlers<RequestDurationHook>,
    pub(crate) commands: DurationHandlers<CommandDurationHook>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_threshold_is_strict_and_absolute_values_use_registration_seconds() -> Result<()> {
        let now = DateTime::from_timestamp_micros(100_900_000)
            .ok_or_else(|| crate::Error::custom("test_timestamp", "测试时刻无效"))?;
        let target = DateTime::from_timestamp_micros(101_010_000)
            .ok_or_else(|| crate::Error::custom("test_timestamp", "测试时刻无效"))?;
        let threshold = LifecycleThreshold::At(target).normalized(now);
        assert!(!threshold.exceeded_by(Duration::from_secs(1)));
        assert!(
            LifecycleThreshold::At(target)
                .normalized(now)
                .exceeded_by(Duration::from_secs(1) + Duration::from_nanos(1))
        );
        assert!(
            !LifecycleThreshold::At(now)
                .normalized(target)
                .exceeded_by(Duration::ZERO)
        );
        assert!(
            LifecycleThreshold::Milliseconds(-0.5)
                .normalized(now)
                .exceeded_by(Duration::ZERO)
        );
        assert!(
            !LifecycleThreshold::Milliseconds(0.5)
                .normalized(now)
                .exceeded_by(Duration::from_micros(500))
        );
        assert!(
            LifecycleThreshold::Milliseconds(0.5)
                .normalized(now)
                .exceeded_by(Duration::from_micros(501))
        );
        Ok(())
    }
}
