use openrust::{
    auth::AuthResult,
    contracts::{auth::Authenticatable, config::Repository},
    http::{HttpError, HttpFuture, Request, Response},
    pipeline::PipelinePipe,
};
use serde_json::Value;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

pub type LoginPipe = PipelinePipe<(), Response, HttpError, Request>;
pub type AuthenticateCallback = dyn for<'a> Fn(&'a mut Request) -> HttpFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>
    + Send
    + Sync;
pub type ConfirmPasswordCallback = dyn for<'a> Fn(&'a dyn Authenticatable, Option<&'a str>) -> HttpFuture<'a, AuthResult<bool>>
    + Send
    + Sync;
pub type LoginPipelineCallback =
    dyn Fn(&Request) -> Result<Vec<LoginPipe>, HttpError> + Send + Sync;

/// Application-owned Fortify configuration and replaceable callbacks.
pub struct Fortify {
    pub(crate) config: Arc<dyn Repository>,
    authenticate: RwLock<Option<Arc<AuthenticateCallback>>>,
    confirm_password: RwLock<Option<Arc<ConfirmPasswordCallback>>>,
    pipeline: RwLock<Option<Arc<LoginPipelineCallback>>>,
    login_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    confirm_password_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    encrypter: RwLock<Option<Arc<dyn openrust::contracts::encryption::Encrypter>>>,
    recovery_code_generator: RwLock<Option<Arc<crate::recovery_code::RecoveryCodeGenerator>>>,
    pub(crate) two_factor_models: RwLock<
        std::collections::HashMap<
            std::any::TypeId,
            Arc<dyn crate::two_factor_model::RegisteredModel>,
        >,
    >,
    two_factor_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    pub(crate) creator: RwLock<Option<Arc<dyn crate::contracts::CreatesNewUsers>>>,
    pub(crate) profile_updater:
        RwLock<Option<Arc<dyn crate::contracts::UpdatesUserProfileInformation>>>,
    pub(crate) password_updater: RwLock<Option<Arc<dyn crate::contracts::UpdatesUserPasswords>>>,
    register_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    pub(crate) email_verification_models: RwLock<
        std::collections::HashMap<
            std::any::TypeId,
            Arc<dyn crate::email_verification_model::RegisteredModel>,
        >,
    >,
    verify_email_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    pub(crate) password_resetter: RwLock<Option<Arc<dyn crate::contracts::ResetsUserPasswords>>>,
    pub(crate) password_reset_models: RwLock<
        std::collections::HashMap<
            std::any::TypeId,
            Arc<dyn crate::password_reset_model::RegisteredModel>,
        >,
    >,
    request_password_reset_link_view:
        RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    reset_password_view: RwLock<Option<Arc<crate::http::responses::SimpleViewResponse>>>,
    registers_routes: AtomicBool,
}
#[allow(
    non_snake_case,
    reason = "Matches Fortify's application configuration API"
)]
impl Fortify {
    pub const VERIFICATION_LINK_SENT: &str = "verification-link-sent";
    pub const PROFILE_INFORMATION_UPDATED: &str = "profile-information-updated";
    pub const PASSWORD_UPDATED: &str = "password-updated";
    pub const RECOVERY_CODES_GENERATED: &str = "recovery-codes-generated";
    pub const TWO_FACTOR_AUTHENTICATION_CONFIRMED: &str = "two-factor-authentication-confirmed";
    pub const TWO_FACTOR_AUTHENTICATION_DISABLED: &str = "two-factor-authentication-disabled";
    pub const TWO_FACTOR_AUTHENTICATION_ENABLED: &str = "two-factor-authentication-enabled";

