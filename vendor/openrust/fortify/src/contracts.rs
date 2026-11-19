use crate::FortifyResult;
use openrust::http::HttpFuture;

/// The replaceable Fortify two-factor authentication engine.
#[allow(non_snake_case, reason = "Matches the Fortify provider contract")]
pub trait TwoFactorAuthenticationProvider: Send + Sync {
    fn generateSecretKey(&self, secretLength: usize) -> FortifyResult<String>;
    fn qrCodeUrl(&self, companyName: &str, companyEmail: &str, secret: &str) -> String;
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>>;
}
