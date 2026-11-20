//! Third-party login through the application's HTTP client and Session.
#![forbid(unsafe_code)]
#![allow(non_snake_case, reason = "Matches Laravel Socialite public API")]
pub mod contracts;
mod error;
pub mod facades;
mod manager;
pub mod one;
mod provider;
pub mod two;
pub mod wechat;
pub use error::{Error, Result};
pub use manager::SocialiteManager;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
pub use provider::Provider;
use std::sync::Arc;
pub struct SocialiteServiceProvider;
impl ServiceProvider for SocialiteServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singleton::<SocialiteManager, _>(|container| {
                    Ok(Arc::new(SocialiteManager::new(
                        container.get("config")?,
                        container.make::<openrust::http::client::Factory>()?,
                    )))
                })?;
            Ok(())
        })
    }
}
pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(SocialiteServiceProvider)]
}
