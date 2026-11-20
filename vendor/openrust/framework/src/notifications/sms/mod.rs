//! SMS messages, named provider drivers, and submission results.
//!
//! Direct sends and the notification channel share the same application-scoped manager.

mod facade;
mod failure;
mod message;
pub(super) mod providers;
mod sms_manager;
mod sms_sender;
mod submission;

pub use facade::Sms;
pub use failure::{SmsFailure, SmsFailureKind};
pub(super) use message::international_number;
pub use message::{SmsMessage, SmsTemplate, SmsTemplateParameters};
pub use sms_manager::SmsManager;
pub use sms_sender::SmsSender;
pub use submission::{SmsDeliveryStatus, SmsSubmission, SmsSubmissionStatus};

use serde_json::Value;

use super::{NotificationFuture, NotificationResult, delivery};

/// Submits a message and returns the provider's response.
pub trait SmsDriver: Send + Sync {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>>;
}
