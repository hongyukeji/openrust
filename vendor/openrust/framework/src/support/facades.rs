//! Static entry points for application services, routes and commands.

mod app;
mod app_container;
pub use app::App;
mod config;
mod maintenance_mode;
pub use maintenance_mode::MaintenanceMode;
mod event;
mod facade;
pub use event::Event;
mod redirect;
mod request;
mod response;
mod response_api;
pub use request::{Request, RequestHandle};
mod url;
pub use crate::auth::{Auth, access::GateFacade as Gate, passwords::Password};
pub use crate::broadcasting::Broadcast;
pub use crate::bus::Bus;
pub use crate::cache::Cache;
pub use crate::concurrency::Concurrency;
pub use crate::console::scheduling::ScheduleFacade as Schedule;
pub use crate::cookie::facade::Cookie;
pub use crate::database::{Db, schema::Schema};
pub use crate::encryption::Crypt;
pub use crate::filesystem::{File, Storage};
pub use crate::foundation::vite::ViteFacade as Vite;
pub use crate::hashing::Hash;
pub use crate::http::client::Http;
pub use crate::image::ImageFacade as Image;
pub use crate::localization::Lang;
pub use crate::log::Log;
pub use crate::log::context::Context;
pub use crate::mail::Mail;
pub use crate::notifications::NotificationFacade as Notification;
pub use crate::notifications::sms::Sms;
pub use crate::pipeline::Pipeline;
pub use crate::process::Process;
pub use crate::queue::QueueFacade as Queue;
pub use crate::redis::Redis;
pub use crate::session::Session;
pub use crate::testing::ParallelTestingFacade as ParallelTesting;
pub use crate::validation::facade::Validator;
pub use config::Config;
pub use facade::Facade;
pub(crate) use facade::{
    ContextSnapshot, WeakContextSnapshot, application_scope, application_scope_sync,
    context_snapshot, current_container, database_routing, isolated_scope, scope,
};
pub use redirect::Redirect;
pub use response::Response;
pub use url::Url;

use std::{
    cell::RefCell,
    ffi::OsString,
    path::Path,
    sync::{Arc, Weak},
};

use http::Uri;

use crate::{
    config::{ConfigError, Value},
    console::{Application, ConsoleError, Output},
    contracts::config::Repository,
    foundation,
    routing::{IntoRouteHandler, Router, RoutingError},
};

/// Resolves the current application's named rate limit policies.
pub struct RateLimiter;

impl Facade for RateLimiter {
    type Service = crate::cache::RateLimiter;

    fn get_facade_accessor() -> crate::container::ServiceKey {
        "rateLimiter".into()
    }
}

