#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

use crate::{auth::AuthResult, http::Request};

use super::Guard;

/// Resolves named authentication guards for one HTTP request.
pub trait Factory: Send + Sync {
    fn guard<'a>(
        &self,
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<Box<dyn Guard + 'a>>;
    fn shouldUse(&self, request: &mut Request, name: &str) -> AuthResult<()>;
}
