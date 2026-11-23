use std::time::SystemTime;

use crate::{
    http::{RedirectResponse, Request},
    routing::{RedirectOptions, Redirector, RouteParameter, UrlResult},
};

use super::URL;

/// Creates redirect responses for the current application.
pub struct Redirect;

impl Redirect {
    pub fn redirector() -> UrlResult<Redirector> {
        Ok(Redirector::new(URL::generator()?))
    }

    pub fn to(path: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        Self::redirector()?.to(path, options)
    }

    pub fn secure(path: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        Self::redirector()?.secure(path, options)
    }

    pub fn refresh(options: RedirectOptions) -> UrlResult<RedirectResponse> {
        Self::redirector()?.refresh(options)
    }

    pub fn away(target: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        Self::redirector()?.away(target, options)
    }

    pub fn signed_route(
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.signed_route(name, parameters, expiration, options)
    }

    pub fn temporary_signed_route(
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.temporary_signed_route(name, expiration, parameters, options)
    }

    pub fn route(
        name: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.route(name, parameters, options)
    }

    pub fn action(
        action: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.action(action, parameters, options)
    }

    pub fn back(
        request: &Request,
        fallback: Option<&str>,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.back(request, fallback, options)
    }

    pub fn guest(
        request: &mut Request,
        path: &str,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.guest(request, path, options)
    }

    pub fn intended(
        request: &mut Request,
        fallback: &str,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        Self::redirector()?.intended(request, fallback, options)
    }

    pub fn get_intended_url(request: &Request) -> UrlResult<Option<&str>> {
        Self::redirector()?.get_intended_url(request)
    }

    pub fn set_intended_url(request: &mut Request, target: &str) -> UrlResult<()> {
        Self::redirector()?.set_intended_url(request, target)
    }
}
