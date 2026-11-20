use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The outcome of a provider submission, before carrier confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmsSubmissionStatus {
    Logged,
    Accepted,
    Delivered,
    Undelivered,
}

/// Information available to callers and notification events without the raw provider response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SmsSubmission {
    pub provider: String,
    pub status: SmsSubmissionStatus,
    pub message_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    delivery_statuses: BTreeMap<String, SmsDeliveryStatus>,
}

/// A carrier outcome for one message ID from a verified callback or a provider lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmsDeliveryStatus {
    Delivered,
    Undelivered,
}

impl SmsSubmission {
    /// Applies a verified terminal receipt for a message belonging to this submission.
    /// Returns false for another provider, an unknown ID, or a conflicting terminal outcome.
    pub fn record_delivery(
        &mut self,
        provider: &str,
        message_id: &str,
        status: SmsDeliveryStatus,
    ) -> bool {
        if self.provider != provider
            || self.status == SmsSubmissionStatus::Logged
            || !self.message_ids.iter().any(|id| id == message_id)
        {
            return false;
        }
        if let Some(previous) = self.delivery_statuses.get(message_id) {
            return *previous == status;
        }
        if self.status == SmsSubmissionStatus::Delivered && status != SmsDeliveryStatus::Delivered {
            return false;
        }
        self.delivery_statuses.insert(message_id.into(), status);
        if status == SmsDeliveryStatus::Undelivered {
            self.status = SmsSubmissionStatus::Undelivered;
        } else if self
            .message_ids
            .iter()
            .all(|id| self.delivery_statuses.get(id) == Some(&SmsDeliveryStatus::Delivered))
        {
            self.status = SmsSubmissionStatus::Delivered;
        }
        true
    }

    pub(in crate::notifications) fn from_response(provider: &str, response: &Value) -> Self {
        let ids = match provider {
            "aliyun" => response
                .get("BizId")
                .or_else(|| response.get("MessageId"))
                .into_iter()
                .collect(),
            "tencent" => response
                .pointer("/Response/SendStatusSet/0/SerialNo")
                .into_iter()
                .collect(),
            "twilio" | "yunpian" => response.get("sid").into_iter().collect(),
            "vonage" => response["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|message| message.get("message-id"))
                .collect(),
            _ => Vec::new(),
        };
        Self {
            provider: provider.into(),
            status: if provider == "log" {
                SmsSubmissionStatus::Logged
            } else if provider == "twilio" && response["status"] == "delivered" {
                SmsSubmissionStatus::Delivered
            } else {
                SmsSubmissionStatus::Accepted
            },
            message_ids: ids
                .into_iter()
                .filter_map(|id| {
                    id.as_str()
                        .map(str::to_owned)
                        .or_else(|| id.as_u64().map(|id| id.to_string()))
                })
                .collect(),
            delivery_statuses: BTreeMap::new(),
        }
    }
}
