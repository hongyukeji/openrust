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
            #[cfg(feature = "sanctum")]
            auth.registerSanctumModel::<crate::app::models::User>("app::models::User");
            #[cfg(not(feature = "sanctum"))]
            auth.registerModel::<crate::app::models::User>("app::models::User");
            Ok(())
        })
    }

    fn boot(&mut self, _app: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async { Ok(()) })
    }
}
