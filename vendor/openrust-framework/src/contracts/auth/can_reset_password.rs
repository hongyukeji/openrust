use crate::auth::{AuthFuture, AuthResult};

use super::Authenticatable;

/// User capability required by the password broker.
pub trait CanResetPassword: Authenticatable {
    fn get_email_for_password_reset(&self) -> &str;
    fn send_password_reset_notification<'a>(
        &'a self,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>>;
}
