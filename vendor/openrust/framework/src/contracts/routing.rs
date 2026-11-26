mod url_generator;
pub use url_generator::UrlGenerator;

use std::sync::Arc;

use crate::{
    http::{HttpError, HttpFuture, Method, Request, Response},
    routing::{BindingContext, Route, RoutingError},
};

/// Handles a matched route after middleware has run.
pub trait Handler: Send + Sync {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>>;

    /// 返回已注册控制器的稳定动作标识。
    #[doc(hidden)]
    fn controller_action(&self) -> Option<&str> {
        None
    }

    /// 控制器声明的中间件在路由中间件之后进入同一管线。
    #[doc(hidden)]
    fn controller_middleware(&self) -> &[crate::routing::MiddlewareEntry] {
        &[]
    }

    /// Route dispatch hook; parameter-aware adapters resolve dependencies first.
    #[doc(hidden)]
    fn handle_precognitive<'a>(
        &'a self,
        _request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { Ok(crate::foundation::Precognition::success()) })
    }
}

impl<F> Handler for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> + Send + Sync,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        self(request)
    }
}

/// Registers handlers with HTTP methods and URI patterns.
pub trait Registrar {
    #[track_caller]
    fn get(&mut self, uri: &str, handler: Arc<dyn Handler>) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::GET, Method::HEAD], uri, handler)
    }

    #[track_caller]
    fn post(&mut self, uri: &str, handler: Arc<dyn Handler>) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::POST], uri, handler)
    }

    #[track_caller]
    fn put(&mut self, uri: &str, handler: Arc<dyn Handler>) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::PUT], uri, handler)
    }

    #[track_caller]
    fn delete(&mut self, uri: &str, handler: Arc<dyn Handler>) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::DELETE], uri, handler)
    }

    #[track_caller]
    fn patch(&mut self, uri: &str, handler: Arc<dyn Handler>) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::PATCH], uri, handler)
    }

    #[track_caller]
    fn options(
        &mut self,
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<&mut Route, RoutingError> {
        self.r#match(&[Method::OPTIONS], uri, handler)
    }

    #[track_caller]
    fn r#match(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<&mut Route, RoutingError> {
        let methods = {
            if methods.iter().any(|method| {
                method
                    .as_str()
                    .bytes()
                    .any(|byte| byte.is_ascii_lowercase())
            }) {
                std::borrow::Cow::Owned(
                    methods
                        .iter()
                        .map(|method| {
                            Method::from_bytes(method.as_str().to_ascii_uppercase().as_bytes())
                                .expect("有效的 HTTP 方法转换为大写后仍然有效")
                        })
                        .collect(),
                )
            } else {
                std::borrow::Cow::Borrowed(methods)
            }
        };
        self.add_route(&methods, uri, handler)
    }

    #[track_caller]
    fn add_route(
        &mut self,
        methods: &[Method],
        uri: &str,
        handler: Arc<dyn Handler>,
    ) -> Result<&mut Route, RoutingError>;
}

/// A model that can be looked up using a matched route parameter.
pub trait UrlRoutable: Send + Sync + 'static {
    fn get_route_key_name() -> &'static str
    where
        Self: Sized,
    {
        "id"
    }

    /// Projects the model's key for use in a named URL.
    fn get_route_key(
        &self,
    ) -> crate::routing::UrlResult<Option<crate::routing::UrlValue<'static>>> {
        Ok(None)
    }

    /// Reads the attribute selected by the route's custom binding field.
    fn route_url_attribute(
        &self,
        _field: &str,
    ) -> crate::routing::UrlResult<Option<crate::routing::UrlValue<'static>>> {
        Ok(None)
    }

    fn resolve_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>>
    where
        Self: Sized;

    fn resolve_child_route_binding(
        _binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>>
    where
        Self: Sized,
    {
        Box::pin(async {
            Err(HttpError::Internal {
                source: std::io::Error::other("model has no scoped route binding resolver").into(),
            })
        })
    }
}
