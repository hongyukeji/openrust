use crate::{Passkeys, http};
use openrust::contracts::container::Container as _;
use openrust::{
    contracts::foundation::Application as _,
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;

pub struct PasskeysServiceProvider;
impl ServiceProvider for PasskeysServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let config = app
                    .container()
                    .get::<dyn openrust::contracts::config::Repository>("config")?;
                let url = config
                    .get("app.url")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let host = url::Url::parse(&url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned));
                let environment = app.make::<openrust::config::Environment>().await?;
                let secret = environment
                    .get_utf8("PASSKEYS_USER_HANDLE_SECRET")?
                    .map(serde_json::Value::from)
                    .or_else(|| config.get("app.key"));
                self.merge_config_from(app,json!({"relying_party_id":host,"allowed_origins":[url],"user_handle_secret":secret,"timeout":60000,"guard":"web","middleware":["web"],"management_middleware":["password.confirm"],"throttle":"throttle:6,1","redirect":"/"}),"passkeys")?;
            }
            http::register(app)
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            crate::resources::register(app)?;
            if app.make::<Passkeys>().await?.shouldRegisterRoutes() {
                app.make::<Passkeys>().await?.webauthn()?;
                self.load_routes_from(app, crate::routes::register)?;
            }
            Ok(())
        })
    }
}
