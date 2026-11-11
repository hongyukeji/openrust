use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{migrations::MigrationRegistry, seeder::SeederRegistry},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

/// The default registration and boot hooks for application services.
pub(crate) struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .instance::<MigrationRegistry>(std::sync::Arc::new(MigrationRegistry(
                    crate::database::migrations::ENTRIES,
                )))?;
            app.container()
                .instance::<SeederRegistry>(std::sync::Arc::new(SeederRegistry(
                    crate::database::seeders::ENTRIES,
                )))?;
            Ok(())
        })
    }

    fn boot(&mut self, _app: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async { Ok(()) })
    }
}
