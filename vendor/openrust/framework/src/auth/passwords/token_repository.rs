#![allow(non_snake_case, reason = "Matches Laravel’s token repository API")]

use crate::{
    auth::{AuthFuture, AuthResult},
    contracts::auth::CanResetPassword,
};

/// A reserved token awaiting completion of the password reset callback.
pub struct TokenClaim {
    pub(super) hash: String,
}

impl TokenClaim {
    pub fn new(reservation: String) -> Self {
        Self { hash: reservation }
    }
    pub fn into_reservation(self) -> String {
        self.hash
    }
}

/// Storage and lifetime contract for password reset tokens.
pub trait TokenRepository: Send + Sync {
    fn create<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<String>>;
    /// Issues a token unless the user is within the repository's throttle window.
    fn issue<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
    ) -> AuthFuture<'a, AuthResult<Option<String>>>;
    /// Atomically reserves a valid token for a single callback.
    fn claim<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<TokenClaim>>>;
    /// Deletes a successfully consumed token or releases the reservation on callback failure.
    fn finish<'a>(
        &'a self,
        email: &'a str,
        claim: TokenClaim,
        success: bool,
    ) -> AuthFuture<'a, AuthResult<()>>;
    fn exists<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<bool>>;
    fn recentlyCreatedToken<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
    ) -> AuthFuture<'a, AuthResult<bool>>;
    fn delete<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<()>>;
    fn deleteExpired(&self) -> AuthFuture<'_, AuthResult<()>>;
}
