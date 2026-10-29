//! Carbon Unit 显式单位；不混用 human 表达式别名。

/// `interval_with_unit` 可使用的显式单位，也接受上游规定的名称字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntervalUnit {
    /// 千年。
    Millennium,
    /// 世纪。
    Century,
    /// 十年。
    Decade,
    /// 年。
    Year,
    /// 季度。
    Quarter,
    /// 月。
    Month,
    /// 周。
    Week,
    /// 日。
    Day,
    /// 小时。
    Hour,
    /// 分钟。
    Minute,
    /// 秒。
    Second,
    /// 毫秒。
    Millisecond,
    /// 微秒。
    Microsecond,
}
impl AsRef<str> for IntervalUnit {
    fn as_ref(&self) -> &str {
        match self {
            Self::Millennium => "millennium",
            Self::Century => "century",
            Self::Decade => "decade",
            Self::Year => "year",
            Self::Quarter => "quarter",
            Self::Month => "month",
            Self::Week => "week",
            Self::Day => "day",
            Self::Hour => "hour",
            Self::Minute => "minute",
            Self::Second => "second",
            Self::Millisecond => "millisecond",
            Self::Microsecond => "microsecond",
        }
    }
}
