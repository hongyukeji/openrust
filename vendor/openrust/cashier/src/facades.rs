use crate::{CashierManager, Client, Error, Result};
use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Cashier;
impl Facade for Cashier {
    type Service = CashierManager;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<CashierManager>()
    }
}
impl Cashier {
    pub fn driver(name: &str) -> Result<Client> {
        Self::getFacadeRoot()
            .map_err(|_| Error::Configuration("application context"))?
            .driver(name)
    }
    pub fn defaultDriver() -> Result<Client> {
        let manager =
            Self::getFacadeRoot().map_err(|_| Error::Configuration("application context"))?;
        manager.driver(&manager.getDefaultDriver()?)
    }
    pub fn forgetDrivers() -> Result<()> {
        Self::getFacadeRoot()
            .map_err(|_| Error::Configuration("application context"))?
            .forgetDrivers();
        Ok(())
    }
}