    pub fn new(config: Arc<dyn Repository>) -> Self {
        Self {
            config,
            authenticate: RwLock::new(None),
            confirm_password: RwLock::new(None),
            pipeline: RwLock::new(None),
            login_view: RwLock::new(None),
            confirm_password_view: RwLock::new(None),
            encrypter: RwLock::new(None),
            recovery_code_generator: RwLock::new(None),
            two_factor_models: RwLock::new(Default::default()),
            two_factor_view: RwLock::new(None),
            creator: RwLock::new(None),
            profile_updater: RwLock::new(None),
            password_updater: RwLock::new(None),
            register_view: RwLock::new(None),
            email_verification_models: RwLock::new(Default::default()),
            verify_email_view: RwLock::new(None),
            password_resetter: RwLock::new(None),
            password_reset_models: RwLock::new(Default::default()),
            request_password_reset_link_view: RwLock::new(None),
            reset_password_view: RwLock::new(None),
            registers_routes: AtomicBool::new(true),
        }
    }
    /// Registers a compiled model implementing Fortify's explicit account trait.
    pub fn registerTwoFactorAuthenticatable<T>(&self)
    where
        T: crate::TwoFactorAuthenticatable + Clone,
    {
        self.two_factor_models
            .write()
            .expect("Fortify model registry lock")
            .insert(
                std::any::TypeId::of::<T>(),
                Arc::new(crate::two_factor_model::ModelAdapter::<T>::new()),
            );
    }
    pub fn twoFactorChallengeView(
        &self,
        view: impl Into<crate::http::responses::SimpleViewResponse>,
    ) {
        *self.two_factor_view.write().expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn two_factor_view(
        &self,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        self.two_factor_view
            .read()
            .expect("Fortify views lock")
            .clone()
    }
    pub fn createUsersUsing(&self, creator: Arc<dyn crate::contracts::CreatesNewUsers>) {
        *self.creator.write().expect("Fortify actions lock") = Some(creator);
    }
    pub fn updateUserProfileInformationUsing(
        &self,
        updater: Arc<dyn crate::contracts::UpdatesUserProfileInformation>,
    ) {
        *self.profile_updater.write().expect("Fortify actions lock") = Some(updater);
    }
    pub fn updateUserPasswordsUsing(
        &self,
        updater: Arc<dyn crate::contracts::UpdatesUserPasswords>,
    ) {
        *self.password_updater.write().expect("Fortify actions lock") = Some(updater);
    }
    pub fn registerView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self.register_view.write().expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn register_view(&self) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        self.register_view
            .read()
            .expect("Fortify views lock")
            .clone()
    }
    /// Registers the model whose primary key is authorized by signed verification links.
    pub fn registerEmailVerification<T>(&self)
    where
        T: openrust::database::eloquent::Model + openrust::contracts::auth::MustVerifyEmail + Clone,
    {
        self.email_verification_models
            .write()
            .expect("Fortify model registry lock")
            .insert(
                std::any::TypeId::of::<T>(),
                Arc::new(crate::email_verification_model::ModelAdapter::<T>::new()),
            );
    }
    pub fn verifyEmailView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self.verify_email_view.write().expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn verify_email_view(
        &self,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        self.verify_email_view
            .read()
            .expect("Fortify views lock")
            .clone()
    }
    pub fn resetUserPasswordsUsing(&self, action: Arc<dyn crate::contracts::ResetsUserPasswords>) {
        *self
            .password_resetter
            .write()
            .expect("Fortify actions lock") = Some(action);
    }
    pub fn registerPasswordReset<T>(&self)
    where
        T: openrust::database::eloquent::Model + openrust::contracts::auth::CanResetPassword,
    {
        self.password_reset_models
            .write()
            .expect("Fortify model registry lock")
            .insert(
                std::any::TypeId::of::<T>(),
                Arc::new(crate::password_reset_model::ModelAdapter::<T>::new()),
            );
    }
    pub fn requestPasswordResetLinkView(
        &self,
        view: impl Into<crate::http::responses::SimpleViewResponse>,
    ) {
        *self
            .request_password_reset_link_view
            .write()
            .expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub fn resetPasswordView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self
            .reset_password_view
            .write()
            .expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn request_password_reset_link_view(
        &self,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        self.request_password_reset_link_view
            .read()
            .expect("Fortify views lock")
            .clone()
    }
    pub(crate) fn reset_password_view(
        &self,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        self.reset_password_view
            .read()
            .expect("Fortify views lock")
            .clone()
    }
    pub fn username(&self) -> String {
        self.string("fortify.username", "email")
    }
    pub fn email(&self) -> String {
        self.string("fortify.email", "email")
    }
    pub fn guard(&self) -> Option<String> {
        self.config.get("fortify.guard").and_then(|v| {
            v.as_str()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
    }
    pub fn redirects(&self, name: &str, default: Option<&str>) -> String {
        self.config
            .get(&format!("fortify.redirects.{name}"))
            .filter(|v| !v.is_null())
            .map(|v| v.as_str().unwrap_or_default().to_owned())
            .or_else(|| default.map(str::to_owned))
            .unwrap_or_else(|| self.string("fortify.home", "/home"))
    }
    pub fn authenticateUsing(&self, callback: Arc<AuthenticateCallback>) {
        *self.authenticate.write().expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn confirmPasswordsUsing(&self, callback: Arc<ConfirmPasswordCallback>) {
        *self
            .confirm_password
            .write()
            .expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn authenticateThrough(&self, callback: Arc<LoginPipelineCallback>) {
        *self.pipeline.write().expect("Fortify callbacks lock") = Some(callback);
    }
    pub fn loginView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self.login_view.write().expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub fn confirmPasswordView(&self, view: impl Into<crate::http::responses::SimpleViewResponse>) {
        *self
            .confirm_password_view
            .write()
            .expect("Fortify views lock") = Some(Arc::new(view.into()));
    }
    pub(crate) fn view(
        &self,
        login: bool,
    ) -> Option<Arc<crate::http::responses::SimpleViewResponse>> {
        if login {
            self.login_view.read().expect("Fortify views lock").clone()
        } else {
            self.confirm_password_view
                .read()
                .expect("Fortify views lock")
                .clone()
        }
    }
    pub fn confirmsTwoFactorAuthentication(&self) -> Result<bool, openrust::config::ConfigError> {
        crate::Features::new(self.config.clone())
            .optionEnabled(crate::Features::twoFactorAuthentication(), "confirm")
    }
    pub fn encryptUsing(
        &self,
        encrypter: Option<Arc<dyn openrust::contracts::encryption::Encrypter>>,
    ) {
        *self.encrypter.write().expect("Fortify encrypter lock") = encrypter;
    }
    pub fn currentEncrypter(
        &self,
    ) -> crate::FortifyResult<Arc<dyn openrust::contracts::encryption::Encrypter>> {
        if let Some(encrypter) = self
            .encrypter
            .read()
            .expect("Fortify encrypter lock")
            .clone()
        {
            return Ok(encrypter);
        }
        use openrust::support::facades::Facade as _;
        Ok(crate::resolver::Resolver::<
            dyn openrust::contracts::encryption::Encrypter,
        >::getFacadeRoot()?)
    }
    pub fn generateRecoveryCodesUsing(
        &self,
        callback: Option<Arc<crate::recovery_code::RecoveryCodeGenerator>>,
    ) {
        *self
            .recovery_code_generator
            .write()
            .expect("Fortify recovery codes lock") = callback;
    }
    pub(crate) fn recovery_code_generator(
        &self,
    ) -> Option<Arc<crate::recovery_code::RecoveryCodeGenerator>> {
        self.recovery_code_generator
            .read()
            .expect("Fortify recovery codes lock")
            .clone()
    }
    pub fn ignoreRoutes(&self) {
        self.registers_routes.store(false, Ordering::Relaxed);
    }
    pub fn registersRoutes(&self) -> bool {
        self.registers_routes.load(Ordering::Relaxed)
    }
    pub(crate) fn authenticate_callback(&self) -> Option<Arc<AuthenticateCallback>> {
        self.authenticate
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn confirm_password_callback(&self) -> Option<Arc<ConfirmPasswordCallback>> {
        self.confirm_password
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn pipeline_callback(&self) -> Option<Arc<LoginPipelineCallback>> {
        self.pipeline
            .read()
            .expect("Fortify callbacks lock")
            .clone()
    }
    pub(crate) fn string(&self, key: &str, default: &str) -> String {
        self.config
            .get(key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    }
    pub(crate) fn truthy(&self, key: &str) -> bool {
        self.config.get(key).is_some_and(|value| match value {
            Value::Null => false,
            Value::Bool(v) => v,
            Value::String(v) => !v.is_empty() && v != "0",
            Value::Number(v) => v.as_f64() != Some(0.0),
            Value::Array(v) => !v.is_empty(),
            Value::Object(v) => !v.is_empty(),
        })
    }
}
