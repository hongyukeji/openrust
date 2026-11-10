use std::sync::Arc;

mod binding_resolution_error;

pub use binding_resolution_error::BindingResolutionError;

/// Registers services by type and resolves them through explicit factories.
pub trait Container: Send + Sync + Sized {
    /// Register a factory that is invoked for each resolution.
    fn bind<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    /// Register a factory whose first successful result is shared.
    fn singleton<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    /// Register an existing shared instance.
    fn instance<T>(&self, service: Arc<T>)
    where
        T: ?Sized + Send + Sync + 'static;

    /// Check whether the service type has a binding.
    fn bound<T>(&self) -> bool
    where
        T: ?Sized + Send + Sync + 'static;

    /// Resolve the binding for the requested service type.
    fn make<T>(&self) -> Result<Arc<T>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static;
}
