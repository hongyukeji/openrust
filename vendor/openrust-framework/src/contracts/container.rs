use std::{future::Future, sync::Arc};

use crate::container::{ContextualBindingBuilder, ResolutionEvent, ServiceKey, Tagged};

mod binding_resolution_error;

pub use binding_resolution_error::BindingResolutionError;

/// An attribute that participates in contextual parameter resolution.
pub trait ContextualAttribute: Send + Sync + 'static {
    fn resolve(
        &self,
        _container: &crate::container::Container,
        _parameter: crate::container::AttributeParameter,
    ) -> Result<crate::container::AttributeValue, BindingResolutionError> {
        Err(BindingResolutionError::AttributeHandlerMissing {
            attribute: ServiceKey::typed::<Self>(),
        })
    }

    /// Resolves an attribute in an asynchronous construction or method invocation.
    fn resolve_async<'a>(
        self: Arc<Self>,
        container: &'a crate::container::Container,
        parameter: crate::container::AttributeParameter,
    ) -> crate::container::CallFuture<'a, crate::container::AttributeValue> {
        Box::pin(async move { self.resolve(container, parameter) })
    }

    fn after(
        &self,
        _value: &crate::container::AttributeValue,
        _container: &crate::container::Container,
    ) -> Result<(), BindingResolutionError> {
        Ok(())
    }
}

/// A concrete class whose construction entry point is a static method.
pub trait SelfBuilding: Sized + Send + Sync + 'static {
    fn new_instance(
        args: crate::container::ResolvedArgs,
        container: &crate::container::Container,
    ) -> crate::container::FactoryFuture<'_, Self>;
}

/// Registers services by type and resolves them through explicit factories.
pub trait Container: Send + Sync + Sized {
    /// Register a factory that is invoked for each resolution.
    fn bind<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    /// Register a factory whose first successful result is shared.
    fn singleton<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    /// Register an existing shared instance.
    fn instance<T>(&self, service: Arc<T>) -> Result<Arc<T>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static;

