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
    ConfirmPasswordViewResponse,
    RegisterResponse,
    RegisterViewResponse,
    ProfileInformationUpdatedResponse,
    PasswordUpdateResponse,
    TwoFactorLoginResponse,
    FailedTwoFactorLoginResponse,
    TwoFactorEnabledResponse,
    TwoFactorConfirmedResponse,
    TwoFactorDisabledResponse,
    RecoveryCodesGeneratedResponse,
    TwoFactorChallengeViewResponse
);

/// Replaceable challenge stage used by Fortify's default login pipeline.
pub trait RedirectsIfTwoFactorAuthenticatable: Send + Sync {
    fn handle<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
    ) -> HttpFuture<'a, Result<Option<openrust::http::Response>, openrust::http::HttpError>>;
}

/// Owned action result that can also provide immutable authentication event snapshots.
#[allow(
    non_snake_case,
    reason = "Matches the user naming used by Fortify actions"
)]
pub trait User: openrust::contracts::auth::Authenticatable {
    fn cloneUser(&self) -> Box<dyn openrust::contracts::auth::Authenticatable>;
}
impl<T: openrust::contracts::auth::Authenticatable + Clone> User for T {
    fn cloneUser(&self) -> Box<dyn openrust::contracts::auth::Authenticatable> {
        Box::new(self.clone())
    }
}
/// Application action responsible for validating input and persisting a new account.
pub trait CreatesNewUsers: Send + Sync {
    fn create<'a>(
        &'a self,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>>;
}
/// Application action responsible for validating and persisting profile changes.
pub trait UpdatesUserProfileInformation: Send + Sync {
    fn update<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>>;
}
/// Application action responsible for validating and persisting a password change.
pub trait UpdatesUserPasswords: Send + Sync {
    fn update<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>>;
}
