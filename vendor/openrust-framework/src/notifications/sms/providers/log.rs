use serde_json::{Value, json};

use crate::{
    log::{Log, LogContext},
    notifications::{NotificationFuture, NotificationResult, delivery},
};

use super::super::{SmsDriver, SmsMessage, international_number};

pub(super) struct LogDriver;

impl SmsDriver for LogDriver {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let digits = international_number(message.to())?;
            let context = LogContext::from_iter([
                (
                    "recipient_last_four".into(),
                    json!(&digits[digits.len() - 4..]),
                ),
                (
                    "kind".into(),
                    json!(if message.approved_template().is_some() {
                        "template"
                    } else {
                        "text"
                    }),
                ),
            ]);
            Log::info("SMS recorded by log driver.", &context).map_err(delivery)?;
            Ok(json!({"driver": "log", "logged": true}))
        })
    }
}
