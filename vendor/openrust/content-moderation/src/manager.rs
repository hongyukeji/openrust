use openrust::contracts::config::Repository;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    ModerationDriver, ModerationError, ModerationResult, ProfileRevision,
    credentials::valid_identifier,
};

type DriverFactory = dyn Fn(&Value) -> ModerationResult<Arc<dyn ModerationDriver>> + Send + Sync;
type DriverSlot = Arc<Mutex<Option<Arc<dyn ModerationDriver>>>>;

/// 当前应用的具名配置、驱动工厂和实例快照。
pub struct ModerationManager {
    config: Arc<dyn Repository>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
    drivers: Mutex<HashMap<String, DriverSlot>>,
}

impl ModerationManager {
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
        factory: impl Fn(&Value) -> ModerationResult<Arc<dyn ModerationDriver>> + Send + Sync + 'static,
    ) -> &Self {
        self.factories
            .write()
            .expect("内容审核工厂锁")
            .insert(name.into(), Arc::new(factory));
        self
    }

    pub fn get_default_driver(&self) -> ModerationResult<String> {
        match self.config.get("moderation.default") {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            None | Some(Value::Null) => Err(ModerationError::DefaultNotConfigured),
            _ => Err(ModerationError::InvalidConfiguration("default")),
        }
    }

    pub fn default_driver(&self) -> ModerationResult<Arc<dyn ModerationDriver>> {
        self.driver(&self.get_default_driver()?)
    }

    fn configured_driver(&self, name: &str) -> ModerationResult<(Value, Arc<DriverFactory>)> {
        let profiles = self.config.get("moderation.drivers");
        let profile = profiles
            .as_ref()
            .and_then(|profiles| profiles.get(name))
            .ok_or(ModerationError::ProfileNotConfigured)?;
        Ok((profile.clone(), self.factory(profile)?))
    }

    fn factory(&self, profile: &Value) -> ModerationResult<Arc<DriverFactory>> {
        let driver = profile
            .get("driver")
            .and_then(Value::as_str)
            .filter(|driver| !driver.trim().is_empty())
            .ok_or(ModerationError::InvalidConfiguration(
                "drivers.<profile>.driver",
            ))?;
        self.factories
            .read()
            .expect("内容审核工厂锁")
            .get(driver)
            .cloned()
            .ok_or(ModerationError::DriverNotRegistered)
    }

    fn revision_config(&self, name: &str, revision: &str) -> ModerationResult<Value> {
        if !valid_identifier(name) || !valid_identifier(revision) {
            return Err(ModerationError::InvalidConfiguration("profile_revisions"));
        }
        let revisions = self.config.get("moderation.profile_revisions");
        revisions
            .as_ref()
            .and_then(|revisions| revisions.get(name)?.get(revision))
            .cloned()
            .ok_or(ModerationError::ProfileRevisionUnavailable)
    }

    /// 固定要写入领域任务的配置版本，便于进程重启后解析同一配置。
    pub fn pin_profile(&self, name: &str, revision: &str) -> ModerationResult<ProfileRevision> {
        let profile = self.revision_config(name, revision)?;
        let snapshot = ProfileRevision::configured(name, revision, &profile)?;
        self.factory(&profile)?;
        Ok(snapshot)
    }

    /// 每次复核不可变配置及凭据，旧任务避开默认配置和具名驱动实例缓存。
    pub fn driver_for_revision(
        &self,
        pinned: &ProfileRevision,
    ) -> ModerationResult<Arc<dyn ModerationDriver>> {
        if !pinned.supported() {
            return Err(ModerationError::ProfileRevisionChanged);
        }
        let profile = self.revision_config(pinned.name(), pinned.revision())?;
        let configured = ProfileRevision::configured(pinned.name(), pinned.revision(), &profile)
            .map_err(|_| ModerationError::ProfileRevisionChanged)?;
        if &configured != pinned {
            return Err(ModerationError::ProfileRevisionChanged);
        }
        self.factory(&profile)?(&profile)
    }

    pub fn driver(&self, name: &str) -> ModerationResult<Arc<dyn ModerationDriver>> {
        let existing = self
            .drivers
            .lock()
            .expect("内容审核实例锁")
            .get(name)
            .cloned();
        let (slot, configured) = match existing {
            Some(slot) => (slot, None),
            None => {
                let configured = self.configured_driver(name)?;
                let slot = self
                    .drivers
                    .lock()
                    .expect("内容审核实例锁")
                    .entry(name.to_owned())
                    .or_default()
                    .clone();
                (slot, Some(configured))
            }
        };
        let mut current = slot.lock().expect("内容审核初始化锁");
        if let Some(driver) = &*current {
            return Ok(driver.clone());
        }
        let (profile, factory) = match configured {
            Some(configured) => configured,
            None => self.configured_driver(name)?,
        };
        let driver = factory(&profile)?;
        *current = Some(driver.clone());
        Ok(driver)
    }

    /// 清除当前应用的具名实例。已交给调用方的句柄继续使用原快照。
    pub fn forget_drivers(&self) -> &Self {
        self.drivers.lock().expect("内容审核实例锁").clear();
        self
    }
}
