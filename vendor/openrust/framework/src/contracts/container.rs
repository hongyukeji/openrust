use std::sync::Arc;

use crate::container::{ContextualBindingBuilder, ServiceKey, Tagged};

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

    /// Register a factory under a named identity instead of a Rust type.
    #[allow(non_snake_case, reason = "Named overload for Rust's typed bind")]
    fn bindNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Named overload for Rust's typed singleton")]
    fn singletonNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Named overload for Rust's typed instance")]
    fn instanceNamed<T: ?Sized + Send + Sync + 'static>(&self, name: &str, service: Arc<T>);

    /// Point an alias at another typed or named identity.
    fn alias(
        &self,
        r#abstract: impl Into<ServiceKey>,
        alias: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isAlias(&self, name: impl Into<ServiceKey>) -> bool;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn getAlias(&self, name: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError>;

    /// Tests whether a typed or named entry is bound or registered as an alias.
    fn has(&self, id: impl Into<ServiceKey>) -> bool;

    /// Resolves a typed or named entry and checks its Rust output type.
    fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey>,
    ) -> Result<Arc<T>, BindingResolutionError>;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isShared(&self, id: impl Into<ServiceKey>) -> bool;
    fn resolved(&self, id: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError>;
    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstance(&self, id: impl Into<ServiceKey>);
    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetInstances(&self);

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetScopedInstances(&self);
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scoped<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIf<T, F>(&self, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIfNamed<T, F>(&self, name: &str, factory: F)
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn addContextualBinding<T, F>(
        &self,
        concrete: impl Into<ServiceKey>,
        r#abstract: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    /// Appends raw service identities to each tag, preserving order and duplicates.
    fn tag(
        &self,
        abstracts: impl IntoIterator<Item = impl Into<ServiceKey>>,
        tags: impl IntoIterator<Item = impl Into<String>>,
    );

    /// Returns a lazy, rewindable view of services carrying this tag.
    fn tagged<T: ?Sized + Send + Sync + 'static>(&self, tag: &str) -> Tagged<'_, Self, T>;

    fn when<T: ?Sized + 'static>(
        &self,
    ) -> Result<ContextualBindingBuilder<'_, Self>, BindingResolutionError> {
        self.whenMany([ServiceKey::typed::<T>()])
    }

    #[allow(non_snake_case, reason = "Named overload for Laravel's when")]
    fn whenNamed(
        &self,
        concrete: &str,
    ) -> Result<ContextualBindingBuilder<'_, Self>, BindingResolutionError> {
        self.whenMany([ServiceKey::named(concrete)])
    }

    #[allow(non_snake_case, reason = "Rust form of Laravel's array when")]
    fn whenMany(
        &self,
        concretes: impl IntoIterator<Item = impl Into<ServiceKey>>,
    ) -> Result<ContextualBindingBuilder<'_, Self>, BindingResolutionError> {
        let consumers = concretes
            .into_iter()
            .map(|key| self.getAlias(key))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ContextualBindingBuilder::new(self, consumers))
    }

    /// Check whether the service type has a binding.
    fn bound<T>(&self) -> bool
    where
        T: ?Sized + Send + Sync + 'static;

    /// Resolve the binding for the requested service type.
    fn make<T>(&self) -> Result<Arc<T>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static;
}