impl RateLimiter {
    pub fn r#for<R>(
        name: impl Into<String>,
        callback: impl Fn(&crate::http::Request) -> R + Send + Sync + 'static,
    ) -> crate::cache::CacheResult<()>
    where
        R: Into<crate::cache::rate_limiting::LimiterResponse>,
    {
        Self::get_facade_root()?.r#for(name, callback);
        Ok(())
    }

    pub fn for_async(
        name: impl Into<String>,
        callback: impl for<'a> Fn(
            &'a mut crate::http::Request,
        ) -> crate::http::HttpFuture<
            'a,
            Result<crate::cache::rate_limiting::LimiterResponse, crate::http::HttpError>,
        > + Send
        + Sync
        + 'static,
    ) -> crate::cache::CacheResult<()> {
        Self::get_facade_root()?.for_async(name, callback);
        Ok(())
    }

    pub fn async_limiter(
        name: &str,
    ) -> crate::cache::CacheResult<Option<Arc<crate::cache::rate_limiting::AsyncLimiter>>> {
        Ok(Self::get_facade_root()?.async_limiter(name))
    }

    pub fn for_job<R>(
        name: impl Into<String>,
        callback: impl Fn(&crate::queue::Payload) -> R + Send + Sync + 'static,
    ) -> crate::cache::CacheResult<()>
    where
        R: Into<crate::cache::rate_limiting::JobLimiterResponse>,
    {
        Self::get_facade_root()?.for_job(name, callback);
        Ok(())
    }

    pub async fn attempt<T>(
        key: &str,
        max_attempts: u64,
        action: impl std::future::Future<Output = crate::cache::CacheResult<T>>,
        decay_seconds: u64,
    ) -> crate::cache::CacheResult<crate::cache::rate_limiting::Attempt<T>> {
        Self::get_facade_root()?
            .attempt(key, max_attempts, action, decay_seconds)
            .await
    }

    pub async fn hit(key: &str, decay_seconds: u64) -> crate::cache::CacheResult<u64> {
        Self::get_facade_root()?.hit(key, decay_seconds).await
    }

    pub async fn attempts(key: &str) -> crate::cache::CacheResult<u64> {
        Self::get_facade_root()?.attempts(key).await
    }

    pub async fn too_many_attempts(
        key: &str,
        max_attempts: u64,
    ) -> crate::cache::CacheResult<bool> {
        Self::get_facade_root()?
            .too_many_attempts(key, max_attempts)
            .await
    }

    pub async fn remaining(key: &str, max_attempts: u64) -> crate::cache::CacheResult<u64> {
        Self::get_facade_root()?.remaining(key, max_attempts).await
    }

    pub async fn available_in(key: &str) -> crate::cache::CacheResult<u64> {
        Self::get_facade_root()?.available_in(key).await
    }

    pub async fn clear(key: &str) -> crate::cache::CacheResult<()> {
        Self::get_facade_root()?.clear(key).await
    }

    pub fn limiter(
        name: &str,
    ) -> crate::cache::CacheResult<Option<Arc<crate::cache::rate_limiting::Limiter>>> {
        Ok(Self::get_facade_root()?.limiter(name))
    }
}

/// Resolves the view factory for the current application.
pub struct View;

impl Facade for View {
    type Service = crate::view::Factory;

    fn get_facade_accessor() -> crate::container::ServiceKey {
        crate::container::ServiceKey::typed::<Self::Service>()
    }
}

impl View {
    pub fn render(name: &str, data: &crate::view::ViewData) -> crate::view::ViewResult<String> {
        Self::get_facade_root()?.render(name, data)
    }
    pub fn render_inline(
        source: &str,
        data: &crate::view::ViewData,
    ) -> crate::view::ViewResult<String> {
        Self::get_facade_root()?.render_inline(source, data)
    }
    pub fn render_when(
        condition: bool,
        view: &str,
        data: &crate::view::ViewData,
    ) -> crate::view::ViewResult<String> {
        Self::get_facade_root()?.render_when(condition, view, data)
    }
    pub fn render_unless(
        condition: bool,
        view: &str,
        data: &crate::view::ViewData,
    ) -> crate::view::ViewResult<String> {
        Self::get_facade_root()?.render_unless(condition, view, data)
    }
    pub fn render_each(
        view: &str,
        data: impl IntoIterator<Item = (crate::view::ViewValue, crate::view::ViewValue)>,
        iterator: &str,
        empty: &str,
    ) -> crate::view::ViewResult<String> {
        Self::get_facade_root()?.render_each(view, data, iterator, empty)
    }
    pub fn get_shared() -> crate::view::ViewResult<crate::view::ViewData> {
        Ok(Self::get_facade_root()?.get_shared())
    }

    /// Resolves the application view factory through its public contract.
    pub fn contract() -> crate::view::ViewResult<Arc<dyn crate::contracts::view::Factory>> {
        let factory = Self::get_facade_root()?;
        Ok(factory)
    }

    pub fn make(
        name: impl Into<std::borrow::Cow<'static, str>>,
        data: crate::view::ViewData,
    ) -> crate::view::ViewResult<crate::view::View> {
        Self::get_facade_root()?.make(name, data)
    }

    pub fn first(
        names: &[&str],
        data: crate::view::ViewData,
    ) -> crate::view::ViewResult<crate::view::View> {
        Self::get_facade_root()?.first(names, data)
    }

    pub fn share(
        key: impl Into<String>,
        value: impl Into<crate::view::ViewValue>,
    ) -> crate::view::ViewResult<()> {
        Self::get_facade_root()?.share(key, value);
        Ok(())
    }

