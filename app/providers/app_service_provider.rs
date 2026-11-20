use openrust::{
    auth::AuthManager,
    contracts::{container::Container as _, foundation::Application as _},
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
            let auth = app.container().make::<AuthManager>()?;
            auth.registerModel::<crate::app::models::User>("app::models::User");
            #[cfg(feature = "settings")]
            openrust_settings::SettingsServiceProvider::registerSettings::<
                crate::app::settings::SiteSettings,
            >(app)
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, _app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            #[cfg(feature = "settings")]
            self.loadMigrationsFrom(
                _app,
                openrust_settings::SettingsServiceProvider::migrations(),
            )
            .await?;
            Ok(())
        })
    }
}
