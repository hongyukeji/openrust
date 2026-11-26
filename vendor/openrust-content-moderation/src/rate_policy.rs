use serde_json::{Map, Value};

use crate::{ModerationError, ModerationResult};

#[derive(Clone, Copy)]
pub(crate) enum RateOperation {
    Submit,
    Query,
}

impl RateOperation {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Query => "query",
        }
    }
}

pub(crate) struct AccountRates {
    submit: i64,
    query: i64,
    pub(crate) window_seconds: i64,
}

impl AccountRates {
    pub(crate) fn configured(config: Option<&Value>) -> ModerationResult<Self> {
        let empty = Map::new();
        let invalid = || ModerationError::InvalidConfiguration("account_rate_limits");
        let values = match config {
            None => &empty,
            Some(config) => config.as_object().ok_or_else(invalid)?,
        };
        if values
            .keys()
            .any(|field| !matches!(field.as_str(), "submit" | "query" | "window_seconds"))
        {
            return Err(invalid());
        }
        let bounded = |field, default, maximum| match values.get(field) {
            None => Ok(default),
            Some(value) => value
                .as_i64()
                .filter(|value| (1..=maximum).contains(value))
                .ok_or_else(invalid),
        };
        Ok(Self {
            submit: bounded("submit", 60, 10_000)?,
            query: bounded("query", 300, 10_000)?,
            window_seconds: bounded("window_seconds", 60, 300)?,
        })
    }

    pub(crate) fn maximum(&self, operation: RateOperation) -> i64 {
        match operation {
            RateOperation::Submit => self.submit,
            RateOperation::Query => self.query,
        }
    }
}
