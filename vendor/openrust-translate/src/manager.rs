use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use openrust::contracts::config::Repository;
use serde_json::Value;

use crate::{Translation, TranslationError, TranslationRequest, TranslationResult, Translator};

type DriverFactory = dyn Fn(&Value) -> TranslationResult<Arc<dyn Translator>> + Send + Sync;
type DriverSlot = Arc<Mutex<Option<Arc<dyn Translator>>>>;

/// 当前应用的具名文本翻译配置、驱动工厂和已解析实例。
pub struct TranslateManager {
    config: Arc<dyn Repository>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
    drivers: Mutex<HashMap<String, DriverSlot>>,
}

impl TranslateManager {
    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            factories: RwLock::default(),
            drivers: Mutex::default(),
        }
    }

    pub fn extend(
        &self,
        name: impl Into<String>,
        factory: impl Fn(&Value) -> TranslationResult<Arc<dyn Translator>> + Send + Sync + 'static,
    ) -> &Self {
        self.factories
            .write()
            .unwrap()
            .insert(name.into(), Arc::new(factory));
        self
    }

    pub fn get_default_driver(&self) -> TranslationResult<String> {
        match self.config.get("translate.default") {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            None | Some(Value::Null) => Err(TranslationError::DefaultNotConfigured),
            _ => Err(TranslationError::InvalidConfiguration("default")),
        }
    }

    pub fn default_driver(&self) -> TranslationResult<Arc<dyn Translator>> {
        self.driver(&self.get_default_driver()?)
    }

    fn configured_driver(&self, name: &str) -> TranslationResult<(Value, Arc<DriverFactory>)> {
        let profile = self
            .config
            .get("translate.drivers")
            .and_then(|profiles| profiles.get(name).cloned())
            .ok_or(TranslationError::ProfileNotConfigured)?;
        let driver = profile
            .get("driver")
            .and_then(Value::as_str)
            .filter(|driver| !driver.trim().is_empty())
            .ok_or(TranslationError::InvalidConfiguration(
                "drivers.<profile>.driver",
            ))?;
        let factory = self
            .factories
            .read()
            .unwrap()
            .get(driver)
            .cloned()
            .ok_or(TranslationError::DriverNotRegistered)?;
        Ok((profile, factory))
    }

    pub fn driver(&self, name: &str) -> TranslationResult<Arc<dyn Translator>> {
        let existing = self.drivers.lock().unwrap().get(name).cloned();
        let (slot, config) = match existing {
            Some(slot) => (slot, None),
            None => {
                let config = self.configured_driver(name)?;
                let slot = self
                    .drivers
                    .lock()
                    .unwrap()
                    .entry(name.to_owned())
                    .or_default()
                    .clone();
                (slot, Some(config))
            }
        };
        let mut current = slot.lock().unwrap();
        if let Some(driver) = &*current {
            return Ok(driver.clone());
        }
        let (profile, factory) = match config {
            Some(config) => config,
            None => self.configured_driver(name)?,
        };
        let driver = factory(&profile)?;
        *current = Some(driver.clone());
        Ok(driver)
    }

    /// 清除具名驱动实例。已取得的句柄继续使用原配置。
    pub fn forget_drivers(&self) -> &Self {
        self.drivers.lock().unwrap().clear();
        self
    }

    pub async fn translate(
        &self,
        text: impl Into<String>,
        target: &str,
    ) -> TranslationResult<Translation> {
        self.translate_request(TranslationRequest::new(text, target)?)
            .await
    }

    pub async fn translate_request(
        &self,
        request: TranslationRequest,
    ) -> TranslationResult<Translation> {
        self.default_driver()?.translate(request).await
    }
}
