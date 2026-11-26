use crate::FortifyResult;
use openrust::http::HttpFuture;

/// The replaceable Fortify two-factor authentication engine.
pub trait TwoFactorAuthenticationProvider: Send + Sync {
    fn generate_secret_key(&self, secret_length: usize) -> FortifyResult<String>;
    fn qr_code_url(&self, company_name: &str, company_email: &str, secret: &str) -> String;
    fn verify<'a>(&'a self, secret: &'a str, code: &'a str) -> HttpFuture<'a, FortifyResult<bool>>;
}

/// Object-safe response contract for application container replacements.
pub trait Response: Send + Sync {
    fn to_response<'a>(
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
    VerifyEmailViewResponse,
    VerifyEmailResponse,
    EmailVerificationNotificationSentResponse,
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
pub trait User: openrust::contracts::auth::Authenticatable {
    fn clone_user(&self) -> Box<dyn openrust::contracts::auth::Authenticatable>;
}
impl<T: openrust::contracts::auth::Authenticatable + Clone> User for T {
    fn clone_user(&self) -> Box<dyn openrust::contracts::auth::Authenticatable> {
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
    /// 用户套件事务前验证；已有Action须显式接入，不在锁内另取池连接。
    fn validate_for_transaction<'a>(
        &'a self,
        _user: &'a dyn openrust::contracts::auth::Authenticatable,
        _input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<(), openrust::http::HttpError>> {
        Box::pin(async {
            Err(openrust::http::HttpError::from(std::io::Error::other(
                "资料Action尚未实现用户套件的事务验证入口",
            )))
        })
    }
    /// 仅在提供的用户事务中保存；近期证明与凭据撤销由套件协调。
    fn update_in_transaction<'a>(
        &'a self,
        _user: &'a dyn openrust::contracts::auth::Authenticatable,
        _input: &'a openrust::http::RequestValue,
        _transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>> {
        Box::pin(async {
            Err(openrust::http::HttpError::from(std::io::Error::other(
                "资料Action尚未实现用户套件的事务保存入口",
            )))
        })
    }
}
/// Application action responsible for validating and persisting a password change.
pub trait UpdatesUserPasswords: Send + Sync {
    fn update<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>>;
    /// Allows application validation to resolve the real request's named guard.
    fn update_from_request<'a>(
        &'a self,
        _request: &'a mut openrust::http::Request,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>> {
        self.update(user, input)
    }
    /// 用户套件事务前验证；已有Action须显式接入，不在锁内另取池连接。
    fn validate_for_transaction<'a>(
        &'a self,
        _user: &'a dyn openrust::contracts::auth::Authenticatable,
        _input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<(), openrust::http::HttpError>> {
        Box::pin(async {
            Err(openrust::http::HttpError::from(std::io::Error::other(
                "密码Action尚未实现用户套件的事务验证入口",
            )))
        })
    }
    /// 仅在提供的用户事务中保存；近期证明与凭据撤销由套件协调。
    fn update_in_transaction<'a>(
        &'a self,
        _user: &'a dyn openrust::contracts::auth::Authenticatable,
        _input: &'a openrust::http::RequestValue,
        _transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<Box<dyn User>, openrust::http::HttpError>> {
        Box::pin(async {
            Err(openrust::http::HttpError::from(std::io::Error::other(
                "密码Action尚未实现用户套件的事务保存入口",
            )))
        })
    }
}

/// Application action validates and persists a password on the broker's actual user.
pub trait ResetsUserPasswords: Send + Sync {
    fn reset<'a>(
        &'a self,
        user: &'a mut dyn openrust::contracts::auth::CanResetPassword,
        input: &'a openrust::http::RequestValue,
    ) -> HttpFuture<'a, Result<(), openrust::http::HttpError>>;
}

/// Rust passes the broker outcome explicitly instead of container constructor parameters.
pub trait PasswordResponse: Send + Sync {
    fn to_response<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        status: openrust::auth::passwords::PasswordResetStatus,
    ) -> HttpFuture<'a, Result<openrust::http::Response, openrust::http::HttpError>>;
}
pub trait PasswordResetResponse: PasswordResponse {}
pub trait FailedPasswordResetResponse: PasswordResponse {}
pub trait SuccessfulPasswordResetLinkRequestResponse: PasswordResponse {}
pub trait FailedPasswordResetLinkRequestResponse: PasswordResponse {}
pub trait RequestPasswordResetLinkViewResponse: Response {}
pub trait ResetPasswordViewResponse: Response {}
