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
    #[allow(non_snake_case, reason = "Matches Laravel's self-building contract")]
    fn newInstance(
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
    #[allow(non_snake_case, reason = "Named overload for Rust's typed bind")]
    fn bindNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Named overload for Rust's typed singleton")]
    fn singletonNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Named overload for Rust's typed instance")]
    fn instanceNamed<T: ?Sized + Send + Sync + 'static>(
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

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn isAlias(&self, name: impl Into<ServiceKey>) -> bool;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn getAlias(&self, name: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError>;

    /// Tests whether a typed or named entry is bound or registered as an alias.
    fn has(&self, id: impl Into<ServiceKey>) -> bool;

    #[allow(non_snake_case, reason = "Typed Rust contextual array adapter")]
    fn addContextualServices<T: ?Sized + Send + Sync + 'static>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        services: Vec<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn registerMethod<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
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
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn registerStaticMethod<C: ?Sized + 'static, R: Send + 'static, F>(
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
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn bindMethod<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(Arc<C>, &'a Self) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn bindStaticMethod<C: ?Sized + 'static, R: Send + 'static, F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(ServiceKey, &'a Self) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static;
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn hasMethodBinding<C: ?Sized + 'static>(&self, name: &str) -> bool;
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn callMethodBinding<'a, C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
        receiver: Arc<C>,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a;
    #[allow(non_snake_case, reason = "Laravel method API and typed Rust overloads")]
    fn callStaticMethodBinding<'a, C: ?Sized + 'static, R: Send + 'static>(
        &'a self,
        name: &'a str,
    ) -> impl std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'a;
    #[allow(non_snake_case, reason = "Matches Laravel's container environment API")]
    fn resolveEnvironmentUsing(&self, callback: Option<crate::container::EnvironmentResolver>);
    #[allow(non_snake_case, reason = "Matches Laravel's container environment API")]
    fn currentEnvironmentIs<'a>(
        &self,
        environments: impl Into<crate::container::EnvironmentQuery<'a>>,
    ) -> Result<crate::container::EnvironmentResult, BindingResolutionError>;

    /// Return an owned ordered snapshot of explicit factories and their declared shared flags.
    #[allow(non_snake_case, reason = "Matches Laravel's binding directory API")]
    fn getBindings(&self) -> crate::container::Bindings;
    /// Clear bindings, aliases, instances and resolution history, retaining extension rules.
    fn flush(&self);

    /// Return a lazy closure that resolves the current binding on every invocation.
    fn factory<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
    #[allow(non_snake_case, reason = "Named Rust factory overload")]
    fn factoryNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> impl Fn() -> Result<Arc<T>, BindingResolutionError> + Send + Sync;
    #[allow(non_snake_case, reason = "Async Rust factory overload")]
    fn factoryAsync<'a, T: ?Sized + Send + Sync + 'static>(
        &'a self,
    ) -> impl Fn() -> crate::container::FactoryFuture<'a, T> + Send + Sync;
    #[allow(non_snake_case, reason = "Async named Rust factory overload")]
    fn factoryAsyncNamed<'a, T: ?Sized + Send + Sync + 'static>(
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
    #[allow(non_snake_case, reason = "Static Rust reflection adapter")]
    fn registerClass(
        &self,
        descriptor: crate::container::ClassDescriptor,
    ) -> Result<(), BindingResolutionError>;
    /// Constructs directly, bypassing the target's binding, cache, extenders and resolution callbacks.
    fn build<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, BindingResolutionError>;
    #[allow(non_snake_case, reason = "Async Rust build adapter")]
    fn buildAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;

    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    fn getLastParameterOverride(&self) -> crate::container::ParameterOverrides;
    #[allow(non_snake_case, reason = "Laravel parameter override API")]
    fn makeWith<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError>;
    #[allow(non_snake_case, reason = "Named Rust overload")]
    fn makeWithNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey>,
        parameters: crate::container::ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError>;
    #[allow(non_snake_case, reason = "Async Rust overload")]
    fn makeWithAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;
    #[allow(non_snake_case, reason = "Named async Rust overload")]
    fn makeWithAsyncNamed<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: impl Into<ServiceKey> + Send,
        parameters: crate::container::ParameterOverrides,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send;

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
    fn bindIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn bindIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn singletonIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetScopedInstances(&self);
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scoped<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIf<T, F>(&self, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's container API with explicit named overloads"
    )]
    fn scopedIfNamed<T, F>(&self, name: &str, factory: F) -> Result<(), BindingResolutionError>
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

    #[allow(non_snake_case, reason = "Matches Laravel's container API")]
    fn forgetExtenders(
        &self,
        r#abstract: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn beforeResolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn resolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Matches Laravel's container callbacks")]
    fn afterResolving(
        &self,
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Self) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError>;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn bindAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn bindAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn bindIfAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn bindIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn singletonAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn singletonAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn singletonIfAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn singletonIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn scopedAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn scopedAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn scopedIfAsync<T, F>(
        &self,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async factory adapter for Laravel binding")]
    fn scopedIfAsyncNamed<T, F>(
        &self,
        name: &str,
        factory: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async companion to Laravel rebinding")]
    fn rebindingAsync<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        callback: F,
    ) -> impl Future<Output = Result<Option<Arc<T>>, BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Self, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async companion to Laravel refresh")]
    fn refreshAsync<T, Target, F>(
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
        self.rebindingAsync(key, move |_, value| setter(target.as_ref(), value))
    }

    #[allow(non_snake_case, reason = "Async companion to Laravel extend")]
    fn extendAsync<T, F>(
        &self,
        key: impl Into<ServiceKey> + Send,
        closure: F,
    ) -> impl Future<Output = Result<(), BindingResolutionError>> + Send
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Self) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static;

    #[allow(non_snake_case, reason = "Async contextual factory adapter")]
    fn addContextualBindingAsync<T, F>(
        &self,
        consumer: impl Into<ServiceKey>,
        need: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: for<'a> Fn(&'a Self) -> crate::container::FactoryFuture<'a, T> + Send + Sync + 'static;

    /// Resolve a service after awaiting an asynchronous deferred provider, when present.
    #[allow(
        non_snake_case,
        reason = "Async companion to Laravel container resolution"
    )]
    fn getAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
        id: impl Into<ServiceKey> + Send,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send {
        async move { self.get(id) }
    }

    /// Resolve a type without blocking the executor while its provider initializes.
    #[allow(
        non_snake_case,
        reason = "Async companion to Laravel container resolution"
    )]
    fn makeAsync<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> impl Future<Output = Result<Arc<T>, BindingResolutionError>> + Send {
        self.getAsync(ServiceKey::typed::<T>())
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
