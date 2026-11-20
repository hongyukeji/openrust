use crate::{Error, Gateway, PayManager, Result};
use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Pay;
impl Facade for Pay {
    type Service = PayManager;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<PayManager>()
    }
}
impl Pay {
    pub fn driver(name: &str) -> Result<Gateway> {
        Self::getFacadeRoot()
            .map_err(|_| Error::Context)?
            .driver(name)
    }
    pub fn defaultDriver() -> Result<Gateway> {
        let manager = Self::getFacadeRoot().map_err(|_| Error::Context)?;
        manager.driver(&manager.getDefaultDriver()?)
    }
    pub fn forgetDrivers() -> Result<()> {
        Self::getFacadeRoot()
            .map_err(|_| Error::Context)?
            .forgetDrivers();
        Ok(())
    }
}
