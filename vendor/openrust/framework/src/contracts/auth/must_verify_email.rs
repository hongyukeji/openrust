use crate::auth::{AuthFuture, AuthIdentifier, AuthResult};

use super::Authenticatable;

/// User capability for applications that require email verification.
pub trait MustVerifyEmail: Authenticatable {
    /// 验证链接的身份键；Eloquent模型应返回主键，其他实现默认沿用认证标识。
    fn get_email_verification_identifier(&self) -> AuthResult<AuthIdentifier> {
        Ok(self.get_auth_identifier())
    }
    fn has_verified_email(&self) -> bool;
    /// Returns true only when this exact email changes from unverified to verified.
    fn mark_email_as_verified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn mark_email_as_unverified(&mut self) -> AuthFuture<'_, AuthResult<bool>>;
    fn send_email_verification_notification(&self) -> AuthFuture<'_, AuthResult<()>>;
    fn get_email_for_verification(&self) -> &str;
}
