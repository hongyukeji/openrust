//! OpenID Connect relying-party client: authorization code, PKCE and verified identities.
#![forbid(unsafe_code)]
#![allow(non_snake_case, reason = "Uses OpenRust's Laravel-style service API")]
mod config;
mod identity;
mod provider;
pub use config::ProviderConfig;
pub use identity::{Authentication, LogoutHint, Tokens, VerifiedIdentity};
use openrust::{
    cache::CacheManager,
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpFuture, Request, client::Factory},
    support::{ProviderResult, ServiceProvider},
};
pub use provider::Provider;
use std::sync::Arc;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid OIDC configuration: {0}")]
    Configuration(&'static str),
    #[error("OIDC provider [{0}] is not configured.")]
    UnknownProvider(String),
    #[error("The OIDC discovery document is invalid.")]
    InvalidDiscovery,
    #[error("The OIDC authorization attempt is invalid or already used.")]
    InvalidState,
    #[error("The OIDC provider rejected the authorization request.")]
    AuthorizationDenied,
    #[error("The OIDC token response is invalid.")]
    InvalidResponse,
    #[error("The OIDC identity token is invalid.")]
    InvalidToken,
    #[error("The OIDC UserInfo subject does not match the verified identity.")]
    InvalidUserInfo,
    #[error("The OIDC provider does not support RP initiated logout.")]
    UnsupportedLogout,
    #[error("The OIDC logout context is unavailable or invalid.")]
    InvalidLogout,
    #[error("The OIDC provider returned HTTP {0}.")]
    ProviderStatus(u16),
    #[error("The OIDC HTTP request failed.")]
    Http(#[from] openrust::http::client::Error),
    #[error("The OIDC application service is unavailable.")]
    Unavailable(#[from] openrust::contracts::container::BindingResolutionError),
    #[error("The OIDC replay store failed.")]
    Cache(#[from] openrust::cache::CacheError),
    #[error("The OIDC Session is unavailable.")]
    Session(#[from] openrust::session::SessionError),
    #[error("The OIDC callback is malformed.")]
    Request(#[from] openrust::http::HttpError),
    #[error("The OIDC redirect is invalid.")]
    Redirect(#[from] openrust::routing::UrlError),
    #[error("Cryptographic random generation failed.")]
    Random,
}
pub type Result<T> = std::result::Result<T, Error>;
pub struct OidcManager {
    config: Arc<dyn Repository>,
    http: Arc<Factory>,
    cache: Arc<CacheManager>,
}
impl OidcManager {
    pub fn new(config: Arc<dyn Repository>, http: Arc<Factory>, cache: Arc<CacheManager>) -> Self {
        Self {
            config,
            http,
            cache,
        }
    }
    pub fn provider<'a>(&self, name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Configuration("provider name"));
        }
        let value = self
            .config
            .get(&format!("oidc.providers.{name}"))
            .filter(|v| !v.is_null())
            .ok_or_else(|| Error::UnknownProvider(name.into()))?;
        let config = ProviderConfig::from_config(&value)?;
        let cache = self.cache.store(config.replay_store.as_deref())?;
        Ok(Provider::new(
            name.into(),
            request,
            config,
            self.http.clone(),
            cache,
        ))
    }
}
pub struct OidcServiceProvider;
impl ServiceProvider for OidcServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configurationIsCached() {
                self.mergeConfigFrom(app, serde_json::json!({"providers":{}}), "oidc")?;
            }
            app.container().singleton::<OidcManager, _>(|c| {
                Ok(Arc::new(OidcManager::new(
                    c.get("config")?,
                    c.make::<Factory>()?,
                    c.make::<CacheManager>()?,
                )))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/oidc.rs", include_bytes!("../config/oidc.rs"))],
                "oidc-config",
            )?;
            app.container()
                .make::<openrust::foundation::console::installation::ExtensionInstallers>()?
                .register("oidc", install_configuration);
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(OidcServiceProvider)]
}
pub mod facades {
    use super::*;
    use openrust::{container::ServiceKey, support::facades::Facade};
    pub struct Oidc;
    impl Facade for Oidc {
        type Service = OidcManager;
        fn getFacadeAccessor() -> ServiceKey {
            ServiceKey::typed::<OidcManager>()
        }
    }
    impl Oidc {
        pub fn provider<'a>(name: &str, request: &'a mut Request) -> Result<Provider<'a>> {
            Self::getFacadeRoot()?.provider(name, request)
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/identity.rs"]
mod identity_tests;

fn install_configuration<'a>(
    installation: &'a openrust::foundation::console::installation::SourceInstallation<'a>,
    options: openrust::foundation::console::installation::InstallationOptions<'a>,
    output: &'a mut openrust::console::Output,
) -> openrust::console::ConsoleFuture<'a, std::result::Result<(), openrust::console::ConsoleError>>
{
    Box::pin(async move {
        installation
            .publish("oidc-config", options.force, output)
            .await?;
        installation.registerConfiguration("oidc")
    })
}
