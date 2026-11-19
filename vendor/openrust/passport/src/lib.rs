//! Laravel Passport's application configuration, publishable schema and key command.
#![forbid(unsafe_code)]
mod api_token_cookie_factory;
mod auth_code;
pub mod bridge;
pub mod contracts;
mod device_code;
mod testing;
pub use api_token_cookie_factory::ApiTokenCookieFactory;
pub use contracts::TransientToken;
pub mod events;
mod personal_access_token_factory;
pub use device_code::DeviceCode;
pub use personal_access_token_factory::{
    HasApiTokens, PersonalAccessTokenFactory, PersonalAccessTokenResult, PersonalToken,
};
mod lifetime;
pub use lifetime::GrantLifetime;
mod authorization_server;
pub use authorization_server::DeviceCodeResponse;
pub use authorization_server::{AuthorizationRequest, AuthorizationServer, TokenResponse};
mod authorization_response;
mod error;
mod parameters;
mod pkce;
mod redirect_uri;
pub use authorization_response::{AuthorizationFailure, AuthorizationRedirect};
pub use error::OAuthError;
pub use parameters::OAuthParameters;
pub use pkce::{CodeChallenge, CodeChallengeMethod, PkceError, verify_optional_challenge};
pub use redirect_uri::{
    RedirectUriError, ValidatedRedirectUri, matches_registered_redirect_uri,
    validate_token_redirect_uri,
};
mod client;
mod client_record;
mod client_repository;
#[path = "../config/passport.rs"]
mod configuration;
pub mod console;
pub mod database;
pub mod guards;
pub mod http;
mod resource_server;
pub use resource_server::{AccessToken, ResourceServer};
mod keys;
pub mod migrations;
mod passport;
mod php_array;
mod refresh_token;
mod scope;
mod scope_json;
pub use scope_json::ScopeJsonOptions;
mod grant_store;
mod token;
mod token_store;

pub use auth_code::AuthCode;
pub use client::{Client, ClientModel};
pub use client_record::ClientRecord;
pub use client_repository::{ClientOwner, ClientRepository, ClientRepositoryError};
pub use keys::{CryptKey, KeyError, PrivateKey, PublicKey};
pub use passport::{Passport, TokenEncryptionKey};
pub mod exceptions {
    pub use crate::http::middleware::MissingScopeException;
}
pub use php_array::PhpArray;
pub use refresh_token::{RefreshToken, RefreshTokenModel};
pub use scope::Scope;
pub use token::{Token, TokenModel};

use openrust::{
    config::Environment,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

/// Registers Passport configuration and the implemented publishing / key lifecycle.
pub struct PassportServiceProvider;
impl ServiceProvider for PassportServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let env = app.make::<Environment>().await?;
            self.mergeConfigFrom(app, configuration::load(&env)?, "passport")?;
            app.container().singleton::<Passport, _>(|container| {
                Ok(Arc::new(Passport::new(
                    container.get("config")?,
                    container.make::<openrust::foundation::Paths>()?,
                    container.make::<openrust::database::DatabaseManager>()?,
                )))
            })?;
            app.container().scoped::<ClientRepository, _>(|container| {
                let passport = container.make::<Passport>()?;
                Ok(Arc::new(passport.clientRepository()))
            })?;
            app.container()
                .scoped::<AuthorizationServer, _>(|container| {
                    use sha2::{Digest as _, Sha256};
                    let paths = container.make::<openrust::foundation::Paths>()?;
                    let domain = format!(
                        "{:x}",
                        Sha256::digest(paths.storagePath("").to_string_lossy().as_bytes())
                    );
                    Ok(Arc::new(AuthorizationServer::new(
                        container.make::<Passport>()?,
                        container.make::<ClientRepository>()?,
                        container.make::<openrust::encryption::Encrypter>()?,
                        domain,
                    )))
                })?;
            app.container().singleton::<PrivateKey, _>(|container| {
                let passport = container.make::<Passport>()?;
                let key = passport.make_crypt_key("private").map_err(|error| {
                    openrust::contracts::container::BindingResolutionError::FactoryFailed {
                        service: "Passport cryptographic key",
                        source: Box::new(error),
                    }
                })?;
                Ok(Arc::new(PrivateKey(key)))
            })?;
            app.container().scoped::<ResourceServer, _>(|container| {
                Ok(Arc::new(ResourceServer::new(
                    container.make::<Passport>()?,
                    container.make::<PublicKey>()?,
                )))
            })?;
            app.container().singleton::<PublicKey, _>(|container| {
                let passport = container.make::<Passport>()?;
                let key = passport.make_crypt_key("public").map_err(|error| {
                    openrust::contracts::container::BindingResolutionError::FactoryFailed {
                        service: "Passport cryptographic key",
                        source: Box::new(error),
                    }
                })?;
                Ok(Arc::new(PublicKey(key)))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            // Laravel publishes these migrations; it does not silently run package schema.
            self.publishesMigrations(app, migrations::SOURCES, "passport-migrations")?;
            self.publishes(
                app,
                &[(
                    "config/passport.rs",
                    include_bytes!("../config/passport.rs"),
                )],
                "passport-config",
            )?;
            self.commands(app, console::commands::register);
            app.make::<openrust::auth::AuthManager>()
                .await?
                .extend("passport", guards::TokenGuard::resolve);
            self.loadRoutesFrom(app, http::routes)?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(PassportServiceProvider)]
}
