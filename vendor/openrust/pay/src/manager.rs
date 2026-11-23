use crate::{
    Error, Order, Payment, Refund, Result, Scene, VerifiedNotification, alipay::Alipay,
    wechat::Wechat,
};
use openrust::{contracts::config::Repository, http::client::Factory};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

enum Driver {
    Wechat(Wechat),
    Alipay(Alipay),
}
/// A resolved merchant and immutable configuration/key snapshot.
#[derive(Clone)]
pub struct Gateway(Arc<Driver>);
impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gateway([REDACTED])")
    }
}
impl Gateway {
    pub async fn pay(&self, scene: Scene, order: &Order) -> Result<Payment> {
        match &*self.0 {
            Driver::Wechat(d) => d.pay(scene, order).await,
            Driver::Alipay(d) => d.pay(scene, order).await,
        }
    }
    pub async fn find(&self, out_trade_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.find(out_trade_no).await,
            Driver::Alipay(d) => d.find(out_trade_no).await,
        }
    }
    pub async fn close(&self, out_trade_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.close(out_trade_no).await,
            Driver::Alipay(d) => d.close(out_trade_no).await,
        }
    }
    pub async fn refund(&self, refund: &Refund) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.refund(refund).await,
            Driver::Alipay(d) => d.refund(refund).await,
        }
    }
    pub async fn findRefund(&self, out_trade_no: &str, out_refund_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.find_refund(out_trade_no, out_refund_no).await,
            Driver::Alipay(d) => d.find_refund(out_trade_no, out_refund_no).await,
        }
    }
    pub fn callback(&self, headers: &http::HeaderMap, body: &[u8]) -> Result<VerifiedNotification> {
        match &*self.0 {
            Driver::Wechat(d) => d.callback(headers, body),
            Driver::Alipay(d) => d.callback(headers, body),
        }
    }
}
type Slot = Arc<Mutex<Option<Gateway>>>;
/// Application-local named merchant manager; no process-global payment configuration.
pub struct PayManager {
    config: Arc<dyn Repository>,
    http: Arc<Factory>,
    drivers: RwLock<HashMap<String, Slot>>,
}
impl PayManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>) -> Self {
        Self {
            config,
            http,
            drivers: RwLock::default(),
        }
    }
    pub fn get_default_driver(&self) -> Result<String> {
        self.config
            .get("pay.default")
            .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .ok_or(Error::Configuration("pay.default"))
    }
    pub fn driver(&self, name: &str) -> Result<Gateway> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        {
            return Err(Error::Configuration("driver name"));
        }
        let existing = self.drivers.read().expect("pay cache").get(name).cloned();
        let slot = match existing {
            Some(slot) => slot,
            None => {
                let profiles = self
                    .config
                    .get("pay.drivers")
                    .ok_or(Error::Configuration("pay.drivers"))?;
                if profiles.get(name).is_none() {
                    return Err(Error::Configuration("merchant profile"));
                }
                self.drivers
                    .write()
                    .expect("pay cache")
                    .entry(name.into())
                    .or_default()
                    .clone()
            }
        };
        let mut slot = slot.lock().expect("pay initialization");
        if let Some(driver) = slot.as_ref() {
            return Ok(driver.clone());
        }
        let profiles = self
            .config
            .get("pay.drivers")
            .ok_or(Error::Configuration("pay.drivers"))?;
        let profile = profiles
            .get(name)
            .ok_or(Error::Configuration("merchant profile"))?;
        let driver = match profile["driver"].as_str() {
            Some("wechat") => Driver::Wechat(Wechat::new(profile, self.http.clone())?),
            Some("alipay") => Driver::Alipay(Alipay::new(profile, self.http.clone())?),
            _ => return Err(Error::Configuration("driver")),
        };
        let gateway = Gateway(Arc::new(driver));
        *slot = Some(gateway.clone());
        Ok(gateway)
    }
    pub fn forgetDrivers(&self) -> &Self {
        self.drivers.write().expect("pay cache").clear();
        self
    }
}
