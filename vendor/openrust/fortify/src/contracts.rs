use crate::FortifyResult;
use openrust::http::HttpFuture;

/// The replaceable Fortify two-factor authentication engine.
#[allow(non_snake_case, reason = "Matches the Fortify provider contract")]
pub trait TwoFactorAuthenticationProvider: Send + Sync {
    fn generateSecretKey(&self, secretLength: usize) -> FortifyResult<String>;
    fn qrCodeUrl(&self, companyName: &str, companyEmail: &str, secret: &str) -> String;
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>>;
}

/// Object-safe response contract for application container replacements.
#[allow(non_snake_case, reason = "Matches Fortify's Responsable contract")]
pub trait Response: Send + Sync {
    fn toResponse<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
    ) -> HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>;
}
macro_rules! responses {
    ($($name:ident),+ $(,)?) => {$(pub trait $name: Response {})+};
}
responses!(
    LoginResponse,
    LogoutResponse,
    PasswordConfirmedResponse,
    FailedPasswordConfirmationResponse,
    LockoutResponse,
    LoginViewResponse,
    ConfirmPasswordViewResponse
);
