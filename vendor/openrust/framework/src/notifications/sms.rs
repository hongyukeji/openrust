//! Selectable SMS drivers for direct sends and the SMS notification channel.

mod providers;

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
        };
        message.validate()?;
        Ok(message)
    }

    pub fn template(to: impl Into<String>, template: SmsTemplate) -> NotificationResult<Self> {
        let message = Self {
            to: to.into(),
            content: String::new(),
            template: Some(template),
        };
        message.validate()?;
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
            None => Ok("vonage".into()),
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            _ => Err(delivery("The default SMS profile is invalid.")),
        }
    }

    fn configured_driver(&self, name: &str) -> NotificationResult<(Value, Arc<DriverFactory>)> {
        let profiles = self.config.get("sms.drivers");
        let profile = match profiles.as_ref().and_then(|profiles| profiles.get(name)) {
            Some(profile) => profile.clone(),
            None if profiles.is_none() && name == "vonage" => {
                serde_json::json!({"driver":"vonage"})
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

    /// Uses `sms.default` when no profile name is supplied.
    pub fn driver(&self, name: Option<&str>) -> NotificationResult<Arc<dyn SmsDriver>> {
        let (profile, factory) = self.configured_driver(&self.profile_name(name)?)?;
        factory(&profile)
    }
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
    pub fn driver(name: Option<&str>) -> NotificationResult<Arc<dyn SmsDriver>> {
        Self::getFacadeRoot().map_err(delivery)?.driver(name)
    }
}
