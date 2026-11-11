use crate::{
    http::{Request, Response},
    routing::{RedirectOptions, Redirector, RouteParameter, UrlResult},
};

use super::URL;

/// Creates redirect responses for the current application.
pub struct Redirect;

impl Redirect {
    pub fn redirector() -> UrlResult<Redirector> {
        Ok(Redirector::new(URL::generator()?))
    }

    pub fn to(path: &str, options: RedirectOptions) -> UrlResult<Response> {
        Self::redirector()?.to(path, options)
    }

    pub fn route(
        name: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        Self::redirector()?.route(name, parameters, options)
    }

    pub fn back(
        request: &Request,
        fallback: Option<&str>,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        Self::redirector()?.back(request, fallback, options)
    }

    pub fn guest(
        request: &mut Request,
        path: &str,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        Self::redirector()?.guest(request, path, options)
    }

    pub fn intended(
        request: &mut Request,
        fallback: &str,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        Self::redirector()?.intended(request, fallback, options)
    }
}
