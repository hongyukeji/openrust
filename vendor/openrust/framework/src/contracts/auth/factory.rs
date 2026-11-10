use crate::auth::AuthResult;

use super::Guard;

/// Resolves and selects named authentication guards for the current scope.
pub trait Factory: Send {
    fn guard(&mut self, name: Option<&str>) -> AuthResult<&mut dyn Guard>;
    fn should_use(&mut self, name: &str) -> AuthResult<()>;
}
