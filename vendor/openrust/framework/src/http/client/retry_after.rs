use std::time::UNIX_EPOCH;

use http::HeaderValue;

/// HTTP 服务建议等待的秒数或最早重试的 Unix 时间戳。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryAfter {
    Seconds(u64),
    Timestamp(i64),
}

impl RetryAfter {
    pub fn from_header(header: &HeaderValue) -> Option<Self> {
        let value = header.to_str().ok()?.trim();
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            return value
                .parse::<i64>()
                .ok()
                .map(|seconds| Self::Seconds(seconds as u64));
        }
        let date = httpdate::parse_http_date(value)
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?;
        Some(Self::Timestamp(i64::try_from(date.as_secs()).ok()?))
    }

    /// 使用调用者提供的时钟。秒数加法溢出时取最大可表示时间。
    pub fn earliest_at(self, now: i64) -> i64 {
        match self {
            Self::Seconds(seconds) => {
                now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX))
            }
            Self::Timestamp(timestamp) => timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_delay_and_http_dates_without_wall_clock() {
        let parse = |value| RetryAfter::from_header(&HeaderValue::from_static(value));
        assert_eq!(parse("00030"), Some(RetryAfter::Seconds(30)));
        assert_eq!(
            parse("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(RetryAfter::Timestamp(784_111_777))
        );
        assert_eq!(
            parse("Sunday, 06-Nov-94 08:49:37 GMT"),
            parse("Sun Nov  6 08:49:37 1994")
        );
        assert_eq!(parse("-1"), None);
        assert_eq!(parse("1, 2"), None);
        assert_eq!(parse("9223372036854775808"), None);
        assert_eq!(RetryAfter::Seconds(30).earliest_at(100), 130);
        assert_eq!(RetryAfter::Timestamp(400).earliest_at(100), 400);
        assert_eq!(RetryAfter::Seconds(30).earliest_at(i64::MAX), i64::MAX);
    }
}
