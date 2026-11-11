use std::{collections::HashMap, sync::Arc, time::Duration};

use aws_lc_rs::digest::{SHA256, digest};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq as _;

use crate::{
    contracts::{
        auth::{Authenticatable, Guard, StatefulGuard, UserProvider},
        session::Session as _,
    },
    cookie::{CookieOptions, facade::Cookie},
    http::Request,
    support::facades::{Facade as _, RateLimiter},
};

use super::{AuthError, AuthFuture, AuthIdentifier, AuthResult, Credentials};

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
        Self::mutable(request).guards.insert(
            name.to_owned(),
            GuardState {
                user: user.map(Arc::from),
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
        }
    }

    pub(super) fn configure(mut self, remember_minutes: i64, rehash: bool) -> Self {
        self.remember_minutes = remember_minutes;
        self.rehash = rehash;
        self
    }

    pub fn getName(&self) -> &str {
        &self.session_key
    }
    pub fn getRecallerName(&self) -> &str {
        &self.recaller
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
        let session = self.session()?;
        session.regenerate(true).await.map_err(AuthError::backend)?;
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
            AuthContext::set(self.request, &self.name, Some(user), false);
            return Ok(());
        }
        let user = self.recall().await?;
        let remembered = user.is_some();
        if let Some(user) = &user {
            self.update_session(&user.get_auth_identifier()).await?;
        }
        AuthContext::set(self.request, &self.name, user, remembered);
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
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        let started = tokio::time::Instant::now();
        if let Some(user) = self.provider.retrieve_by_credentials(credentials).await?
            && self
                .provider
                .validate_credentials(user.as_ref(), credentials)
                .await?
        {
            return Ok(Some(user));
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
        let Some(mut user) = self.validated(credentials).await? else {
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
            let payload = serde_json::to_string(&Recaller {
                id: id.clone(),
                token,
                password,
            })
            .map_err(AuthError::backend)?;
            let options = CookieOptions {
                secure: self.request.is_secure().then_some(true),
                ..Default::default()
            };
            Some(
                Cookie::make(&self.recaller, &payload, self.remember_minutes, &options)
                    .map_err(AuthError::backend)?,
            )
        } else {
            None
        };
        self.update_session(&id).await?;
        if let Some(cookie) = cookie {
            Cookie::queue(cookie).map_err(AuthError::backend)?;
        }
        AuthContext::set(self.request, &self.name, Some(user), false);
        Ok(())
    }

    /// Logs out, invalidates the application session and rotates its CSRF token.
    pub async fn logoutAndInvalidate(&mut self) -> AuthResult<()> {
        self.logout().await?;
        let session = self.session()?;
        session.invalidate().await.map_err(AuthError::backend)?;
        session.regenerateToken().map_err(AuthError::backend)
    }

    async fn end(&mut self) -> AuthResult<()> {
        self.resolve().await?;
        let id = self
            .state()
            .and_then(|state| state.user.as_ref())
            .filter(|user| {
                user.get_remember_token()
                    .is_some_and(|token| !token.is_empty())
            })
            .map(|user| user.get_auth_identifier());
        if let Some(id) = id
            && let Some(mut user) = self.provider.retrieve_by_id(&id).await?
        {
            self.provider
                .update_remember_token(user.as_mut(), &random_token()?)
                .await?;
        }
        let key = self.session_key.clone();
        self.session()?.forget(&[&key]);
        if self.request.cookie_is_protected(&self.recaller) {
            Cookie::expire(&self.recaller, None, None).map_err(AuthError::backend)?;
        }
        AuthContext::set(self.request, &self.name, None, false);
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
        Box::pin(async move { Ok(self.validated(credentials).await?.is_some()) })
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
            let Some(user) = self.validated(credentials).await? else {
                return Ok(false);
            };
            self.set_user(user);
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
            self.set_user(user);
            Ok(true)
        })
    }

    fn via_remember(&self) -> bool {
        self.state().is_some_and(|state| state.via_remember)
    }

    fn logout(&mut self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(self.end())
    }
}
