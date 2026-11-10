use std::{
    any::{Any, TypeId, type_name},
    error::Error,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use crate::foundation::Application;

/// The result of a service provider lifecycle hook.
pub type ProviderResult = Result<(), Box<dyn Error + Send + Sync>>;

/// Compiled constructor used for a fresh deferred registration attempt.
pub type ProviderFactory = dyn Fn() -> Box<dyn ServiceProvider> + Send + Sync;

/// Registers service bindings before starting application services.
pub trait ServiceProvider: Any + Send + Sync {
    /// Names the provider in startup diagnostics.
    fn name(&self) -> &'static str {
        type_name::<Self>()
    }

    #[doc(hidden)]
    #[allow(non_snake_case, reason = "Rust provider type identity adapter")]
    fn providerType(&self) -> TypeId {
        self.type_id()
    }

    #[doc(hidden)]
    #[allow(non_snake_case, reason = "Rust compiled provider constructor adapter")]
    fn deferredFactory(&self) -> Option<Arc<ProviderFactory>> {
        None
    }

    /// Declare deferred service keys instead of eagerly registering this provider.
    #[allow(non_snake_case, reason = "Matches Laravel's provider API")]
    fn isDeferred(&self) -> bool {
        false
    }

    /// Exact typed or named service identities provided by a deferred provider.
    fn provides(&self) -> Vec<crate::container::ServiceKey> {
        Vec::new()
    }

    /// Register bindings without starting services.
    fn register<'a>(
        &'a mut self,
        _app: &'a Application,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Initialize services after all providers have registered.
    fn boot<'a>(
        &'a mut self,
        _app: &'a Application,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
