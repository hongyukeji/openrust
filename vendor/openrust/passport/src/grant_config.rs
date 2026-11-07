//! 固定 Passport 默认授权期限；日历年遵循 PHP `DateInterval` 的日期进位语义。

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};

/// OAuth 授权期限，以秒或 UTC 日历年表示，不将一年换算为固定天数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantLifetime {
    /// 从起始时刻增加给定秒数。
    Seconds(u64),
    /// 增加日历年，闰日落入非闰年时进位至三月一日。
    CalendarYears(u32),
}

impl GrantLifetime {
    /// 根据起始 Unix 秒计算失效 Unix 秒。
    ///
    /// UTC 是本实现的明确时间基准；例如 2024-02-29 加一年为 2025-03-01，
    /// 对应 PHP 日期进位，不使用将闰日截断为二月二十八日的算法。
    ///
    /// # Errors
    /// 起始时刻不能表示为有效 UTC 日期、年份或结果溢出时返回错误。
    pub fn expires_at(self, now: i64) -> openrust::Result<i64> {
        let start = DateTime::<Utc>::from_timestamp(now, 0).ok_or_else(invalid_lifetime)?;
        match self {
            Self::Seconds(seconds) => {
                let timestamp = i64::try_from(i128::from(now) + i128::from(seconds))
                    .map_err(|_| invalid_lifetime())?;
                DateTime::<Utc>::from_timestamp(timestamp, 0)
                    .map(|value| value.timestamp())
                    .ok_or_else(invalid_lifetime)
            }
            Self::CalendarYears(years) => {
                let years = i32::try_from(years).map_err(|_| invalid_lifetime())?;
                let year = start
                    .year()
                    .checked_add(years)
                    .ok_or_else(invalid_lifetime)?;
                let first =
                    NaiveDate::from_ymd_opt(year, start.month(), 1).ok_or_else(invalid_lifetime)?;
                let date = first
                    .checked_add_signed(Duration::days(i64::from(start.day() - 1)))
                    .ok_or_else(invalid_lifetime)?;
                Ok(date.and_time(start.time()).and_utc().timestamp())
            }
        }
    }
}

/// 授权码与令牌的默认配置，对应固定 Passport 与 League 的默认行为。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantConfig {
    /// 授权码有效期，默认十分钟。
    pub authorization_code_lifetime: GrantLifetime,
    /// 访问令牌有效期，默认一个日历年。
    pub access_token_lifetime: GrantLifetime,
    /// 刷新令牌有效期，默认一个日历年。
    pub refresh_token_lifetime: GrantLifetime,
    /// 刷新时撤销旧刷新令牌，默认启用。
    pub revoke_refresh_tokens: bool,
    /// 请求未指定 scope 时使用的默认 scope 集合，默认空集合。
    pub default_scopes: Vec<String>,
}

impl Default for GrantConfig {
    fn default() -> Self {
        Self {
            authorization_code_lifetime: GrantLifetime::Seconds(600),
            access_token_lifetime: GrantLifetime::CalendarYears(1),
            refresh_token_lifetime: GrantLifetime::CalendarYears(1),
            revoke_refresh_tokens: true,
            default_scopes: Vec::new(),
        }
    }
}

fn invalid_lifetime() -> openrust::Error {
    openrust::Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_grant_lifetime_invalid",
        "OAuth授权期限或UTC时刻超出可表示范围",
    )
}

#[cfg(test)]
#[path = "grant_config_tests.rs"]
mod tests;
