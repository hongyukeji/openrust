use crate::{CashierManager, Client, Error, Result};
use openrust::{container::ServiceKey, support::facades::Facade};
pub struct Cashier;
impl Facade for Cashier {
    type Service = CashierManager;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<CashierManager>()
    }
}
impl Cashier {
    pub fn driver(name: &str) -> Result<Client> {
        Self::get_facade_root()
            .map_err(|_| Error::Configuration("application context"))?
            .driver(name)
    }
    pub fn defaultDriver() -> Result<Client> {
        let manager =
            Self::get_facade_root().map_err(|_| Error::Configuration("application context"))?;
        manager.driver(&manager.get_default_driver()?)
    }
    pub fn forgetDrivers() -> Result<()> {
        Self::get_facade_root()
            .map_err(|_| Error::Configuration("application context"))?
            .forgetDrivers();
        Ok(())
    }
}
