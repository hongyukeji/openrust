use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use aws_lc_rs::digest::{SHA256, digest};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq as _;

use crate::{
    container::Container,
    contracts::{
        auth::{Authenticatable, Guard, StatefulGuard, UserProvider},
        config::Repository,
        container::Container as _,
        events::{Dispatcher as _, Event},
        session::Session as _,
    },
    cookie::{CookieOptions, facade::Cookie},
    events::{Dispatcher, EventContext},
    http::Request,
    session::PASSWORD_CONFIRMED_AT,
    support::facades::{Facade as _, RateLimiter},
};

use super::{AuthError, AuthFuture, AuthIdentifier, AuthResult, Credentials};
use super::{
    access::Argument,
    events::{
        Attempting, Authenticated, CurrentDeviceLogout, Failed, Lockout, Login, Logout,
        OtherDeviceLogout, Validated,
    },
    session_fingerprint::SessionFingerprint,
};

#[derive(Clone, Default)]
pub(super) struct GuardState {
    pub user: Option<Arc<dyn Authenticatable>>,
    pub resolved: bool,
    pub via_remember: bool,
}

#[derive(Clone, Default)]
pub(super) struct AuthContext {
    guards: HashMap<String, GuardState>,
    pub selected: Option<String>,
}

impl AuthContext {
    pub(super) fn get(request: &Request) -> Option<&Self> {
        request.parts().extensions.get()
    }

    pub(super) fn mutable(request: &mut Request) -> &mut Self {
        request.parts_mut().extensions.get_or_insert_default()
    }

    pub(super) fn state<'a>(request: &'a Request, name: &str) -> Option<&'a GuardState> {
        Self::get(request)?.guards.get(name)
    }

    pub(super) fn set(
        request: &mut Request,
        name: &str,
        user: Option<Box<dyn Authenticatable>>,
        remembered: bool,
    ) {
        Self::set_shared(request, name, user.map(Arc::from), remembered);
    }

    pub(super) fn set_shared(
        request: &mut Request,
        name: &str,
        user: Option<Arc<dyn Authenticatable>>,
        remembered: bool,
    ) {
        Self::mutable(request).guards.insert(
            name.to_owned(),
            GuardState {
                user,
                resolved: true,
                via_remember: remembered,
            },
        );
    }
}

#[derive(Serialize, Deserialize)]
struct Recaller {
    id: AuthIdentifier,
    token: String,
    password: String,
}

/// A named session guard borrowing only the current HTTP request.
pub struct SessionGuard<'a> {
    request: &'a mut Request,
    provider: Arc<dyn UserProvider>,
    name: String,
    session_key: String,
    recaller: String,
    remember_minutes: i64,
    rehash: bool,
    owner: Weak<Container>,
}

#[allow(non_snake_case, reason = "Matches Laravel's SessionGuard API")]
impl<'a> SessionGuard<'a> {
    pub fn new(
        name: impl Into<String>,
        provider: Arc<dyn UserProvider>,
        request: &'a mut Request,
    ) -> Self {
        let name = name.into();
        Self {
            request,
            provider,
            session_key: format!("login_{name}"),
            recaller: format!("remember_{name}"),
            name,
            remember_minutes: 576_000,
            rehash: true,
            owner: Weak::new(),
        }
    }

    pub(super) fn configure(
        mut self,
        remember_minutes: i64,
        rehash: bool,
        owner: Weak<Container>,
    ) -> Self {
        self.remember_minutes = remember_minutes;
        self.rehash = rehash;
        self.owner = owner;
        self
    }

