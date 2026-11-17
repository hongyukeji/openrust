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

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<ResponseFactory>()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's response macro API")]
impl Response {
    pub fn r#macro<Fut>(
        name: impl Into<String>,
        callback: impl Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
    ) -> Result<(), BindingResolutionError>
    where
        Fut: Future<Output = Result<HttpResponse, HttpError>> + Send + 'static,
    {
        Self::getFacadeRoot()?.r#macro(name, callback);
        Ok(())
    }

    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, ResponseMacro)>,
        replace: bool,
    ) -> Result<(), BindingResolutionError> {
        Self::getFacadeRoot()?.mixin(macros, replace);
        Ok(())
    }

    pub fn hasMacro(name: &str) -> Result<bool, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.hasMacro(name))
    }

    pub fn flushMacros() -> Result<(), BindingResolutionError> {
        Self::getFacadeRoot()?.flushMacros();
        Ok(())
    }

    pub async fn call(name: &str, arguments: Vec<Value>) -> Result<HttpResponse, HttpError> {
        Self::getFacadeRoot()
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?
            .call(name, arguments)
            .await
    }
}
