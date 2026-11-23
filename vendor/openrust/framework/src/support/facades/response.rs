use std::future::Future;

use serde_json::Value;

use super::Facade;
use crate::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    http::{HttpError, Response as HttpResponse, ResponseFactory, ResponseMacro},
};

/// Resolves the current application's response factory.
pub struct Response;

impl Facade for Response {
    type Service = ResponseFactory;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<ResponseFactory>()
    }
}

impl Response {
    pub fn r#macro<Fut>(
        name: impl Into<String>,
        callback: impl Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError>
    where
        Fut: Future<Output = Result<HttpResponse, HttpError>> + Send + 'static,
    {
        Self::get_facade_root()?.r#macro(name, callback);
        Ok(())
    }

    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, ResponseMacro)>,
        replace: bool,
    ) -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.mixin(macros, replace);
        Ok(())
    }

    pub fn has_macro(name: &str) -> Result<bool, BindingResolutionError> {
        Ok(Self::get_facade_root()?.has_macro(name))
    }

    pub fn flush_macros() -> Result<(), BindingResolutionError> {
        Self::get_facade_root()?.flush_macros();
        Ok(())
    }

    pub async fn call(name: &str, arguments: Vec<Value>) -> Result<HttpResponse, HttpError> {
        Self::get_facade_root()?.call(name, arguments).await
    }
}
