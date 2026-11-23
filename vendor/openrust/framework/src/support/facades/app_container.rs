use super::{App, Facade};
use crate::{
    container::{
        Container, EnvironmentQuery, EnvironmentResult, ParameterOverrides, ResolutionEvent,
        ServiceKey,
    },
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::{Application, BootstrapError, configuration::ApplicationBuilder},
};
use std::{path::PathBuf, sync::Arc};

impl App {
    pub fn configure(base_path: PathBuf) -> ApplicationBuilder {
        Application::configure(base_path)
    }
    pub fn infer_base_path(source_root: Option<&str>) -> Result<PathBuf, BootstrapError> {
        Application::infer_base_path(source_root)
    }
    pub fn get_instance() -> Result<Arc<Application>, BindingResolutionError> {
        Self::get_facade_root()
    }
    pub fn get_container() -> Result<Arc<Container>, BindingResolutionError> {
        Ok(Self::get_facade_root()?.container_handle())
    }
    pub async fn make<T: ?Sized + Send + Sync + 'static>() -> Result<Arc<T>, BindingResolutionError>
    {
        Self::get_facade_root()?.make::<T>().await
    }
    pub async fn get<T: ?Sized + Send + Sync + 'static>(
        key: impl Into<ServiceKey> + Send,
    ) -> Result<Arc<T>, BindingResolutionError> {
        Self::get_facade_root()?.get::<T>(key).await
    }
    pub async fn make_with<T: ?Sized + Send + Sync + 'static>(
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        Self::get_facade_root()?.make_with::<T>(parameters).await
    }
    pub async fn make_with_named<T: ?Sized + Send + Sync + 'static>(
        key: impl Into<ServiceKey> + Send,
        parameters: ParameterOverrides,
    ) -> Result<Arc<T>, BindingResolutionError> {
        Self::get_facade_root()?
            .make_with_named::<T>(key, parameters)
            .await
    }
    pub fn base_path(path: &str) -> Result<PathBuf, BindingResolutionError> {
        Ok(crate::foundation::paths::join(
            Self::get_facade_root()?.base_path(),
            path,
        ))
    }
    pub fn public_path(path: &str) -> Result<PathBuf, BindingResolutionError> {
        Ok(crate::foundation::paths::join(
            &Self::get_facade_root()?.public_path(),
            path,
        ))
    }
    pub fn storage_path(path: &str) -> Result<PathBuf, BindingResolutionError> {
        Ok(Self::get_facade_root()?.storage_path(path))
    }
    pub fn use_storage_path(
        path: impl Into<PathBuf>,
    ) -> Result<Arc<Application>, BindingResolutionError> {
        let app = Self::get_facade_root()?;
        app.use_storage_path(path)?;
        Ok(app)
    }
    pub fn join_paths(base: &std::path::Path, path: &str) -> PathBuf {
        crate::foundation::paths::join(base, path)
    }
    pub fn environment(
        patterns: Option<EnvironmentQuery<'_>>,
    ) -> Result<EnvironmentResult, BindingResolutionError> {
        Self::get_facade_root()?.environment(patterns)
    }
    pub fn is_local() -> Result<bool, BindingResolutionError> {
        Ok(bool::from(Self::environment(Some(
            EnvironmentQuery::Name("local"),
        ))?))
    }
    pub fn is_production() -> Result<bool, BindingResolutionError> {
        Ok(bool::from(Self::environment(Some(
            EnvironmentQuery::Name("production"),
        ))?))
    }
    pub fn running_unit_tests() -> Result<bool, BindingResolutionError> {
        Self::get_facade_root()?.running_unit_tests()
    }
    pub fn is_booted() -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.is_booted())
    }
    pub fn configuration_is_cached() -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.configuration_is_cached())
    }
    pub fn routes_are_cached() -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.routes_are_cached())
    }
    pub fn provider_is_loaded(provider: &str) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.provider_is_loaded(provider))
    }
    pub fn is_deferred_service(key: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.is_deferred_service(key))
    }
    pub async fn load_deferred_providers() -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.load_deferred_providers().await
    }
    pub fn get_cached_routes_path() -> Result<Option<PathBuf>, BindingResolutionError> {
        Ok(Self::get_facade_root()?
            .get_cached_routes_path()
            .map(std::path::Path::to_path_buf))
    }
    pub fn get_cached_events_path() -> Result<Option<PathBuf>, BindingResolutionError> {
        Ok(Self::get_facade_root()?
            .get_cached_events_path()
            .map(std::path::Path::to_path_buf))
    }
    pub fn get_cached_packages_path() -> Result<Option<PathBuf>, BindingResolutionError> {
        Ok(Self::get_facade_root()?
            .get_cached_packages_path()
            .map(std::path::Path::to_path_buf))
    }
    pub fn bind<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.bind(factory)
    }
    pub fn bind_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.bind_named(name, factory)
    }
    pub fn singleton<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.singleton(factory)
    }
    pub fn singleton_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.singleton_named(name, factory)
    }
    pub fn scoped<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.scoped(factory)
    }
    pub fn scoped_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.scoped_named(name, factory)
    }
    pub fn bind_if<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.bind_if(factory)
    }
    pub fn bind_if_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.bind_if_named(name, factory)
    }
    pub fn singleton_if<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.singleton_if(factory)
    }
    pub fn singleton_if_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.singleton_if_named(name, factory)
    }
    pub fn scoped_if<T, F>(factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.scoped_if(factory)
    }
    pub fn scoped_if_named<T, F>(name: &str, factory: F) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.scoped_if_named(name, factory)
    }
    pub fn has(key: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_container()?.has(key))
    }
    pub fn is_shared(key: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_container()?.is_shared(key))
    }
    pub fn is_alias(key: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_container()?.is_alias(key))
    }
    pub fn bound<T: ?Sized + Send + Sync + 'static>() -> Result<bool, BindingResolutionError> {
        Ok(Self::get_container()?.bound::<T>())
    }
    pub fn resolved(key: impl Into<ServiceKey>) -> Result<bool, BindingResolutionError> {
        Self::get_container()?.resolved(key)
    }
    pub fn alias(
        abstract_key: impl Into<ServiceKey>,
        alias: impl Into<ServiceKey>,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.alias(abstract_key, alias)
    }
    pub fn get_alias(key: impl Into<ServiceKey>) -> Result<ServiceKey, BindingResolutionError> {
        Self::get_container()?.get_alias(key)
    }
    pub fn get_bindings() -> Result<crate::container::Bindings, BindingResolutionError> {
        Ok(Self::get_container()?.get_bindings())
    }
    pub fn instance<T: ?Sized + Send + Sync + 'static>(
        value: Arc<T>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        Self::get_container()?.instance(value)
    }
    pub fn instance_named<T: ?Sized + Send + Sync + 'static>(
        name: &str,
        value: Arc<T>,
    ) -> Result<Arc<T>, BindingResolutionError> {
        Self::get_container()?.instance_named(name, value)
    }
    pub fn forget_instance(key: impl Into<ServiceKey>) -> Result<(), BindingResolutionError> {
        Self::get_container()?.forget_instance(key);
        Ok(())
    }
    pub fn forget_instances() -> Result<(), BindingResolutionError> {
        Self::get_container()?.forget_instances();
        Ok(())
    }
    pub fn forget_scoped_instances() -> Result<(), BindingResolutionError> {
        Self::get_container()?.forget_scoped_instances();
        Ok(())
    }
    pub fn forget_extenders(key: impl Into<ServiceKey>) -> Result<(), BindingResolutionError> {
        Self::get_container()?.forget_extenders(key)
    }
    pub fn extend<T, F>(
        key: impl Into<ServiceKey>,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, &Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.extend(key, callback)
    }
    pub fn rebinding<T, F>(
        key: impl Into<ServiceKey>,
        callback: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.rebinding(key, callback)
    }
    pub fn before_resolving(
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Container) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.before_resolving(selector, callback)
    }
    pub fn resolving(
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Container) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.resolving(selector, callback)
    }
    pub fn after_resolving(
        selector: Option<ServiceKey>,
        callback: impl Fn(&ResolutionEvent, &Container) -> Result<(), BindingResolutionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.after_resolving(selector, callback)
    }

    pub fn currently_resolving() -> Result<Option<ServiceKey>, BindingResolutionError> {
        Ok(Self::get_container()?.currently_resolving())
    }

    pub fn tag(
        services: impl IntoIterator<Item = impl Into<ServiceKey>>,
        tags: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.tag(services, tags);
        Ok(())
    }

    /// 标签序列借用容器；回调内仍按需解析，不能把借用带出应用访问。
    pub fn tagged<T: ?Sized + Send + Sync + 'static, R>(
        tag: &str,
        callback: impl FnOnce(crate::container::Tagged<'_, Container, T>) -> R,
    ) -> Result<R, BindingResolutionError> {
        let container = Self::get_container()?;
        Ok(callback(container.tagged(tag)))
    }

    pub fn when<T: ?Sized + 'static, R>(
        callback: impl FnOnce(
            crate::container::ContextualBindingBuilder<'_, Container>,
        ) -> Result<R, BindingResolutionError>,
    ) -> Result<R, BindingResolutionError> {
        let container = Self::get_container()?;
        callback(container.when::<T>()?)
    }

    pub fn when_named<R>(
        consumer: &str,
        callback: impl FnOnce(
            crate::container::ContextualBindingBuilder<'_, Container>,
        ) -> Result<R, BindingResolutionError>,
    ) -> Result<R, BindingResolutionError> {
        let container = Self::get_container()?;
        callback(container.when_named(consumer)?)
    }

    pub fn add_contextual_binding<T, F>(
        consumer: impl Into<ServiceKey>,
        dependency: impl Into<ServiceKey>,
        factory: F,
    ) -> Result<(), BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(&Container) -> Result<Arc<T>, BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.add_contextual_binding(consumer, dependency, factory)
    }

    pub fn refresh<T, Target, F>(
        key: impl Into<ServiceKey>,
        target: Arc<Target>,
        setter: F,
    ) -> Result<Option<Arc<T>>, BindingResolutionError>
    where
        T: ?Sized + Send + Sync + 'static,
        Target: ?Sized + Send + Sync + 'static,
        F: Fn(&Target, Arc<T>) -> Result<(), BindingResolutionError> + Send + Sync + 'static,
    {
        Self::get_container()?.refresh(key, target, setter)
    }

    pub fn factory<T: ?Sized + Send + Sync + 'static>() -> Result<
        impl Fn() -> crate::container::FactoryFuture<'static, T> + Send + Sync,
        BindingResolutionError,
    > {
        let app = Self::get_facade_root()?;
        Ok(move || {
            let app = app.clone();
            Box::pin(async move { app.make::<T>().await })
                as crate::container::FactoryFuture<'static, T>
        })
    }

    pub fn factory_named<T: ?Sized + Send + Sync + 'static>(
        key: ServiceKey,
    ) -> Result<
        impl Fn() -> crate::container::FactoryFuture<'static, T> + Send + Sync,
        BindingResolutionError,
    > {
        let app = Self::get_facade_root()?;
        Ok(move || {
            let app = app.clone();
            let key = key.clone();
            Box::pin(async move { app.get::<T>(key).await })
                as crate::container::FactoryFuture<'static, T>
        })
    }

    pub fn build<T: ?Sized + Send + Sync + 'static>() -> Result<Arc<T>, BindingResolutionError> {
        Self::get_container()?.build::<T>()
    }

    pub async fn call<R: Send + 'static, Target: for<'a> crate::container::Callable<'a, R>>(
        target: Target,
        parameters: ParameterOverrides,
    ) -> Result<R, BindingResolutionError> {
        Self::get_facade_root()?.call(target, parameters).await
    }

    /// 显式回调可借用调用方数据，不要求回调或结果具有静态生命周期。
    pub async fn call_callback<'p, R, F, Fut>(
        callback: crate::container::Callback<'p, F>,
        parameters: ParameterOverrides,
    ) -> Result<R, BindingResolutionError>
    where
        R: Send + 'p,
        F: FnOnce(crate::container::CallArguments) -> Fut + Send + 'p,
        Fut: std::future::Future<Output = Result<R, BindingResolutionError>> + Send + 'p,
    {
        Self::get_facade_root()?.call(callback, parameters).await
    }

    pub fn has_method_binding<C: ?Sized + 'static>(
        method: &str,
    ) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_container()?.has_method_binding::<C>(method))
    }

    pub fn bind_method<C: ?Sized + Send + Sync + 'static, R: Send + 'static, F>(
        method: &str,
        callback: F,
    ) -> Result<(), BindingResolutionError>
    where
        F: for<'a> Fn(Arc<C>, &'a Container) -> crate::container::CallFuture<'a, R>
            + Send
            + Sync
            + 'static,
    {
        Self::get_container()?.bind_method(method, callback)
    }

    pub async fn call_method_binding<C: ?Sized + Send + Sync + 'static, R: Send + 'static>(
        method: &str,
        receiver: Arc<C>,
    ) -> Result<R, BindingResolutionError> {
        Self::get_facade_root()?
            .call_method_binding::<C, R>(method, receiver)
            .await
    }

    pub fn resolve_environment_using(
        callback: Option<crate::container::EnvironmentResolver>,
    ) -> Result<(), BindingResolutionError> {
        Self::get_container()?.resolve_environment_using(callback);
        Ok(())
    }

    pub fn current_environment_is<'a>(
        patterns: impl Into<EnvironmentQuery<'a>>,
    ) -> Result<EnvironmentResult, BindingResolutionError> {
        Self::get_container()?.current_environment_is(patterns)
    }
}
