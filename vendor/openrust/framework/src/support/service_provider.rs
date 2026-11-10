use std::{
    any::{Any, type_name},
    error::Error,
    future::Future,
    pin::Pin,
};

use crate::foundation::Application;

/// The result of a service provider lifecycle hook.
pub type ProviderResult = Result<(), Box<dyn Error + Send + Sync>>;

/// Registers service bindings before starting application services.
pub trait ServiceProvider: Any + Send + Sync {
    /// Names the provider in startup diagnostics.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Register bindings without starting services.
    fn register(&mut self, app: &Application) -> ProviderResult;

    /// Initialize services after all providers have registered.
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>>;
}
