use crate::{
    Fortify, LoginRateLimiter, contracts,
    http::responses::invalid,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{
        Auth, Credentials,
        events::{Failed, Lockout},
    },
    contracts::{
        auth::StatefulGuard as _,
        events::{Dispatcher as _, Event as EventContract},
        session::Session as _,
    },
    database::Value,
    events::EventContext,
    http::{HttpError, Request, Response},
    support::{
        Str,
        facades::{Event, Facade as _, Lang},
    },
};
use std::sync::Arc;

pub struct AttemptToAuthenticate {
    fortify: Arc<Fortify>,
    limiter: Arc<LoginRateLimiter>,
}
pub struct EnsureLoginIsNotThrottled {
    fortify: Arc<Fortify>,
    limiter: Arc<LoginRateLimiter>,
}
pub struct CanonicalizeUsername {
    fortify: Arc<Fortify>,
}
pub struct PrepareAuthenticatedSession {
    limiter: Arc<LoginRateLimiter>,
}

pub(crate) async fn credentials(
    request: &mut Request,
    fortify: &Fortify,
) -> Result<Credentials, HttpError> {
    let username = fortify.username();
    let value = request
        .input(username.as_str())
        .await?
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let password = request
        .input(Some("password"))
        .await?
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    Ok(Credentials::new([(username, Value::Text(value))], password))
}
pub(crate) async fn emit<E: EventContract>(
    event: E,
    arguments: &[&(dyn std::any::Any + Send + Sync)],
) -> Result<(), HttpError> {
    let event = Arc::new(event);
    let mut context = EventContext::with_arguments(event.as_ref(), arguments);
    Event::dispatcher()
        .map_err(internal)?
        .dispatch(event.clone(), &mut context)
        .await
        .map_err(internal)?;
    Ok(())
}
impl AttemptToAuthenticate {
    pub fn new(fortify: Arc<Fortify>, limiter: Arc<LoginRateLimiter>) -> Self {
        Self { fortify, limiter }
    }
    pub async fn handle(&self, request: &mut Request) -> Result<(), HttpError> {
        let remember = request.boolean("remember").await?;
        let credentials = credentials(request, &self.fortify).await?;
        let success = if let Some(callback) = self.fortify.authenticate_callback() {
            match callback(request).await? {
                Some(user) => {
                    Auth::session_guard(request, self.fortify.guard().as_deref())?
                        .login(user, remember)
                        .await?;
                    true
                }
                None => {
                    emit(
                        Failed {
                            guard: self.fortify.guard().unwrap_or(
                                Resolver::<openrust::auth::AuthManager>::get_facade_root()
                                    .map_err(internal)?
                                    .get_default_driver(),
                            ),
                            user: None,
                        },
                        &[&credentials],
                    )
                    .await?;
                    false
                }
            }
        } else {
            Auth::session_guard(request, self.fortify.guard().as_deref())?
                .attempt(&credentials, remember)
                .await?
        };
        if success {
            return Ok(());
        }
        self.limiter.increment(request).await?;
        let message = Lang::get("auth.failed", &Default::default(), None).map_err(internal)?;
        Err(HttpError::Validation(Box::new(invalid(
            &self.fortify.username(),
            message,
        ))))
    }
}
impl EnsureLoginIsNotThrottled {
    pub fn new(fortify: Arc<Fortify>, limiter: Arc<LoginRateLimiter>) -> Self {
        Self { fortify, limiter }
    }
    pub async fn handle(&self, request: &mut Request) -> Result<Option<Response>, HttpError> {
        if !self.limiter.too_many_attempts(request).await? {
            return Ok(None);
        }
        emit(
            Lockout {
                guard: self.fortify.guard().unwrap_or(
                    Resolver::<openrust::auth::AuthManager>::get_facade_root()
                        .map_err(internal)?
                        .get_default_driver(),
                ),
                retry_after: self.limiter.available_in(request).await?,
            },
            &[request.parts()],
        )
        .await?;
        let response = Resolver::<dyn contracts::LockoutResponse>::get_facade_root()
            .map_err(internal)?
            .to_response(request)
            .await?;
        Ok(Some(response))
    }
}
impl CanonicalizeUsername {
    pub fn new(fortify: Arc<Fortify>) -> Self {
        Self { fortify }
    }
    pub async fn handle(&self, request: &mut Request) -> Result<(), HttpError> {
        let name = self.fortify.username();
        if let Some(value) = request
            .input(name.as_str())
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            request
                .merge(serde_json::json!({name:Str::lower(&value)}))
                .await?;
        }
        Ok(())
    }
}
impl PrepareAuthenticatedSession {
    pub fn new(limiter: Arc<LoginRateLimiter>) -> Self {
        Self { limiter }
    }
    pub async fn handle(&self, request: &mut Request) -> Result<(), HttpError> {
        if request.has_session() {
            request
                .session_mut()
                .map_err(internal)?
                .regenerate(false)
                .await
                .map_err(internal)?;
        }
        self.limiter.clear(request).await
    }
}