    fn emit<'event, E: Event>(
        &self,
        event: E,
        arguments: &'event [&'event Argument],
    ) -> AuthFuture<'event, AuthResult<()>> {
        let owner = self.owner.upgrade();
        Box::pin(async move {
            let Some(owner) = owner.filter(|owner| owner.has("events")) else {
                return Ok(());
            };
            let dispatcher = owner
                .get::<Dispatcher>("events")
                .map_err(AuthError::backend)?;
            let event = Arc::new(event);
            let mut context = EventContext::with_arguments(event.as_ref(), arguments);
            dispatcher
                .dispatch(event.clone(), &mut context)
                .await
                .map_err(AuthError::backend)?;
            Ok(())
        })
    }

    /// Installs a user for the current request and dispatches `Authenticated`.
    pub async fn setUser(&mut self, user: Box<dyn Authenticatable>) -> AuthResult<()> {
        let user: Arc<dyn Authenticatable> = user.into();
        self.authenticate(user).await
    }

    async fn authenticate(&mut self, user: Arc<dyn Authenticatable>) -> AuthResult<()> {
        AuthContext::set_shared(self.request, &self.name, Some(user.clone()), false);
        self.emit(
            Authenticated {
                guard: self.name.clone(),
                user,
            },
            &[],
        )
        .await
    }

    pub fn getName(&self) -> &str {
        &self.session_key
    }
    pub fn getRecallerName(&self) -> &str {
        &self.recaller
    }

    /// Authenticates an HTTP Basic request and persists a new login in the session.
    pub async fn basic(
        &mut self,
        field: &str,
        conditions: &[(String, crate::database::Value)],
    ) -> AuthResult<()> {
        if self.check().await? {
            return Ok(());
        }
        let credentials = super::basic::credentials(self.request, field, conditions)
            .ok_or_else(super::basic::challenge)?;
        if self.attempt(&credentials, false).await? {
            Ok(())
        } else {
            Err(super::basic::challenge())
        }
    }

    /// Authenticates an HTTP Basic request for the current request only.
    pub async fn onceBasic(
        &mut self,
        field: &str,
        conditions: &[(String, crate::database::Value)],
    ) -> AuthResult<()> {
        let credentials = super::basic::credentials(self.request, field, conditions)
            .ok_or_else(super::basic::challenge)?;
        if self.once(&credentials).await? {
            Ok(())
        } else {
            Err(super::basic::challenge())
        }
    }

    fn state(&self) -> Option<&GuardState> {
        AuthContext::state(self.request, &self.name)
    }

    fn session(&mut self) -> AuthResult<&mut crate::session::Store> {
        self.request.session_mut().map_err(AuthError::backend)
    }

    async fn update_session(&mut self, id: &AuthIdentifier) -> AuthResult<()> {
        let value = serde_json::to_value(id).map_err(AuthError::backend)?;
        let key = self.session_key.clone();
        let password_key = SessionFingerprint::session_key(&self.name);
        let session = self.session()?;
        session.regenerate(true).await.map_err(AuthError::backend)?;
        session.forget(&[&password_key, PASSWORD_CONFIRMED_AT]);
        session.put(&key, value).map_err(AuthError::backend)
    }

    async fn resolve(&mut self) -> AuthResult<()> {
        if self.state().is_some_and(|state| state.resolved) {
            return Ok(());
        }
        let id = self
            .request
            .session()
            .ok()
            .and_then(|session| session.get(&self.session_key))
            .and_then(|value| serde_json::from_value::<AuthIdentifier>(value.clone()).ok());
        if let Some(id) = id
            && let Some(user) = self.provider.retrieve_by_id(&id).await?
        {
            return self.setUser(user).await;
        }
        if let Some(user) = self.recall().await? {
            let user: Arc<dyn Authenticatable> = user.into();
            self.update_session(&user.get_auth_identifier()).await?;
            AuthContext::set_shared(self.request, &self.name, Some(user.clone()), true);
            return self
                .emit(
                    Login {
                        guard: self.name.clone(),
                        user,
                        remember: true,
                    },
                    &[],
                )
                .await;
        }
        AuthContext::set(self.request, &self.name, None, false);
        Ok(())
    }

    async fn recall(&mut self) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        if !self.request.hasSession() {
            return Ok(None);
        }
        let Some(value) = self.request.cookie(Some(&self.recaller))? else {
            return Ok(None);
        };
        if !self.request.cookie_is_protected(&self.recaller) {
            return Ok(None);
        }
        let Some(recaller) = value
            .as_str()
            .and_then(|raw| serde_json::from_str::<Recaller>(raw).ok())
        else {
            return Ok(None);
        };
        let user = self
            .provider
            .retrieve_by_token(&recaller.id, &recaller.token)
            .await?;
        Ok(user.filter(|user| {
            fingerprint(user.as_ref()).is_some_and(|current| {
                bool::from(current.as_bytes().ct_eq(recaller.password.as_bytes()))
            })
        }))
    }

    async fn validated(
        &mut self,
        credentials: &Credentials,
        attempting: Option<bool>,
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        let started = tokio::time::Instant::now();
        if let Some(remember) = attempting {
            self.emit(
                Attempting {
                    guard: self.name.clone(),
                    remember,
                },
                &[credentials],
            )
            .await?;
        }
        let candidate = self.provider.retrieve_by_credentials(credentials).await?;
        if let Some(user) = &candidate
            && self
                .provider
                .validate_credentials(user.as_ref(), credentials)
                .await?
        {
            self.emit(
                Validated {
                    guard: self.name.clone(),
                    user: user.get_auth_identifier(),
                },
                &[user.as_ref() as &Argument],
            )
            .await?;
            return Ok(candidate);
        }
        if attempting.is_some() {
            self.emit(
                Failed {
                    guard: self.name.clone(),
                    user: candidate.map(Arc::from),
                },
                &[credentials],
            )
            .await?;
        }
        tokio::time::sleep_until(started + Duration::from_millis(200)).await;
        Ok(None)
    }

    /// Atomically charges a login attempt before checking credentials.
    pub async fn attemptThrottled(
        &mut self,
        credentials: &Credentials,
        remember: bool,
        key: &str,
        maximum: u64,
        decay_seconds: u64,
    ) -> AuthResult<bool> {
        if key.is_empty() || maximum == 0 || decay_seconds == 0 {
            return Err(AuthError::Configuration("login throttle"));
        }
        let limiter = RateLimiter::getFacadeRoot().map_err(AuthError::backend)?;
        let key = format!("login:{}:{key}", self.name);
        if let Some(retry) = limiter
            .reserve(&key, maximum, decay_seconds)
            .await
            .map_err(AuthError::backend)?
        {
            self.emit(
                Lockout {
                    guard: self.name.clone(),
                    retry_after: retry,
                },
                &[self.request.parts()],
            )
            .await?;
            return Err(AuthError::Throttled(retry));
        }
        let Some(user) = self.login_candidate(credentials, remember).await? else {
            return Ok(false);
        };
        limiter.clear(&key).await.map_err(AuthError::backend)?;
        self.persist(user, remember).await?;
        Ok(true)
    }

    async fn login_candidate(
        &mut self,
        credentials: &Credentials,
        remember: bool,
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        self.session()?;
        if remember && !self.request.cookie_is_protected(&self.recaller) {
            return Err(AuthError::UnprotectedRememberCookie);
        }
        let Some(mut user) = self.validated(credentials, Some(remember)).await? else {
            return Ok(None);
        };
        if self.rehash {
            self.provider
                .rehash_password_if_required(user.as_mut(), credentials, false)
                .await?;
            if remember {
                return self
                    .provider
                    .retrieve_by_id(&user.get_auth_identifier())
                    .await;
            }
        }
        Ok(Some(user))
    }

    async fn persist(
        &mut self,
        mut user: Box<dyn Authenticatable>,
        remember: bool,
    ) -> AuthResult<()> {
        self.session()?;
        let id = user.get_auth_identifier();
        if matches!(&id, AuthIdentifier::String(id) if id.is_empty()) {
            return Err(AuthError::InvalidIdentity);
        }
        let cookie = if remember {
            if !self.request.cookie_is_protected(&self.recaller) {
                return Err(AuthError::UnprotectedRememberCookie);
            }
            let password = fingerprint(user.as_ref()).ok_or(AuthError::InvalidIdentity)?;
            let token = match user.get_remember_token().filter(|token| !token.is_empty()) {
                Some(token) => token.to_owned(),
                None => {
                    let token = random_token()?;
                    self.provider
                        .update_remember_token(user.as_mut(), &token)
                        .await?;
                    token
                }
            };
            Some(self.remember_cookie(id.clone(), token, password)?)
        } else {
            None
        };
        self.update_session(&id).await?;
        if let Some(cookie) = cookie {
            Cookie::queue(cookie).map_err(AuthError::backend)?;
        }
        let user: Arc<dyn Authenticatable> = user.into();
        self.emit(
            Login {
                guard: self.name.clone(),
                user: user.clone(),
                remember,
            },
            &[],
        )
        .await?;
        self.authenticate(user).await
    }

    fn remember_cookie(
        &self,
        id: AuthIdentifier,
        token: String,
        password: String,
    ) -> AuthResult<crate::cookie::Cookie> {
        let payload = serde_json::to_string(&Recaller {
            id,
            token,
            password,
        })
        .map_err(AuthError::backend)?;
        let options = CookieOptions {
            secure: self.request.is_secure().then_some(true),
            ..Default::default()
        };
        Cookie::make(&self.recaller, &payload, self.remember_minutes, &options)
            .map_err(AuthError::backend)
    }

    /// Changes the password fingerprint while keeping the current session authenticated.
    pub async fn logoutOtherDevices(&mut self, password: &str) -> AuthResult<()> {
        self.resolve().await?;
        let Some(state) = self.state() else {
            return Ok(());
        };
        let Some(user) = &state.user else {
            return Ok(());
        };
        let remembered = state.via_remember;
        let id = user.get_auth_identifier();
        self.session()?;
        let mut user = self
            .provider
            .retrieve_by_id(&id)
            .await?
            .ok_or(AuthError::InvalidIdentity)?;
        let credentials = Credentials::new(
            std::iter::empty::<(String, crate::database::Value)>(),
            password,
        );
        if !self
            .provider
            .validate_credentials(user.as_ref(), &credentials)
            .await?
        {
            return Err(AuthError::InvalidPassword);
        }
        let previous = fingerprint(user.as_ref()).ok_or(AuthError::InvalidIdentity)?;
        let refresh_cookie = self.request.cookie_is_protected(&self.recaller)
            && (self.request.cookie(Some(&self.recaller))?.is_some()
                || Cookie::hasQueued(&self.recaller, None).map_err(AuthError::backend)?);
        self.provider
            .rehash_password_if_required(user.as_mut(), &credentials, true)
            .await?;
        let user: Arc<dyn Authenticatable> = self
            .provider
            .retrieve_by_id(&id)
            .await?
            .ok_or(AuthError::InvalidIdentity)?
            .into();
        let changed = fingerprint(user.as_ref()).ok_or(AuthError::InvalidIdentity)?;
        if changed == previous {
            return Err(AuthError::Configuration(
                "provider did not update the password hash",
            ));
        }
        if let Some(owner) = self.owner.upgrade() {
            let config = owner
                .get::<dyn Repository>("config")
                .map_err(AuthError::backend)?;
            let value =
                SessionFingerprint::from_config(config.as_ref()).sign(user.get_auth_password());
            let key = SessionFingerprint::session_key(&self.name);
            self.session()?
                .put(&key, serde_json::json!(value))
                .map_err(AuthError::backend)?;
        }
        if refresh_cookie
            && let Some(token) = user.get_remember_token().filter(|token| !token.is_empty())
        {
            let cookie = self.remember_cookie(id, token.to_owned(), changed)?;
            Cookie::queue(cookie).map_err(AuthError::backend)?;
        }
        AuthContext::set_shared(self.request, &self.name, Some(user.clone()), remembered);
        self.emit(
            OtherDeviceLogout {
                guard: self.name.clone(),
                user,
            },
            &[],
        )
        .await
    }

    /// Verifies the current user's password and records a confirmation.
    pub async fn confirmPassword(&mut self, password: &str) -> AuthResult<bool> {
        let Some(id) = self.id().await? else {
            return Ok(false);
        };
        let Some(user) = self.provider.retrieve_by_id(&id).await? else {
            return Ok(false);
        };
        let credentials = Credentials::new(
            std::iter::empty::<(String, crate::database::Value)>(),
            password,
        );
        if !self
            .provider
            .validate_credentials(user.as_ref(), &credentials)
            .await?
        {
            return Ok(false);
        }
        self.session()?
            .passwordConfirmed()
            .map_err(AuthError::backend)?;
        Ok(true)
    }

    /// Logs out, invalidates the application session and rotates its CSRF token.
    pub async fn logoutAndInvalidate(&mut self) -> AuthResult<()> {
        self.logout().await?;
        let session = self.session()?;
        session.invalidate().await.map_err(AuthError::backend)?;
        session.regenerateToken().map_err(AuthError::backend)
    }

    async fn end(&mut self, current_device_only: bool) -> AuthResult<()> {
        self.resolve().await?;
        let user = self.state().and_then(|state| state.user.clone());
        let result = async {
            self.clear_device()?;
            if current_device_only {
                return self
                    .emit(
                        CurrentDeviceLogout {
                            guard: self.name.clone(),
                            user,
                        },
                        &[],
                    )
                    .await;
            }
            if let Some(user) = &user
                && user
                    .get_remember_token()
                    .is_some_and(|token| !token.is_empty())
                && let Some(mut current) = self
                    .provider
                    .retrieve_by_id(&user.get_auth_identifier())
                    .await?
            {
                self.provider
                    .update_remember_token(current.as_mut(), &random_token()?)
                    .await?;
            }
            self.emit(
                Logout {
                    guard: self.name.clone(),
                    user,
                },
                &[],
            )
            .await
        }
        .await;
        AuthContext::set(self.request, &self.name, None, false);
        result
    }

    pub async fn logoutCurrentDevice(&mut self) -> AuthResult<()> {
        self.end(true).await
    }

    fn clear_device(&mut self) -> AuthResult<()> {
        let key = self.session_key.clone();
        let password_key = SessionFingerprint::session_key(&self.name);
        self.session()?
            .forget(&[&key, &password_key, PASSWORD_CONFIRMED_AT]);
        if self.request.cookie_is_protected(&self.recaller) {
            Cookie::expire(&self.recaller, None, None).map_err(AuthError::backend)?;
        }
        Ok(())
    }
}

