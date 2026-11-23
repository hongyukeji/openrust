use crate::{Error, Gateway, PayManager, Result};
use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Pay;
impl Facade for Pay {
    type Service = PayManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<PayManager>()
    }
}
impl Pay {
    pub fn driver(name: &str) -> Result<Gateway> {
        Self::get_facade_root()
            .map_err(|_| Error::Context)?
            .driver(name)
    }
    pub fn default_driver() -> Result<Gateway> {
        let manager = Self::get_facade_root().map_err(|_| Error::Context)?;
        manager.driver(&manager.get_default_driver()?)
    }
    pub fn forget_drivers() -> Result<()> {
        Self::get_facade_root()
            .map_err(|_| Error::Context)?
            .forget_drivers();
        Ok(())
    }
}
