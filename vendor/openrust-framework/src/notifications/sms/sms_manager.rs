use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use serde_json::Value;

use crate::contracts::config::Repository;

use super::{NotificationResult, SmsDriver, SmsSender, delivery, providers};

type ResolvedDriver = Arc<Mutex<Option<SmsSender>>>;

type DriverFactory = dyn Fn(&Value) -> NotificationResult<Arc<dyn SmsDriver>> + Send + Sync;

/// Resolves named profiles from the current application's SMS configuration.
pub struct SmsManager {
    config: Arc<dyn Repository>,
    drivers: RwLock<HashMap<String, ResolvedDriver>>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
}

impl SmsManager {
    pub(in crate::notifications) fn new(config: Arc<dyn Repository>) -> Self {
        let manager = Self {
            config,
            drivers: RwLock::default(),
            factories: RwLock::default(),
        };
        providers::register(&manager);
        manager
    }

    pub fn extend(
        &self,
        name: impl Into<String>,
        factory: impl Fn(&Value) -> NotificationResult<Arc<dyn SmsDriver>> + Send + Sync + 'static,
    ) -> &Self {
        self.factories
            .write()
            .expect("SMS driver registry lock poisoned")
            .insert(name.into(), Arc::new(factory));
        self
    }

    fn profile_name(&self, name: Option<&str>) -> NotificationResult<String> {
        if let Some(name) = name {
            return Ok(name.to_owned());
        }
        match self.config.get("sms.default") {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            _ => Err(delivery("The default SMS profile is invalid.")),
        }
    }

    fn configured_driver(&self, name: &str) -> NotificationResult<(Value, Arc<DriverFactory>)> {
        let profiles = self.config.get("sms.drivers");
        let profile = match profiles.as_ref().and_then(|profiles| profiles.get(name)) {
            Some(profile) => profile.clone(),
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

    pub(in crate::notifications) fn select_profile(
        &self,
        name: Option<&str>,
    ) -> NotificationResult<String> {
        let name = self.profile_name(name)?;
        self.configured_driver(&name)?;
        Ok(name)
    }

    /// Resolves each named profile once until `forget_drivers` is called.
    /// Factories for different profiles can resolve independently.
    pub fn driver(&self, name: &str) -> NotificationResult<SmsSender> {
        let existing = self
            .drivers
            .read()
            .expect("SMS driver cache lock poisoned")
            .get(name)
            .cloned();
        let (slot, configuration) = match existing {
            Some(slot) => (slot, None),
            None => {
                let configuration = self.configured_driver(name)?;
                let slot = self
                    .drivers
                    .write()
                    .expect("SMS driver cache lock poisoned")
                    .entry(name.to_owned())
                    .or_default()
                    .clone();
                (slot, Some(configuration))
            }
        };
        let mut resolved = slot
            .lock()
            .expect("SMS driver initialization lock poisoned");
        if let Some(sender) = resolved.as_ref() {
            return Ok(sender.clone());
        }
        let (profile, factory) = match configuration {
            Some(configuration) => configuration,
            None => self.configured_driver(name)?,
        };
        let driver = factory(&profile)?;
        let sender = SmsSender {
            profile: Arc::new(profile),
            driver,
        };
        *resolved = Some(sender.clone());
        Ok(sender)
    }

    /// Returns the current configured default without creating a driver.
    pub fn get_default_driver(&self) -> NotificationResult<String> {
        self.profile_name(None)
    }

    /// Clears resolved profiles. Existing sender handles retain their original settings.
    /// Register replacement factories and update configuration before calling this method.
    pub fn forget_drivers(&self) -> &Self {
        self.drivers
            .write()
            .expect("SMS driver cache lock poisoned")
            .clear();
        self
    }

    pub fn default_driver(&self) -> NotificationResult<SmsSender> {
        self.driver(&self.get_default_driver()?)
    }
}
