use crate::{
    Error, Order, PayDriver, Payment, Refund, Result, Scene, VerifiedNotification, alipay::Alipay,
    types::identifier, wechat::Wechat,
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
    Custom(&'static str, Arc<dyn PayDriver>),
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
            Driver::Custom(_, d) => {
                order.validate()?;
                d.pay(scene, order).await
            }
        }
    }
    pub async fn find(&self, out_trade_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.find(out_trade_no).await,
            Driver::Alipay(d) => d.find(out_trade_no).await,
            Driver::Custom(_, d) => {
                identifier(out_trade_no)?;
                d.find(out_trade_no).await
            }
        }
    }
    pub async fn close(&self, out_trade_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.close(out_trade_no).await,
            Driver::Alipay(d) => d.close(out_trade_no).await,
            Driver::Custom(_, d) => {
                identifier(out_trade_no)?;
                d.close(out_trade_no).await
            }
        }
    }
    pub async fn refund(&self, refund: &Refund) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.refund(refund).await,
            Driver::Alipay(d) => d.refund(refund).await,
            Driver::Custom(_, d) => {
                refund.validate()?;
                d.refund(refund).await
            }
        }
    }
    pub async fn find_refund(&self, out_trade_no: &str, out_refund_no: &str) -> Result<Value> {
        match &*self.0 {
            Driver::Wechat(d) => d.find_refund(out_trade_no, out_refund_no).await,
            Driver::Alipay(d) => d.find_refund(out_trade_no, out_refund_no).await,
            Driver::Custom(_, d) => {
                identifier(out_trade_no)?;
                identifier(out_refund_no)?;
                d.find_refund(out_trade_no, out_refund_no).await
            }
        }
    }
    pub fn callback(&self, headers: &http::HeaderMap, body: &[u8]) -> Result<VerifiedNotification> {
        match &*self.0 {
            Driver::Wechat(d) => d.callback(headers, body),
            Driver::Alipay(d) => d.callback(headers, body),
            Driver::Custom(name, d) => Ok(d.callback(headers, body)?.into_verified(name)),
        }
    }
}
type Slot = Arc<Mutex<Option<Gateway>>>;
type DriverFactory = dyn Fn(&Value) -> Result<Arc<dyn PayDriver>> + Send + Sync;
/// Application-local named merchant manager; no process-global payment configuration.
pub struct PayManager {
    config: Arc<dyn Repository>,
    http: Arc<Factory>,
    drivers: RwLock<HashMap<String, Slot>>,
    factories: RwLock<HashMap<&'static str, Arc<DriverFactory>>>,
}
impl PayManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>) -> Self {
        Self {
            config,
            http,
            drivers: RwLock::default(),
            factories: RwLock::default(),
        }
    }
    /// 注册应用内的自定义渠道。已解析句柄继续使用原快照，可调用 `forget_drivers` 刷新后续解析。
    pub fn extend(
        &self,
        name: &'static str,
        factory: impl Fn(&Value) -> Result<Arc<dyn PayDriver>> + Send + Sync + 'static,
    ) -> Result<&Self> {
        if !valid_name(name) || matches!(name, "wechat" | "alipay") {
            return Err(Error::Configuration("custom driver name"));
        }
        self.factories
            .write()
            .expect("pay driver registry")
            .insert(name, Arc::new(factory));
        Ok(self)
    }
    pub fn get_default_driver(&self) -> Result<String> {
        self.config
            .get("pay.default")
            .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .ok_or(Error::Configuration("pay.default"))
    }
    pub fn driver(&self, name: &str) -> Result<Gateway> {
        if !valid_name(name) {
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
            Some(name) => {
                let (name, factory) = self
                    .factories
                    .read()
                    .expect("pay driver registry")
                    .get_key_value(name)
                    .map(|(&name, factory)| (name, factory.clone()))
                    .ok_or(Error::Configuration("driver"))?;
                Driver::Custom(name, factory(profile)?)
            }
            None => return Err(Error::Configuration("driver")),
        };
        let gateway = Gateway(Arc::new(driver));
        *slot = Some(gateway.clone());
        Ok(gateway)
    }
    pub fn forget_drivers(&self) -> &Self {
        self.drivers.write().expect("pay cache").clear();
        self
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