/// Confirms a password using the configured username or application callback.
pub struct ConfirmPassword {
    fortify: Arc<Fortify>,
}
impl ConfirmPassword {
    pub fn new(fortify: Arc<Fortify>) -> Self {
        Self { fortify }
    }
    pub async fn call(
        &self,
        guard: &mut dyn openrust::contracts::auth::StatefulGuard,
        user: &dyn openrust::contracts::auth::Authenticatable,
        password: Option<&str>,
    ) -> openrust::auth::AuthResult<bool> {
        if let Some(callback) = self.fortify.confirm_password_callback() {
            return callback(user, password).await;
        }
        // Native providers use a non-null password type. PHP's providers reject null.
        let Some(password) = password else {
            return Ok(false);
        };
        let username = self.fortify.username();
        let value = user.get_auth_attribute(&username)?.unwrap_or(Value::Null);
        guard
            .validate(&Credentials::new([(username, value)], password))
            .await
    }
}

mod two_factor;
pub use two_factor::{
    ConfirmTwoFactorAuthentication, DisableTwoFactorAuthentication, EnableTwoFactorAuthentication,
    GenerateNewRecoveryCodes,
};
pub(crate) use two_factor::{consume_recovery_code, replace_recovery_code};

mod redirect_if_two_factor_authenticatable;
pub use redirect_if_two_factor_authenticatable::RedirectIfTwoFactorAuthenticatable;

/// Persists remember-token rotation and emits PasswordReset after the application action.
pub struct CompletePasswordReset {
    fortify: Arc<Fortify>,
}
impl CompletePasswordReset {
    pub fn new(fortify: Arc<Fortify>) -> Self {
        Self { fortify }
    }
    pub async fn handle(
        &self,
        user: &mut dyn openrust::contracts::auth::CanResetPassword,
    ) -> Result<(), HttpError> {
        let model = self
            .fortify
            .password_reset_models
            .read()
            .expect("Fortify model registry lock")
            .get(&(user as &dyn std::any::Any).type_id())
            .cloned()
            .ok_or_else(|| {
                internal(std::io::Error::other(
                    "Password reset model has not been registered",
                ))
            })?;
        model.persist(user, Str::random(60)).await?;
        emit(
            openrust::auth::events::PasswordReset {
                user: user.get_auth_identifier(),
            },
            &[],
        )
        .await
    }
}

// 类型擦除仅提供模型能力；验证结果与事件顺序由业务动作负责。
pub(crate) async fn verify_email(
    user: &mut dyn openrust::contracts::auth::MustVerifyEmail,
) -> Result<(), HttpError> {
    if !user.has_verified_email() && user.mark_email_as_verified().await? {
        emit(
            openrust::auth::events::Verified {
                user: user.get_auth_identifier(),
            },
            &[],
        )
        .await?;
    }
    Ok(())
}
