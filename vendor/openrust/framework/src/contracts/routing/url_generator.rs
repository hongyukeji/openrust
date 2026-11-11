#![allow(non_snake_case)]

use std::time::SystemTime;

use crate::{
    http::Request,
    routing::{RouteParameter, SignatureMode, SignatureQueryFilter, UrlResult},
};

/// Application-facing URL generation and signature validation.
pub trait UrlGenerator: Send + Sync {
    fn current(&self) -> UrlResult<String>;
    fn previous(&self, fallback: Option<&str>) -> UrlResult<String>;
    fn to(
        &self,
        path: &str,
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String>;
    fn route(&self, name: &str, parameters: &[RouteParameter], absolute: bool)
    -> UrlResult<String>;
    fn query(
        &self,
        path: &str,
        query: &[RouteParameter],
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String>;
    fn asset(&self, path: &str, secure: Option<bool>) -> UrlResult<String>;
    fn signedRoute(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<String>;
    fn temporarySignedRoute(
        &self,
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String>;
    fn hasValidSignature(
        &self,
        request: &Request,
        mode: SignatureMode,
        ignoreQuery: SignatureQueryFilter<'_>,
    ) -> UrlResult<bool>;
}