fn fingerprint(user: &dyn Authenticatable) -> Option<String> {
    Some(URL_SAFE_NO_PAD.encode(digest(&SHA256, user.get_auth_password()?.as_bytes())))
}

fn random_token() -> AuthResult<String> {
    let mut bytes = zeroize::Zeroizing::new([0_u8; 32]);
    aws_lc_rs::rand::fill(bytes.as_mut()).map_err(AuthError::backend)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes.as_ref()))
}

impl Guard for SessionGuard<'_> {
    fn user(&mut self) -> AuthFuture<'_, AuthResult<Option<&dyn Authenticatable>>> {
        Box::pin(async move {
            self.resolve().await?;
            Ok(self.state().and_then(|state| state.user.as_deref()))
        })
    }

    fn id(&mut self) -> AuthFuture<'_, AuthResult<Option<AuthIdentifier>>> {
        Box::pin(async move { Ok(self.user().await?.map(Authenticatable::get_auth_identifier)) })
    }

    fn check(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move { Ok(self.user().await?.is_some()) })
    }

    fn guest(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move { Ok(!self.check().await?) })
    }

    fn has_user(&self) -> bool {
        self.state().is_some_and(|state| state.user.is_some())
    }

    fn validate<'a>(
        &'a mut self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move { Ok(self.validated(credentials, None).await?.is_some()) })
    }

    fn set_user(&mut self, user: Box<dyn Authenticatable>) {
        AuthContext::set(self.request, &self.name, Some(user), false);
    }
}

