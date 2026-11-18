//! Selectable SMS drivers for direct sends and the SMS notification channel.

pub(super) mod providers;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{container::ServiceKey, contracts::config::Repository, support::facades::Facade};

use super::{NotificationFuture, NotificationResult, channels::VonageDriver, delivery};

/// The fields shared by SMS providers.
#[derive(Deserialize, Serialize)]
pub struct SmsMessage {
    to: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template: Option<SmsTemplate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_name: Option<String>,
}

/// A provider-approved template and the values used to fill it.
#[derive(Clone, Deserialize, Serialize)]
pub struct SmsTemplate {
    pub id: String,
    pub parameters: SmsTemplateParameters,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SmsTemplateParameters {
    Named(BTreeMap<String, String>),
    Positional(Vec<String>),
}

impl SmsTemplate {
    pub fn named(id: impl Into<String>, parameters: BTreeMap<String, String>) -> Self {
        Self {
            id: id.into(),
            parameters: SmsTemplateParameters::Named(parameters),
        }
    }

    pub fn positional(id: impl Into<String>, parameters: Vec<String>) -> Self {
        Self {
            id: id.into(),
            parameters: SmsTemplateParameters::Positional(parameters),
        }
    }
}

impl SmsMessage {
    pub fn new(to: impl Into<String>, content: impl Into<String>) -> NotificationResult<Self> {
        let message = Self {
            to: to.into(),
            content: content.into(),
            template: None,
            template_name: None,
        };
        message.validate()?;
        Ok(message)
    }

    pub fn template(to: impl Into<String>, template: SmsTemplate) -> NotificationResult<Self> {
        let message = Self {
            to: to.into(),
            content: String::new(),
            template: Some(template),
            template_name: None,
        };
        message.validate()?;
        Ok(message)
    }

    /// Builds a message using a business template configured on the selected profile.
    pub fn named_template<K: Into<String>, V: Into<String>>(
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<Self> {
        let name = name.into();
        let mut message = Self::template(
            to,
            SmsTemplate::named(
                &name,
                parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            ),
        )?;
        message.template_name = Some(name);
        Ok(message)
    }

    pub fn to(&self) -> &str {
        &self.to
    }

    /// Returns the text body, which is empty for a template message.
    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn approved_template(&self) -> Option<&SmsTemplate> {
        self.template.as_ref()
    }

    pub(super) fn validate(&self) -> NotificationResult<()> {
        international_number(&self.to)?;
        if let Some(name) = &self.template_name
            && (name.trim().is_empty() || self.template.is_none())
        {
            return Err(delivery("The configured SMS template name is invalid."));
        }
        match &self.template {
            Some(template) if !template.id.trim().is_empty() && self.content.is_empty() => Ok(()),
            Some(_) => Err(delivery(
                "The SMS template requires an ID and cannot include a text body.",
            )),
            None if self.content.trim().is_empty() => {
                Err(delivery("The SMS content cannot be empty."))
            }
            None => Ok(()),
        }
    }
}

pub(super) fn international_number(number: &str) -> NotificationResult<&str> {
    let number = number.trim();
    let digits = number.strip_prefix('+').unwrap_or(number);
    if !(7..=15).contains(&digits.len())
        || digits.starts_with('0')
        || !digits.bytes().all(|digit| digit.is_ascii_digit())
    {
        return Err(delivery(
            "The SMS recipient must be an international phone number.",
        ));
    }
    Ok(digits)
}

/// Submits a message and returns the provider's response.
pub trait SmsDriver: Send + Sync {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>>;
}

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

    pub(super) fn from_response(provider: &str, response: &Value) -> Self {
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

/// Whether a failed submission may be attempted again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmsFailureKind {
    Rejected,
    Retryable,
    SubmissionUnknown,
}

#[derive(Debug)]
pub struct SmsFailure {
    pub kind: SmsFailureKind,
    pub code: Option<String>,
    reason: &'static str,
}

impl std::fmt::Display for SmsFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason)?;
        if let Some(code) = &self.code {
            write!(f, " ({code})")?;
        }
        Ok(())
    }
}