    /// Register a factory under a named identity instead of a Rust type.
    fn bind_named<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn singleton_named<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn instance_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        name: &str,
        service: Arc<T>,
    ) -> Result<Arc<T>, BindingResolutionError>;

    /// Point an alias at another typed or named identity.
    fn alias(
        &self,
        r#abstract: impl Into<ServiceKey>,
        alias: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    fn is_alias(&self, name: impl Into<ServiceKey>) -> bool;

    fn get_alias(&self, name: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError>;

    /// Tests whether a typed or named entry is bound or registered as an alias.
    fn has(&self, id: impl Into<ServiceKey>) -> bool;

    fn add_contextual_services<T: ?Sized + Send + Sync + 'static>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        services: Vec<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    fn register_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<crate::container::ConstructorParam>,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(
                Arc<C>,
                crate::container::CallArguments,
                &'a Self,
            ) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    fn register_static_method<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        signature: Vec<crate::container::ConstructorParam>,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(
                crate::container::CallArguments,
                &'a Self,
            ) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    fn bind_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(Arc<C>, &'a Self) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    fn bind_static_method<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(ServiceKey, &'a Self) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    fn has_method_binding<C: ?Sized + 'static>(&self, name: &str) -> bool;
    fn call_method_binding<'a, C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
        receiver: Arc<C>,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a;
    fn call_static_method_binding<'a, C: ?Sized + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a;
    fn resolve_environment_using(&self, callback: Option<crate::container::EnvironmentResolver>);
    fn current_environment_is<'a>(
        &self,
        environments: impl Into<crate::container::EnvironmentQuery<'a>>,
    ) -> Result<crate::container::EnvironmentResult, BindingResolutionError>;

    /// Return an owned ordered snapshot of explicit factories and their declared shared flags.
    fn get_bindings(&self) -> crate::container::Bindings;
    /// Clear bindings, aliases, instances and resolution history, retaining extension rules.
    fn flush(&self);

    /// Return a lazy closure that resolves the current binding on every invocation.
    fn factory<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
    fn factory_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
    fn factory_async<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync;
    fn factory_async_named<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
        key: ServiceKey,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync;
    /// Return a reusable closure that calls the container with a retained parameter snapshot.
    fn wrap<'a, R, F, Fut>(
        &'a self,
        callback: crate::container::Callback<'a, F>,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Fn() -> crate::container::CallFuture<'a, R> + Send + Sync + 'a
    where
        R: Send + 'a,
        F: Fn(crate::container::CallArguments) -> Fut + Send + Sync + 'a,
        Fut: Future<Output = Result<R, BindingResolutionError>> + Send + 'a;
    fn call<'a, R: Send + 'a, Target: crate::container::Callable<'a, R> + 'a>(
        &'a self,
        target: Target,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Future<Output = Result<R, BindingResolutionError>> + Send + 'a;

    /// Registers immutable constructor metadata without marking the service bound.
    fn register_class(
        &self,
        descriptor: crate::container::ClassDescriptor,
    ) -> Result<(), BindingResolutionError>;
    /// Constructs directly, bypassing the target's binding, cache, extenders and resolution callbacks.
    fn build<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError>;
    fn build_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;

    fn get_last_parameter_override(&self) -> crate::container::ParameterOverrides;
    fn make_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError>;
    fn make_with_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey>,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError>;
    fn make_with_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;
    fn make_with_async_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;

    /// Resolves a typed or named entry and checks its Rust output type.
    fn get<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey>,
    ) -> Result<Arc<T>, BindingResolutionError>;

    fn is_shared(&self, id: impl Into<ServiceKey>) -> bool;
    fn resolved(&self, id: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError>;
    fn forget_instance(&self, id: impl Into<ServiceKey>);
    fn forget_instances(&self);

    fn bind_if<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn singleton_if<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn bind_if_named<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn singleton_if_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn forget_scoped_instances(&self);
    fn scoped<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    fn scoped_if<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    fn scoped_named<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    fn scoped_if_named<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn add_contextual_binding<T, F>(
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
        self.when_many([ServiceKey::typed::<T>()])
    }

    fn when_named(
        &self,
        concrete: &str,
    ) -> Result<ContextualBindingBuilder<'_, Self>, BindingResolutionError> {
        self.when_many([ServiceKey::named(concrete)])
    }

    fn when_many(
        &self,
        concretes: impl IntoIterator<Item = impl Into<ServiceKey>>,
    ) -> Result<ContextualBindingBuilder<'_, Self>, BindingResolutionError> {
        let consumers = concretes
            .into_iter()
            .map(|key| self.get_alias(key))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ContextualBindingBuilder::new(self, consumers))
    }

    /// Registers a listener and resolves an already bound service without invoking that listener.
    fn rebinding<T, F>(
        &self,
        r#abstract: impl Into<ServiceKey>,
        callback: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static;

    /// Updates an explicit Rust target through its setter when a service is rebound.
    fn refresh<T, Target, F>(
        &self,
        r#abstract: impl Into<ServiceKey>,
        target: Arc<Target>,
        method: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        Target: ?Sized + Send + Sync + 'static,
        F: Fn(&Target, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        self.rebinding(r#abstract, move |_, value| method(&target, value))
    }

    /// Decorates a cached value immediately, or appends a future construction decorator.
    fn extend<T, F>(
        &self,
        r#abstract: impl Into<ServiceKey>,
        closure: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn forget_extenders(
        &self,
        r#abstract: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    fn before_resolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    fn resolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    fn after_resolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    fn bind_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn bind_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn bind_if_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn bind_if_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn singleton_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn singleton_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn singleton_if_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn singleton_if_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn scoped_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn scoped_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn scoped_if_async<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn scoped_if_async_named<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    fn rebinding_async<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        callback: F,
    ) -> impl Future<Output = Result<Option<Arc<T>>, BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static;

    fn refresh_async<T, Target, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        target: Arc<Target>,
        setter: F,
    ) -> impl Future<Output = Result<Option<Arc<T>>, BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        Target: ?Sized + Send + Sync + 'static,
        F: Fn(&Target, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        self.rebinding_async(key, move |_, value| setter(target.as_ref(), value))
    }

    fn extend_async<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        closure: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    fn add_contextual_binding_async<T, F>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    /// Resolve a service after awaiting an asynchronous deferred provider, when present.
    fn get_async<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey> + Send,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send {
        async move { self.get(id) }
    }

    /// Resolve a type without blocking the executor while its provider initializes.
    fn make_async<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send {
        self.get_async(ServiceKey::typed::<T>())
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
