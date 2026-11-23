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

    fn get_facade_accessor() -> ServiceKey {
        "url".into()
    }
}

impl URL {
    pub(crate) fn service() -> UrlResult<Arc<UrlGenerator>> {
        Self::get_facade_root().map_err(|_| UrlError::Unavailable)
    }

    pub fn generator() -> UrlResult<UrlGenerator> {
        let mut urls = Self::service()?
            .with_uri(request_context::with_url_uri())
            .with_base_url(request_context::base_url().unwrap_or_default());
        request_context::with_attributes(|attributes| urls.merge_defaults(attributes));
        Ok(urls)
    }

    pub fn for_request(request: &Request) -> UrlResult<UrlGenerator> {
        Ok(Self::service()?.for_request(request))
    }

    pub fn route(name: &str, parameters: &[RouteParameter], absolute: bool) -> UrlResult<String> {
        Self::generator()?.route(name, parameters, absolute)
    }

    pub fn action(
        action: &str,
        parameters: &[RouteParameter],
        absolute: bool,
    ) -> UrlResult<String> {
        Self::generator()?.action(action, parameters, absolute)
    }

    pub fn to(
        path: &str,
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        Self::generator()?.to(path, parameters, secure)
    }

    pub fn defaults(request: &mut Request, defaults: &[(&str, &str)]) {
        UrlGenerator::set_defaults(request, defaults);
    }

    pub fn full() -> UrlResult<String> {
        Self::generator()?.full()
    }

    pub fn current() -> UrlResult<String> {
        Self::generator()?.current()
    }

    pub fn previous(request: &Request, fallback: Option<&str>) -> UrlResult<String> {
        Self::for_request(request)?.previous(fallback)
    }

    pub fn previous_path(request: &Request, fallback: Option<&str>) -> UrlResult<String> {
        Self::for_request(request)?.previous_path(fallback)
    }

    pub fn asset(path: &str, secure: Option<bool>) -> UrlResult<String> {
        Self::generator()?.asset(path, secure)
    }

    pub fn secure_asset(path: &str) -> UrlResult<String> {
        Self::generator()?.secure_asset(path)
    }

    pub fn asset_from(root: &str, path: &str, secure: Option<bool>) -> UrlResult<String> {
        Self::generator()?.asset_from(root, path, secure)
    }

    pub fn get_default_parameters() -> UrlResult<std::collections::HashMap<String, String>> {
        Ok(Self::generator()?.get_default_parameters().clone())
    }

    pub fn query(
        path: &str,
        query: &[RouteParameter],
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        Self::generator()?.query(path, query, parameters, secure)
    }

    pub fn signed_route(
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<String> {
        Self::generator()?.signed_route(name, parameters, expiration, mode)
    }

    pub fn temporary_signed_route(
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String> {
        Self::generator()?.temporary_signed_route(name, expiration, parameters, mode)
    }
    pub fn has_valid_signature(
        request: &Request,
        mode: SignatureMode,
        ignore_query: crate::routing::SignatureQueryFilter<'_>,
    ) -> UrlResult<bool> {
        Self::service()?.has_valid_signature(request, mode, ignore_query)
    }
}
