use crate::{
    container::{Container, ServiceKey},
    contracts::session::SessionHandler,
    http::Request,
    support::facades::Facade,
};

use super::{SessionError, SessionManager, SessionResult};

/// Registers session drivers for the current application.
pub struct Session;

impl Facade for Session {
    type Service = SessionManager;

    fn get_facade_accessor() -> ServiceKey {
        "session".into()
    }
}

impl Session {
    /// Registers a handler factory on the current application's session manager.
    pub fn extend<H: SessionHandler + 'static>(
        driver: impl Into<String>,
        factory: impl Fn(&Container, &Request) -> SessionResult<H> + Send + Sync + 'static,
    ) -> SessionResult<()> {
        Self::get_facade_root()
            .map_err(|_| SessionError::Configuration)?
            .extend(driver, factory)?;
        Ok(())
    }
}