impl std::error::Error for SmsFailure {}

impl SmsFailure {
    pub fn new(kind: SmsFailureKind, code: Option<&str>, reason: &'static str) -> Self {
        Self {
            kind,
            code: code
                .filter(|code| {
                    !code.is_empty()
                        && code.len() <= 80
                        && code.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
                        })
                })
                .map(str::to_owned),
            reason,
        }
    }

    pub fn from_notification(error: &super::NotificationError) -> Option<&Self> {
        match error {
            super::NotificationError::Delivery { source } => source.downcast_ref::<Self>(),
            _ => None,
        }
    }
}

type DriverFactory = dyn Fn(&Value) -> NotificationResult<Arc<dyn SmsDriver>> + Send + Sync;

/// Resolves named profiles from the current application's SMS configuration.
pub struct SmsManager {
    config: Arc<dyn Repository>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
}

impl SmsManager {
    pub(super) fn new(config: Arc<dyn Repository>) -> Self {
        let manager = Self {
            config,
            factories: RwLock::default(),
        };
        manager.extend("vonage", |config| Ok(Arc::new(VonageDriver::new(config)?)));
        providers::register(&manager);
        manager
    }

    pub fn extend(
        &self,
        name: impl Into<String>,
        factory: impl Fn(&Value) -> NotificationResult<Arc<dyn SmsDriver>> + Send + Sync + 'static,
    ) {
        self.factories
            .write()
            .expect("SMS driver registry lock poisoned")
            .insert(name.into(), Arc::new(factory));
    }

    fn profile_name(&self, name: Option<&str>) -> NotificationResult<String> {
        if let Some(name) = name {
            return Ok(name.to_owned());
        }
        match self.config.get("sms.default") {
            None => Ok("log".into()),
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            _ => Err(delivery("The default SMS profile is invalid.")),
        }
    }

    fn configured_driver(&self, name: &str) -> NotificationResult<(Value, Arc<DriverFactory>)> {
        let profiles = self.config.get("sms.drivers");
        let profile = match profiles.as_ref().and_then(|profiles| profiles.get(name)) {
            Some(profile) => profile.clone(),
            None if profiles.is_none() && matches!(name, "log" | "vonage") => {
                serde_json::json!({"driver": name})
            }
            None => return Err(delivery("The SMS profile is not configured.")),
        };
        let driver = profile["driver"]
            .as_str()
            .ok_or_else(|| delivery("The SMS profile requires a driver."))?;
        let factory = self
            .factories
            .read()
            .expect("SMS driver registry lock poisoned")
            .get(driver)
            .cloned()
            .ok_or_else(|| delivery("The configured SMS driver is not registered."))?;
        Ok((profile, factory))
    }

    pub(super) fn select_profile(&self, name: Option<&str>) -> NotificationResult<String> {
        let name = self.profile_name(name)?;
        self.configured_driver(&name)?;
        Ok(name)
    }

    pub fn driver(&self, name: &str) -> NotificationResult<SmsSender> {
        let (profile, factory) = self.configured_driver(name)?;
        let driver = factory(&profile)?;
        Ok(SmsSender { profile, driver })
    }

    pub fn default_driver(&self) -> NotificationResult<SmsSender> {
        self.driver(&self.profile_name(None)?)
    }
}

/// Sends text or templates using one configured SMS profile.
pub struct SmsSender {
    profile: Value,
    driver: Arc<dyn SmsDriver>,
}

