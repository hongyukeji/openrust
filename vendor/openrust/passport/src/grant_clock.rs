//! OAuth 授权期限使用的可信时钟。

use std::time::{SystemTime, UNIX_EPOCH};

/// 为授权事务提供当前 Unix 秒；调用方须在各安全检查边界重新读取。
pub trait GrantClock: Send + Sync {
    /// 获取当前 Unix 秒。
    ///
    /// # Errors
    /// 时钟不可用或时间无法用有符号秒数表示时返回脱敏错误。
    fn now(&self) -> openrust::Result<i64>;
}

/// 使用系统时钟的默认授权时钟。
#[derive(Clone, Copy, Default)]
pub struct SystemGrantClock;

impl GrantClock for SystemGrantClock {
    fn now(&self) -> openrust::Result<i64> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| clock_unavailable())?;
        i64::try_from(elapsed.as_secs()).map_err(|_| clock_unavailable())
    }
}

fn clock_unavailable() -> openrust::Error {
    openrust::Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_clock_unavailable",
        "OAuth 授权时钟不可用",
    )
}

#[cfg(test)]
#[path = "grant_clock_tests.rs"]
mod tests;