impl StatefulGuard for SessionGuard<'_> {
    fn attempt<'a>(
        &'a mut self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some(user) = self.login_candidate(credentials, remember).await? else {
                return Ok(false);
            };
            self.persist(user, remember).await?;
            Ok(true)
        })
    }

    fn once<'a>(&'a mut self, credentials: &'a Credentials) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some(mut user) = self.validated(credentials, Some(false)).await? else {
                return Ok(false);
            };
            if self.rehash {
                self.provider
                    .rehash_password_if_required(user.as_mut(), credentials, false)
                    .await?;
            }
            self.setUser(user).await?;
            Ok(true)
        })
    }

    fn login(
        &mut self,
        user: Box<dyn Authenticatable>,
        remember: bool,
    ) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(self.persist(user, remember))
    }

    fn login_using_id<'a>(
        &'a mut self,
        id: &'a AuthIdentifier,
        remember: bool,
    ) -> AuthFuture<'a, AuthResult<Option<&'a dyn Authenticatable>>> {
        Box::pin(async move {
            let Some(user) = self.provider.retrieve_by_id(id).await? else {
                return Ok(None);
            };
            self.persist(user, remember).await?;
            self.user().await
        })
    }

    fn once_using_id<'a>(&'a mut self, id: &'a AuthIdentifier) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some(user) = self.provider.retrieve_by_id(id).await? else {
                return Ok(false);
            };
            self.setUser(user).await?;
            Ok(true)
        })
    }

    fn via_remember(&self) -> bool {
        self.state().is_some_and(|state| state.via_remember)
    }

    fn logout(&mut self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(self.end(false))
    }
}
