use std::{sync::Arc, time::SystemTime};

use crate::{
    container::ServiceKey,
    contracts::routing::UrlGenerator as _,
    http::{Request, request_context},
    routing::{RouteParameter, SignatureMode, UrlError, UrlGenerator, UrlResult},
};

use super::Facade;

/// Generates URLs using the current application's routes and request.
pub struct URL;

impl Facade for URL {
    type Service = UrlGenerator;

    fn getFacadeAccessor() -> ServiceKey {
        "url".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's URL facade")]
impl URL {
    pub(crate) fn service() -> UrlResult<Arc<UrlGenerator>> {
        Self::getFacadeRoot().map_err(|_| UrlError::Unavailable)
    }

    pub fn generator() -> UrlResult<UrlGenerator> {
        Ok(Self::service()?.with_uri(request_context::with_url_uri()))
    }

    pub fn forRequest(request: &Request) -> UrlResult<UrlGenerator> {
        Ok(Self::service()?.forRequest(request))
    }

    pub fn route(name: &str, parameters: &[RouteParameter], absolute: bool) -> UrlResult<String> {
        Self::generator()?.route(name, parameters, absolute)
    }

    pub fn to(
        path: &str,
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        Self::generator()?.to(path, parameters, secure)
    }

    pub fn current() -> UrlResult<String> {
        Self::generator()?.current()
    }

    pub fn previous(request: &Request, fallback: Option<&str>) -> UrlResult<String> {
        Self::forRequest(request)?.previous(fallback)
    }

    pub fn asset(path: &str, secure: Option<bool>) -> UrlResult<String> {
        Self::generator()?.asset(path, secure)
    }

    pub fn query(
        path: &str,
        query: &[RouteParameter],
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        Self::generator()?.query(path, query, parameters, secure)
    }

    pub fn signedRoute(
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<String> {
        Self::generator()?.signedRoute(name, parameters, expiration, mode)
    }

    pub fn temporarySignedRoute(
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String> {
        Self::generator()?.temporarySignedRoute(name, expiration, parameters, mode)
    }
    pub fn hasValidSignature(
        request: &Request,
        mode: SignatureMode,
        ignore_query: crate::routing::SignatureQueryFilter<'_>,
    ) -> UrlResult<bool> {
        Self::service()?.hasValidSignature(request, mode, ignore_query)
    }
}
