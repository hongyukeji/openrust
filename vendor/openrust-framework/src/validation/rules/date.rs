use super::constraints::{Constraints, joined_parameters as parameters, rule_builder};

/// 显式区分日期字符串与需要按当前格式转换的日期对象。
pub enum DateParameter {
    Text(String),
    Date(crate::support::Date),
}
impl From<&str> for DateParameter {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl From<String> for DateParameter {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<crate::support::Date> for DateParameter {
    fn from(value: crate::support::Date) -> Self {
        Self::Date(value)
    }
}
#[derive(Clone)]
pub struct Date {
    constraints: Constraints,
    format: Option<String>,
}
impl Default for Date {
    fn default() -> Self {
        Self::new()
    }
}
impl Date {
    pub fn new() -> Self {
        Self {
            constraints: Constraints::new("date"),
            format: None,
        }
    }
    pub fn format(mut self, format: &str) -> Self {
        self.constraints.0[0] = format!("date_format:{}", parameters([format]));
        self.format = Some(format.to_owned());
        self
    }
    fn parameter(&self, date: DateParameter) -> Result<String, crate::support::DateError> {
        match date {
            DateParameter::Text(text) => Ok(text),
            DateParameter::Date(date) => date.format(self.format.as_deref().unwrap_or("Y-m-d")),
        }
    }

    pub fn before(mut self, date: impl Into<DateParameter>) -> Self {
        match self.parameter(date.into()) {
            Ok(date) => self
                .constraints
                .push(format!("before:{}", parameters([date]))),
            Err(error) => self.constraints.fail(error),
        };
        self
    }
    pub fn after(mut self, date: impl Into<DateParameter>) -> Self {
        match self.parameter(date.into()) {
            Ok(date) => self
                .constraints
                .push(format!("after:{}", parameters([date]))),
            Err(error) => self.constraints.fail(error),
        };
        self
    }
    pub fn before_or_equal(mut self, date: impl Into<DateParameter>) -> Self {
        match self.parameter(date.into()) {
            Ok(date) => self
                .constraints
                .push(format!("before_or_equal:{}", parameters([date]))),
            Err(error) => self.constraints.fail(error),
        };
        self
    }
    pub fn after_or_equal(mut self, date: impl Into<DateParameter>) -> Self {
        match self.parameter(date.into()) {
            Ok(date) => self
                .constraints
                .push(format!("after_or_equal:{}", parameters([date]))),
            Err(error) => self.constraints.fail(error),
        };
        self
    }
    pub fn equals(mut self, date: impl Into<DateParameter>) -> Self {
        match self.parameter(date.into()) {
            Ok(date) => self
                .constraints
                .push(format!("date_equals:{}", parameters([date]))),
            Err(error) => self.constraints.fail(error),
        };
        self
    }
    pub fn before_today(self) -> Self {
        self.before("today")
    }
    pub fn after_today(self) -> Self {
        self.after("today")
    }
    pub fn today_or_before(self) -> Self {
        self.before_or_equal("today")
    }
    pub fn today_or_after(self) -> Self {
        self.after_or_equal("today")
    }
    pub fn past(self) -> Self {
        self.before("now")
    }
    pub fn future(self) -> Self {
        self.after("now")
    }
    pub fn now_or_past(self) -> Self {
        self.before_or_equal("now")
    }
    pub fn now_or_future(self) -> Self {
        self.after_or_equal("now")
    }
    pub fn between(self, from: impl Into<DateParameter>, to: impl Into<DateParameter>) -> Self {
        self.after(from).before(to)
    }
    pub fn between_or_equal(
        self,
        from: impl Into<DateParameter>,
        to: impl Into<DateParameter>,
    ) -> Self {
        self.after_or_equal(from).before_or_equal(to)
    }
}
rule_builder!(Date);
