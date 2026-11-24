//! 固定日期单位与时间精度查询；单复数规范化与可修改单位检查分别遵循原规则。
use super::{DateError, DateFacade, DateFactory};
impl DateFactory {
    pub fn get_days(&self) -> Vec<String> {
        [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    pub fn singular_unit(&self, unit: &str) -> String {
        let unit = crate::support::Str::lower(unit);
        match unit.trim_end_matches('s') {
            "centurie" => "century".into(),
            "millennia" => "millennium".into(),
            unit => unit.to_owned(),
        }
    }
    pub fn plural_unit(&self, unit: &str) -> String {
        let unit = unit.to_ascii_lowercase();
        match unit.trim_end_matches('s') {
            "century" => "centuries".into(),
            "millennium" | "millennia" => "millennia".into(),
            unit => format!("{unit}s"),
        }
    }
    pub fn is_modifiable_unit(&self, unit: &str) -> bool {
        matches!(
            unit,
            "year"
                | "month"
                | "day"
                | "hour"
                | "minute"
                | "second"
                | "milli"
                | "millisecond"
                | "micro"
                | "microsecond"
                | "millennium"
                | "century"
                | "decade"
                | "quarter"
                | "week"
                | "weekday"
        )
    }
    pub fn get_time_format_by_precision(&self, unit: &str) -> Result<String, DateError> {
        Ok(match self.singular_unit(unit).as_str() {
            "minute" => "H:i",
            "second" => "H:i:s",
            "m" | "millisecond" => "H:i:s.v",
            "µ" | "microsecond" => "H:i:s.u",
            _ => return Err(DateError::InvalidPrecision),
        }
        .into())
    }
}
impl DateFacade {
    pub fn get_days() -> Result<Vec<String>, DateError> {
        Ok(Self::factory()?.get_days())
    }
    pub fn singular_unit(unit: &str) -> Result<String, DateError> {
        Ok(Self::factory()?.singular_unit(unit))
    }
    pub fn plural_unit(unit: &str) -> Result<String, DateError> {
        Ok(Self::factory()?.plural_unit(unit))
    }
    pub fn is_modifiable_unit(unit: &str) -> Result<bool, DateError> {
        Ok(Self::factory()?.is_modifiable_unit(unit))
    }
    pub fn get_time_format_by_precision(unit: &str) -> Result<String, DateError> {
        Self::factory()?.get_time_format_by_precision(unit)
    }
}