    pub fn composer(
        name: impl Into<String>,
        callback: impl Fn(&mut crate::view::View) -> crate::view::ViewResult<()> + Send + Sync + 'static,
    ) -> crate::view::ViewResult<()> {
        Self::get_facade_root()?.composer(name, callback);
        Ok(())
    }

    pub fn creator(
        name: impl Into<String>,
        callback: impl Fn(&mut crate::view::View) -> crate::view::ViewResult<()> + Send + Sync + 'static,
    ) -> crate::view::ViewResult<()> {
        Self::get_facade_root()?.creator(name, callback);
        Ok(())
    }

    pub fn exists(name: &str) -> crate::view::ViewResult<bool> {
        Ok(Self::get_facade_root()?.exists(name))
    }
}

tokio::task_local! {
    static ROUTER: RouteRegistration;
    static CONSOLE: RefCell<Application>;
}

struct RouteRegistration {
    router: RefCell<Router>,
    config: Arc<dyn Repository>,
}

/// Registers routes on the router bound by the current application.
pub struct Route;

impl Facade for Route {
    type Service = Router;

    fn get_facade_accessor() -> crate::container::ServiceKey {
        "router".into()
    }
}

impl Route {
    /// 向应用已有中间件组追加正式入口，保留该组的原生装配。
    pub fn push_middleware_to_group(
        group: impl Into<String>,
        middleware: crate::routing::MiddlewareEntry,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .push_middleware_to_group(group, middleware);
                Ok(())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// 读取路由器实际匹配的名称，不读取可覆写的请求解析器；处理器须先使用 with_container。
    pub fn current_route_name()
    -> Result<Option<String>, crate::contracts::container::BindingResolutionError> {
        App::get_facade_root()?;
        if crate::http::request_context::with_client_ips(|_| ()).is_none() {
            return Ok(None);
        }
        Request::get_facade_root()?.with(|request| request.route_name.clone())
    }

    /// 在当前应用的路由器上分发命名路由；请求借用保持显式以保证并发隔离。
    pub async fn respond_with_route(
        name: &str,
        request: &mut crate::http::Request,
    ) -> Result<crate::http::Response, crate::http::HttpError> {
        Self::get_facade_root()?
            .respond_with_route(name, request)
            .await
    }

    /// Registers an explicit model binding in the current application's router.
    pub fn model<T: crate::contracts::routing::UrlRoutable>(
        name: &str,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .model::<T>(name);
                Ok(())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Registers a custom binding in the current application's router.
    pub fn bind<T, F>(name: &str, resolve: F) -> Result<(), RoutingError>
    where
        T: Send + Sync + 'static,
        F: for<'a> Fn(
                crate::routing::BindingContext<'a>,
            )
                -> crate::http::HttpFuture<'a, Result<Option<T>, crate::http::HttpError>>
            + Send
            + Sync
            + 'static,
    {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .bind(name, resolve);
                Ok(())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Mounts files from a directory as a GET and HEAD fallback.
    pub fn r#static(prefix: &str, directory: impl AsRef<Path>) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .borrow_mut()
                    .mount_static(prefix, directory.as_ref())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    /// Mounts an explicitly public local disk using its configured root and URL.
    pub fn static_disk(disk: &str) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                let config = scope.config.as_ref();
                let key = format!("filesystems.disks.{disk}");
                for (field, expected) in [("driver", "local"), ("visibility", "public")] {
                    let key = format!("{key}.{field}");
                    if config_string(config, &key)?.as_deref() != Some(expected) {
                        return Err(static_config_error(&key, expected));
                    }
                }
                let root_key = format!("{key}.root");
                let root = config_string(config, &root_key)?
                    .ok_or_else(|| static_config_error(&root_key, "a directory path"))?;
                let prefix = disk_url_prefix(config, &format!("{key}.url"))?;
                let permissions_key = format!("{key}.permissions");
                let permissions =
                    crate::filesystem::LocalPermissions::from_config(config.get(&permissions_key))
                        .map_err(|_| {
                            static_config_error(&permissions_key, "valid local permission modes")
                        })?;
                scope
                    .router
                    .borrow_mut()
                    .mount_public_disk(&prefix, Path::new(&root), permissions)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn get<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(
            &[crate::http::Method::GET, crate::http::Method::HEAD],
            uri,
            handler,
        )
    }

    /// 在当前应用作用域注册 POST 路由。
    #[track_caller]
    pub fn post<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&[crate::http::Method::POST], uri, handler)
    }

    /// 在当前应用作用域注册 PUT 路由。
    #[track_caller]
    pub fn put<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&[crate::http::Method::PUT], uri, handler)
    }

    /// 在当前应用作用域注册 PATCH 路由。
    #[track_caller]
    pub fn patch<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&[crate::http::Method::PATCH], uri, handler)
    }

    /// 在当前应用作用域注册 DELETE 路由。
    #[track_caller]
    pub fn delete<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&[crate::http::Method::DELETE], uri, handler)
    }

    /// 在当前应用作用域注册 OPTIONS 路由。
    #[track_caller]
    pub fn options<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&[crate::http::Method::OPTIONS], uri, handler)
    }

    /// 在当前应用作用域注册 QUERY 路由。
    #[track_caller]
    pub fn query<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(
            &[crate::http::Method::from_bytes(b"QUERY").expect("有效的 QUERY 方法")],
            uri,
            handler,
        )
    }

    /// 在当前应用作用域注册全部受支持的 HTTP 方法。
    #[track_caller]
    pub fn any<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::r#match(&Router::all_methods(), uri, handler)
    }

    /// 在普通路由均未匹配时执行 GET/HEAD 后备路由。
    #[track_caller]
    pub fn fallback<Arguments>(
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::fallback_with(handler, |_| Ok(()))
    }

    /// 在登记作用域内配置后备路由，避免返回超出借用范围的可变引用。
    #[track_caller]
    pub fn fallback_with<Arguments>(
        handler: impl IntoRouteHandler<Arguments>,
        configure: impl FnOnce(&mut crate::routing::Route) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        Self::register_configured(
            &[crate::http::Method::GET],
            "/{fallbackPlaceholder}",
            handler,
            |route| {
                route.r#where("fallbackPlaceholder", ".*")?.fallback();
                configure(route)
            },
        )
    }

    /// 通过当前应用视图工厂渲染 GET/HEAD 路由。
    #[track_caller]
    pub fn view(
        uri: &str,
        view: &str,
        options: crate::routing::ViewRouteOptions,
    ) -> Result<(), RoutingError> {
        Self::view_with(uri, view, options, |_| Ok(()))
    }

    #[track_caller]
    pub fn view_with(
        uri: &str,
        view: &str,
        options: crate::routing::ViewRouteOptions,
        configure: impl FnOnce(&mut crate::routing::Route) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        Self::get_with(
            uri,
            crate::routing::ViewController::new(view, options),
            configure,
        )
    }

    /// Configures a GET/HEAD route while its registration scope is active.
    #[track_caller]
    pub fn get_with<Arguments>(
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
        configure: impl FnOnce(&mut crate::routing::Route) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        Self::register_configured(
            &[crate::http::Method::GET, crate::http::Method::HEAD],
            uri,
            handler,
            configure,
        )
    }

    #[track_caller]
    pub fn redirect(
        uri: &str,
        target: &str,
        status: crate::http::StatusCode,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .redirect(uri, target, status)
                    .map(|_| ())
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn permanent_redirect(uri: &str, target: &str) -> Result<(), RoutingError> {
        Self::redirect(uri, target, crate::http::StatusCode::MOVED_PERMANENTLY)
    }

    pub fn group(
        group: crate::routing::RouteGroup,
        register: impl FnOnce(&mut Router) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .group(group, register)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn r#match<Arguments>(
        methods: &[crate::http::Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
    ) -> Result<(), RoutingError> {
        Self::register_configured(methods, uri, handler, |_| Ok(()))
    }

    /// 在当前登记作用域内配置任意方法路由，避免返回超出借用范围的引用。
    #[track_caller]
    pub fn match_with<Arguments>(
        methods: &[crate::http::Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
        configure: impl FnOnce(&mut crate::routing::Route) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        Self::register_configured(methods, uri, handler, configure)
    }

    #[track_caller]
    pub fn resources(
        resources: impl IntoIterator<Item = crate::routing::ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        Self::register_resources(resources, crate::routing::ResourceKind::Resource, false)
    }

    #[track_caller]
    pub fn api_resources(
        resources: impl IntoIterator<Item = crate::routing::ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        Self::register_resources(resources, crate::routing::ResourceKind::ApiResource, false)
    }

    #[track_caller]
    pub fn soft_deletable_resources(
        resources: impl IntoIterator<Item = crate::routing::ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        Self::register_resources(resources, crate::routing::ResourceKind::Resource, true)
    }

    #[track_caller]
    fn register_resources(
        resources: impl IntoIterator<Item = crate::routing::ResourceRegistration>,
        kind: crate::routing::ResourceKind,
        allow_trashed: bool,
    ) -> Result<(), RoutingError> {
        let definition = std::panic::Location::caller();
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .register_resources(resources, kind, allow_trashed, definition)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    pub fn resource<C: Send + Sync + 'static>(
        name: &str,
        controller: crate::routing::ResourceController<C>,
    ) -> Result<(), RoutingError> {
        Self::register_resource(name, controller, crate::routing::ResourceKind::Resource)
    }

    #[track_caller]
    pub fn api_resource<C: Send + Sync + 'static>(
        name: &str,
        controller: crate::routing::ResourceController<C>,
    ) -> Result<(), RoutingError> {
        Self::register_resource(name, controller, crate::routing::ResourceKind::ApiResource)
    }

    #[track_caller]
    pub fn singleton<C: Send + Sync + 'static>(
        name: &str,
        controller: crate::routing::ResourceController<C>,
    ) -> Result<(), RoutingError> {
        Self::register_resource(name, controller, crate::routing::ResourceKind::Singleton)
    }

    #[track_caller]
    pub fn api_singleton<C: Send + Sync + 'static>(
        name: &str,
        controller: crate::routing::ResourceController<C>,
    ) -> Result<(), RoutingError> {
        Self::register_resource(name, controller, crate::routing::ResourceKind::ApiSingleton)
    }

    #[track_caller]
    fn register_resource<C: Send + Sync + 'static>(
        name: &str,
        controller: crate::routing::ResourceController<C>,
        kind: crate::routing::ResourceKind,
    ) -> Result<(), RoutingError> {
        let definition = std::panic::Location::caller();
        ROUTER
            .try_with(|scope| {
                scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?
                    .register_resource(name, controller, kind, definition)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    #[track_caller]
    fn register_configured<Arguments>(
        methods: &[crate::http::Method],
        uri: &str,
        handler: impl IntoRouteHandler<Arguments>,
        configure: impl FnOnce(&mut crate::routing::Route) -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        let definition = std::panic::Location::caller();
        ROUTER
            .try_with(|scope| {
                let mut router = scope
                    .router
                    .try_borrow_mut()
                    .map_err(|_| RoutingError::RegistrationBorrowed)?;
                configure(router.register_with(methods, uri, handler, definition)?)
            })
            .map_err(|_| RoutingError::NoRegistrationScope)?
    }

    pub(crate) fn register(
        router: &mut Router,
        application: &foundation::Application,
        register: fn() -> Result<(), RoutingError>,
    ) -> Result<(), RoutingError> {
        let scope = RouteRegistration {
            router: RefCell::new(std::mem::take(router)),
            config: application
                .configuration_repository()
                .map_err(|source| RoutingError::ContainerFailed { source })?,
        };
        ROUTER.sync_scope(scope, || {
            let result = register();
            *router = ROUTER.with(|active| std::mem::take(&mut *active.router.borrow_mut()));
            result
        })
    }
}

fn config_string(config: &dyn Repository, key: &str) -> Result<Option<String>, RoutingError> {
    match config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Err(static_config_error(key, "a non-empty string")),
    }
}

fn disk_url_prefix(config: &dyn Repository, key: &str) -> Result<String, RoutingError> {
    let invalid_url = || static_config_error(key, "an absolute URL path or a same-origin HTTP URL");
    let url = config_string(config, key)?.ok_or_else(invalid_url)?;
    let uri = url.parse::<Uri>().map_err(|_| invalid_url())?;
    if url.contains('#') || uri.query().is_some() {
        return Err(invalid_url());
    }
    if url.starts_with('/') && !url.starts_with("//") {
        return Ok(uri.path().into());
    }
    let app_url = config_string(config, "app.url")?
        .and_then(|value| value.parse::<Uri>().ok())
        .ok_or_else(invalid_url)?;
    let default_port = match uri.scheme_str() {
        Some("http") => 80,
        Some("https") => 443,
        _ => return Err(invalid_url()),
    };
    let same_host = uri
        .host()
        .zip(app_url.host())
        .is_some_and(|(host, app_host)| host.eq_ignore_ascii_case(app_host));
    if !same_host
        || uri.scheme() != app_url.scheme()
        || uri.port_u16().unwrap_or(default_port) != app_url.port_u16().unwrap_or(default_port)
    {
        return Err(invalid_url());
    }
    Ok(uri.path().into())
}

fn static_config_error(key: &str, expected: &'static str) -> RoutingError {
    RoutingError::StaticMountConfiguration {
        source: ConfigError::InvalidType {
            key: key.into(),
            expected,
        },
    }
}

/// Registers and executes Artisan commands for the current application.
pub struct Artisan;

impl Artisan {
    /// Runs a registered command in-process. Each argument is a separate CLI token.
    pub async fn call(
        command: &str,
        arguments: &[OsString],
        output: &mut (impl crate::console::AsOutput + ?Sized),
    ) -> Result<i32, ConsoleError> {
        Box::pin(foundation::console::call_with_output(
            foundation::console::CommandTarget::Name(command),
            crate::console::command_arguments::CommandArguments::Tokens(arguments),
            output.as_output(),
            crate::console::CommandExecution::Application,
        ))
        .await
    }

    /// 通过已注册的具体命令类型调用同一 Artisan 实例。
    pub async fn call_type<C: crate::console::Command + 'static>(
        arguments: &[OsString],
        output: &mut (impl crate::console::AsOutput + ?Sized),
    ) -> Result<i32, ConsoleError> {
        Box::pin(foundation::console::call_with_output(
            foundation::console::CommandTarget::Type(
                std::any::TypeId::of::<C>(),
                std::any::type_name::<C>(),
            ),
            crate::console::command_arguments::CommandArguments::Tokens(arguments),
            output.as_output(),
            crate::console::CommandExecution::Application,
        ))
        .await
    }

    /// 由当前命令定义解析具名位置参数、数组参数及布尔开关。
    pub async fn call_named(
        command: &str,
        arguments: &[(&str, serde_json::Value)],
        output: &mut (impl crate::console::AsOutput + ?Sized),
    ) -> Result<i32, ConsoleError> {
        Box::pin(foundation::console::call_with_output(
            foundation::console::CommandTarget::Name(command),
            crate::console::command_arguments::CommandArguments::Named(arguments),
            output.as_output(),
            crate::console::CommandExecution::Application,
        ))
        .await
    }

    /// 解析完整命令字符串并在本应用内执行；不会调用 Shell。
    pub async fn call_line(
        command_line: &str,
        output: &mut (impl crate::console::AsOutput + ?Sized),
    ) -> Result<i32, ConsoleError> {
        let (command, arguments) = crate::console::command_arguments::line_arguments(command_line)?;
        Self::call(&command, &arguments, output).await
    }

    /// 执行命令并返回本次输出缓冲，不在应用间共享上一次输出。
    pub async fn call_captured(
        command: &str,
        arguments: &[OsString],
    ) -> Result<(i32, Output), ConsoleError> {
        let mut output = Output::default();
        let status = Self::call(command, arguments, &mut output).await?;
        Ok((status, output))
    }

    /// Runs a registered command and discards its console output.
    pub async fn call_silently(command: &str, arguments: &[OsString]) -> Result<i32, ConsoleError> {
        Box::pin(Self::call(command, arguments, &mut Output::default())).await
    }

    /// Prepares a registered Artisan command for dispatch through the application's queue.
    pub fn queue(
        command: &str,
        arguments: &[OsString],
    ) -> foundation::console::PendingQueuedCommand {
        foundation::console::PendingQueuedCommand::new(command, arguments)
    }

    /// 按注册定义编码具名参数，然后以现有命令队列格式派发。
    pub fn queue_named(
        command: &str,
        arguments: &[(&str, serde_json::Value)],
    ) -> foundation::console::PendingQueuedCommand {
        foundation::console::PendingQueuedCommand::named(command, arguments)
    }

    pub fn command(
        signature: &'static str,
        callback: impl Fn(&mut Output) + Send + Sync + 'static,
    ) -> Result<RegisteredCommand, ConsoleError> {
        Self::command_with(signature, callback)
    }

    /// 注册输入／异步闭包，保留原command的闭包类型推断。
    pub fn command_with<Arguments>(
        signature: &'static str,
        callback: impl crate::console::IntoCommandCallback<Arguments>,
    ) -> Result<RegisteredCommand, ConsoleError> {
        CONSOLE
            .try_with(|console| {
                let mut console = console.borrow_mut();
                let command = console.command_with(signature, callback)?;
                Ok(RegisteredCommand {
                    name: crate::console::Command::definition(command)
                        .name()
                        .to_owned(),
                    definition: command.registration_key(),
                })
            })
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }

    /// 返回当前注册作用域内应用的助手集合。
    pub fn get_helper_set() -> Result<crate::console::HelperSet, ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow().get_helper_set())
            .map_err(|_| ConsoleError::new("当前没有 Artisan 注册作用域"))
    }

    /// 设置此后注册的命令继承的集合。
    pub fn set_helper_set(helpers: crate::console::HelperSet) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow_mut().set_helper_set(helpers))
            .map_err(|_| ConsoleError::new("当前没有 Artisan 注册作用域"))
    }

    /// 配置已注册命令的助手集合，名称沿用命令注册表的解析规则。
    pub fn set_command_helper_set(
        name: &str,
        helpers: crate::console::HelperSet,
    ) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow_mut().set_command_helper_set(name, helpers))
            .map_err(|_| ConsoleError::new("当前没有 Artisan 注册作用域"))?
    }

    /// Adds a typed command to the console in the active registration scope.
    pub fn add(command: Box<dyn crate::console::Command>) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow_mut().add_command(command))
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }

    /// 注册隔离命令类型，保留同一应用装配作用域。
    pub fn add_isolatable(
        command: Box<dyn crate::contracts::console::Isolatable>,
    ) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| console.borrow_mut().add_isolatable_command(command))
            .map_err(|_| ConsoleError::new("当前没有Artisan注册作用域"))?
    }

    pub(crate) fn register(
        console: &mut Application,
        register: fn() -> Result<(), ConsoleError>,
    ) -> Result<(), ConsoleError> {
        CONSOLE.sync_scope(RefCell::new(std::mem::take(console)), || {
            let result = register();
            *console = CONSOLE.with(|active| std::mem::take(&mut *active.borrow_mut()));
            result
        })
    }
}

/// Configures the command just registered through the Artisan facade.
pub struct RegisteredCommand {
    name: String,
    definition: Weak<clap::Command>,
}

impl RegisteredCommand {
    pub fn purpose(self, description: &'static str) -> Result<(), ConsoleError> {
        CONSOLE
            .try_with(|console| {
                console
                    .borrow_mut()
                    .set_closure_purpose(&self.name, &self.definition, description)
            })
            .map_err(|_| ConsoleError::new("No active Artisan registration scope."))?
    }
}

pub use crate::view::Blade;

pub use crate::foundation::exceptions::Exceptions;

pub use crate::foundation::cloud::CloudFacade as Cloud;

pub use crate::support::date_factory::DateFacade as Date;
