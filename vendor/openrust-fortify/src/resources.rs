use crate::FortifyServiceProvider;
use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/// Discovers installation resources without enabling authentication routes.
pub struct FortifyResourcesServiceProvider;
impl ServiceProvider for FortifyResourcesServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move { register(app) })
    }
}
pub(crate) fn register(app: &Application) -> ProviderResult {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    app.container()
        .make::<openrust::foundation::console::installation::ExtensionInstallers>()?
        .register("fortify", crate::console::install_command::initialize);
    crate::starter::install::register(app)?;
    let provider = FortifyServiceProvider;
    provider.publishes(
        app,
        &[("config/fortify.rs", include_bytes!("../stubs/fortify.rs"))],
        "fortify-config",
    )?;
    provider.publishes(
        app,
        &[
            (
                "app/actions/mod.rs",
                b"#[cfg(feature = \"fortify\")]\npub(crate) mod fortify;\n",
            ),
            (
                "app/actions/fortify/mod.rs",
                include_bytes!("../stubs/mod.rs"),
            ),
            (
                "app/actions/fortify/password_validation_rules.rs",
                include_bytes!("../stubs/password_validation_rules.rs"),
            ),
            (
                "app/actions/fortify/create_new_user.rs",
                include_bytes!("../stubs/create_new_user.rs"),
            ),
            (
                "app/actions/fortify/reset_user_password.rs",
                include_bytes!("../stubs/reset_user_password.rs"),
            ),
            (
                "app/actions/fortify/update_user_password.rs",
                include_bytes!("../stubs/update_user_password.rs"),
            ),
            (
                "app/actions/fortify/update_user_profile_information.rs",
                include_bytes!("../stubs/update_user_profile_information.rs"),
            ),
            (
                "app/providers/fortify_service_provider.rs",
                include_bytes!("../stubs/fortify_service_provider.rs"),
            ),
        ],
        "fortify-support",
    )?;
    provider.publishes_migrations(
        app,
        &[(
            "database/migrations/2024_01_01_000000_create_passkeys_table.rs",
            include_bytes!("../../openrust-passkeys/database/migrations/2024_01_01_000000_create_passkeys_table.rs"),
        ), (
            "database/migrations/2014_10_12_200000_add_two_factor_columns_to_users_table.rs",
            include_bytes!(
                "../database/migrations/2014_10_12_200000_add_two_factor_columns_to_users_table.rs"
            ),
        )],
        "fortify-migrations",
    )?;
    provider.commands(app, crate::console::register);
    Ok(())
}
