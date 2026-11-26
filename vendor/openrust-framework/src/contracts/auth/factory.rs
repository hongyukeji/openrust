use crate::{auth::AuthResult, http::Request};

use super::Guard;

/// Resolves named authentication guards for one HTTP request.
pub trait Factory: Send + Sync {
    fn guard<'a>(
        &self,
        request: &'a mut Request,
        name: Option<&str>,
    ) -> AuthResult<Box<dyn Guard + 'a>>;
    fn should_use(&self, request: &mut Request, name: &str) -> AuthResult<()>;
}