impl SmsSender {
    pub async fn send(&self, mut message: SmsMessage) -> NotificationResult<Value> {
        message.validate()?;
        if let Some(name) = message.template_name.take()
            && self.profile["driver"] != "log"
        {
            let market = providers::market_key(message.to(), &self.profile)?;
            let template_setting = self.profile["markets"][market]["templates"]
                .get(&name)
                .or_else(|| self.profile["templates"].get(&name))
                .ok_or_else(|| {
                    delivery("The SMS template is not configured for this profile and market.")
                })?;
            if let Some(text) = template_setting.get("text") {
                let template = message.template.as_ref().expect("validated template");
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery("The configured SMS text requires named input."));
                };
                let text = text
                    .as_str()
                    .ok_or_else(|| delivery("The configured SMS text must be a string."))?;
                let content = render_sms_text(text, parameters)?;
                return self
                    .driver
                    .send(SmsMessage::new(message.to, content)?)
                    .await;
            }
            let id = template_setting
                .as_str()
                .or_else(|| template_setting["id"].as_str())
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| delivery("The configured SMS template requires an ID."))?;
            let template = message.template.as_mut().expect("validated template");
            template.id = id.to_owned();
            if let Some(order) = template_setting.get("parameter_order") {
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery(
                        "The configured SMS template requires named input.",
                    ));
                };
                let order = order
                    .as_array()
                    .ok_or_else(|| delivery("The SMS template parameter order must be a list."))?;
                let ordered = order
                    .iter()
                    .map(|key| {
                        key.as_str()
                            .and_then(|key| parameters.get(key))
                            .cloned()
                            .ok_or_else(|| {
                                delivery("A configured SMS template parameter is missing.")
                            })
                    })
                    .collect::<NotificationResult<Vec<_>>>()?;
                template.parameters = SmsTemplateParameters::Positional(ordered);
            }
        }
        self.driver.send(message).await
    }

    /// Submits an SMS and normalizes the provider acknowledgement.
    pub async fn submit(&self, message: SmsMessage) -> NotificationResult<SmsSubmission> {
        let response = self.send(message).await?;
        let provider = self.profile["driver"]
            .as_str()
            .expect("registered SMS driver");
        Ok(SmsSubmission::from_response(provider, &response))
    }

    pub async fn send_text(
        &self,
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> NotificationResult<SmsSubmission> {
        self.submit(SmsMessage::new(to, content)?).await
    }

    pub async fn send_template<K: Into<String>, V: Into<String>>(
        &self,
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<SmsSubmission> {
        self.submit(SmsMessage::named_template(to, name, parameters)?)
            .await
    }
}

fn render_sms_text(
    mut text: &str,
    parameters: &BTreeMap<String, String>,
) -> NotificationResult<String> {
    let mut content = String::with_capacity(text.len());
    while let Some((literal, tail)) = text.split_once('{') {
        content.push_str(literal);
        let (name, remainder) = tail
            .split_once('}')
            .ok_or_else(|| delivery("The configured SMS text has an unclosed placeholder."))?;
        let value = parameters
            .get(name)
            .ok_or_else(|| delivery("A configured SMS text parameter is missing."))?;
        content.push_str(value);
        text = remainder;
    }
    content.push_str(text);
    Ok(content)
}

/// Resolves SMS drivers for the current application.
pub struct Sms;

#[allow(non_snake_case, reason = "Implements the application facade contract")]
impl Facade for Sms {
    type Service = SmsManager;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<SmsManager>()
    }
}

impl Sms {
    pub fn driver(name: &str) -> NotificationResult<SmsSender> {
        Self::getFacadeRoot().map_err(delivery)?.driver(name)
    }

    pub fn default_driver() -> NotificationResult<SmsSender> {
        Self::getFacadeRoot().map_err(delivery)?.default_driver()
    }

    pub async fn send_text(
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> NotificationResult<SmsSubmission> {
        Self::default_driver()?.send_text(to, content).await
    }

    pub async fn send_template<K: Into<String>, V: Into<String>>(
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<SmsSubmission> {
        Self::default_driver()?
            .send_template(to, name, parameters)
            .await
    }
}
