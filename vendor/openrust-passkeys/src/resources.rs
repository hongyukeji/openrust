use crate::PasskeysServiceProvider;
use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/// Publishes standalone resources without requiring a configured WebAuthn origin.
pub struct PasskeysResourcesServiceProvider;
impl ServiceProvider for PasskeysResourcesServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move { register(app) })
    }
}
pub(crate) fn register(app: &Application) -> ProviderResult {
    let provider = PasskeysServiceProvider;
    provider.publishes(
        app,
        &[(
            "config/passkeys.rs",
            include_bytes!("../config/passkeys.rs"),
        )],
        "passkeys-config",
    )?;
    provider.publishes_migrations(
        app,
        &[(
            "database/migrations/2024_01_01_000000_create_passkeys_table.rs",
            include_bytes!("../database/migrations/2024_01_01_000000_create_passkeys_table.rs"),
        )],
        "passkeys-migrations",
    )?;
    Ok(())
}
