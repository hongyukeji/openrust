use std::time::SystemTime;

use crate::{
    http::Request,
    routing::{RouteParameter, SignatureMode, SignatureQueryFilter, UrlResult},
};

/// Application-facing URL generation and signature validation.
pub trait UrlGenerator: Send + Sync {
    fn full(&self) -> UrlResult<String>;
    fn previous_path(&self, fallback: Option<&str>) -> UrlResult<String>;
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
    fn action(
        &self,
        action: &str,
        parameters: &[RouteParameter],
        absolute: bool,
    ) -> UrlResult<String>;
    fn query(
        &self,
        path: &str,
        query: &[RouteParameter],
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String>;
    fn asset(&self, path: &str, secure: Option<bool>) -> UrlResult<String>;
    fn signed_route(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<String>;
    fn temporary_signed_route(
        &self,
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String>;
    fn has_valid_signature(
        &self,
        request: &Request,
        mode: SignatureMode,
        ignore_query: SignatureQueryFilter<'_>,
    ) -> UrlResult<bool>;
}
