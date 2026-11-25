use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
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
    resolutions: HashMap<String, Arc<AtomicBool>>,
    pub selected: Option<String>,
}

// 认证回调可以写日志；嵌套读取不得重新启动同一请求的同名认证。
pub(super) struct UserResolution(Arc<AtomicBool>);
impl Drop for UserResolution {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl AuthContext {
    pub(super) fn begin_resolution(request: &mut Request, name: &str) -> Option<UserResolution> {
        let flag = Self::mutable(request)
            .resolutions
            .entry(name.to_owned())
            .or_default()
            .clone();
        if flag.swap(true, Ordering::AcqRel) {
            None
        } else {
            Some(UserResolution(flag))
        }
    }

    pub(super) fn get(request: &Request) -> Option<&Self> {
        request.parts().extensions.get()
    }

    pub(super) fn mutable(request: &mut Request) -> &mut Self {
        request.parts_mut().extensions.get_or_insert_default()
    }

    pub(super) fn state<'a>(request: &'a Request, name: &str) -> Option<&'a GuardState> {
        Self::get(request)?.guards.get(name)
    }

    pub(super) fn publish(request: &Request) {
        if let Some(context) = Self::get(request) {
            request.attributes().insert(
                std::any::type_name::<Self>(),
                crate::container::ParameterValue::new(context.clone()),
            );
        }
    }

    pub(super) fn logged_user(
        attributes: &crate::http::RequestAttributes,
        default: &str,
    ) -> Option<Arc<dyn Authenticatable>> {
        let context = attributes
            .get(std::any::type_name::<Self>())?
            .downcast::<Self>()
            .ok()?;
        context.resolved_user(None, default)
    }

    pub(super) fn resolved_user(
        &self,
        name: Option<&str>,
        default: &str,
    ) -> Option<Arc<dyn Authenticatable>> {
        self.guards
            .get(name.or(self.selected.as_deref()).unwrap_or(default))?
            .user
            .clone()
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
        Self::publish(request);
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
    fingerprint_on_login: bool,
    owner: Weak<Container>,
    managed: bool,
}

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
            fingerprint_on_login: false,
            owner: Weak::new(),
            managed: false,
        }
    }

    pub(super) fn configure(
        mut self,
        remember_minutes: i64,
        rehash: bool,
        fingerprint_on_login: bool,
        owner: Weak<Container>,
    ) -> Self {
        self.remember_minutes = remember_minutes;
        self.rehash = rehash;
        self.fingerprint_on_login = fingerprint_on_login;
        self.owner = owner;
        self.managed = true;
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
            let dispatcher = owner.get::<Dispatcher>("events")?;
            let event = Arc::new(event);
            let mut context = EventContext::with_arguments(event.as_ref(), arguments);
            dispatcher.dispatch(event.clone(), &mut context).await?;
            Ok(())
        })
    }

    /// Installs a user for the current request and dispatches `Authenticated`.
    pub async fn set_user_async(&mut self, user: Box<dyn Authenticatable>) -> AuthResult<()> {
        let user: Arc<dyn Authenticatable> = user.into();
        self.authenticate(user).await
    }

    /// 安全变更提交后同步已认证的当前设备；不重新写remember token，也不派发Login。
    /// 调用者须先解析当前Guard，并提供变更前身份，不能借此恢复已失效会话。
    pub async fn synchronize_user(
        &mut self,
        previous: &AuthIdentifier,
        user: Box<dyn Authenticatable>,
    ) -> AuthResult<()> {
        let current = self
            .state()
            .and_then(|state| state.user.as_ref())
            .ok_or(AuthError::InvalidIdentity)?;
        if current.get_auth_identifier() != *previous
            || (current.as_ref() as &dyn std::any::Any).type_id()
                != (user.as_ref() as &dyn std::any::Any).type_id()
        {
            return Err(AuthError::InvalidIdentity);
        }
        let id = user.get_auth_identifier();
        self.update_session(&id).await?;
        self.store_login_fingerprint(user.as_ref())?;
        if (id != *previous || user.get_remember_token().is_none())
            && self.request.cookie_is_protected(&self.recaller)
        {
            Cookie::expire(&self.recaller, None, None)?;
        }
        AuthContext::set(self.request, &self.name, Some(user), false);
        Ok(())
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

    pub fn get_provider(&self) -> Arc<dyn UserProvider> {
        self.provider.clone()
    }

    /// 核对只读会话快照的真实Guard身份及登录指纹；未带指纹的旧会话不视为可信设备。
    pub fn session_belongs_to_user(
        config: &dyn Repository,
        guard: &str,
        user: &dyn Authenticatable,
        attributes: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> AuthResult<bool> {
        let identity = serde_json::to_value(user.get_auth_identifier())?;
        if attributes.get(&format!("login_{guard}")) != Some(&identity) {
            return Ok(false);
        }
        let expected = SessionFingerprint::for_guard(config, guard)?.sign_for_guard(user, guard)?;
        Ok(attributes
            .get(&SessionFingerprint::session_key(guard))
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<u8>>(value).ok())
            .is_some_and(|stored| bool::from(stored.ct_eq(&expected))))
    }

    pub fn get_name(&self) -> &str {
        &self.session_key
    }
    pub fn get_recaller_name(&self) -> &str {
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
    pub async fn once_basic(
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

    fn fingerprint_signer(&self) -> AuthResult<SessionFingerprint> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(AuthError::Configuration("认证会话所属应用"))?;
        let config = owner.get::<dyn Repository>("config")?;
        SessionFingerprint::for_guard(config.as_ref(), &self.name)
    }

    fn store_login_fingerprint(&mut self, user: &dyn Authenticatable) -> AuthResult<()> {
        if self.fingerprint_on_login {
            let owner = self
                .owner
                .upgrade()
                .ok_or(AuthError::Configuration("认证会话所属应用"))?;
            let config = owner.get::<dyn Repository>("config")?;
            let value = SessionFingerprint::for_guard(config.as_ref(), &self.name)?
                .sign_for_guard(user, &self.name)?;
            let key = SessionFingerprint::session_key(&self.name);
            // 登录或经有效remember凭据恢复时立即建立指纹，不能延至首次受保护请求。
            self.session()?.put(&key, serde_json::json!(value))?;
        }
        Ok(())
    }

    fn login_fingerprint_matches(&mut self, user: &dyn Authenticatable) -> AuthResult<bool> {
        if !self.fingerprint_on_login {
            return Ok(true);
        }
        let owner = self
            .owner
            .upgrade()
            .ok_or(AuthError::Configuration("认证会话所属应用"))?;
        let config = owner.get::<dyn Repository>("config")?;
        let expected = SessionFingerprint::for_guard(config.as_ref(), &self.name)?
            .sign_for_guard(user, &self.name)?;
        let key = SessionFingerprint::session_key(&self.name);
        Ok(self.session()?.get(&key).cloned().is_some_and(|stored| {
            serde_json::from_value::<Vec<u8>>(stored)
                .is_ok_and(|stored| bool::from(stored.ct_eq(&expected)))
        }))
    }

    async fn update_session(&mut self, id: &AuthIdentifier) -> AuthResult<()> {
        let value = serde_json::to_value(id)?;
        let key = self.session_key.clone();
        let password_key = SessionFingerprint::session_key(&self.name);
        let session = self.session()?;
        session.regenerate(true).await?;
        session.forget(&[&password_key, PASSWORD_CONFIRMED_AT]);
        session.put(&key, value).map_err(AuthError::backend)
    }

    async fn additional_session_is_valid(
        &mut self,
        user: &dyn Authenticatable,
    ) -> AuthResult<bool> {
        if !self.managed {
            return Ok(true);
        }
        let owner = self
            .owner
            .upgrade()
            .ok_or(AuthError::Configuration("认证会话所属应用"))?;
        owner
            .make::<super::AuthManager>()?
            .validate_session(&self.name, user, self.request.session()?)
            .await
    }

    async fn reject_session(&mut self, user: Box<dyn Authenticatable>) -> AuthResult<()> {
        self.clear_device()?;
        self.session()?.flush();
        AuthContext::set(self.request, &self.name, None, false);
        self.emit(
            CurrentDeviceLogout {
                guard: self.name.clone(),
                user: Some(user.into()),
            },
            &[],
        )
        .await?;
        Err(crate::http::HttpError::Unauthenticated.into())
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
            if !self.login_fingerprint_matches(user.as_ref())?
                || !self.additional_session_is_valid(user.as_ref()).await?
            {
                // 在Guard解析时拒绝已失效凭据，任何业务路由都不能绕过策略。
                return self.reject_session(user).await;
            }
            return self.set_user_async(user).await;
        }
        if let Some(user) = self.recall().await? {
            // 必须在remember恢复轮换ID前检查原会话，防止被撤销会话借恢复获得新ID。
            if !self.additional_session_is_valid(user.as_ref()).await? {
                return self.reject_session(user).await;
            }
            let user: Arc<dyn Authenticatable> = user.into();
            self.update_session(&user.get_auth_identifier()).await?;
            self.store_login_fingerprint(user.as_ref())?;
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
        if !self.request.has_session() {
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
    pub async fn attempt_throttled(
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
        let limiter = RateLimiter::get_facade_root()?;
        let key = format!("login:{}:{key}", self.name);
        if let Some(retry) = limiter.reserve(&key, maximum, decay_seconds).await? {
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
        limiter.clear(&key).await?;
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
        let signer = if self.fingerprint_on_login {
            Some(self.fingerprint_signer()?)
        } else {
            None
        };
        let expected_version = signer
            .as_ref()
            .map(|signer| signer.credential_version(user.as_ref()))
            .transpose()?
            .flatten();
        if self.rehash {
            self.provider
                .rehash_password_if_required(user.as_mut(), credentials, false)
                .await?;
            if remember || signer.is_some() {
                let Some(fresh) = self
                    .provider
                    .retrieve_by_id(&user.get_auth_identifier())
                    .await?
                else {
                    return Ok(None);
                };
                if signer.is_some()
                    && fresh.get_auth_password() != user.get_auth_password()
                    && !self
                        .provider
                        .validate_credentials(fresh.as_ref(), credentials)
                        .await?
                {
                    return Ok(None);
                }
                if let Some(signer) = signer
                    && signer.credential_version(fresh.as_ref())? != expected_version
                {
                    return Ok(None);
                }
                return Ok(Some(fresh));
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
        if self.fingerprint_on_login {
            let signer = self.fingerprint_signer()?;
            let fresh = self
                .provider
                .retrieve_by_id(&id)
                .await?
                .ok_or(crate::http::HttpError::Unauthenticated)?;
            if fresh.get_auth_identifier() != id
                || !bool::from(
                    signer
                        .sign_for_guard(user.as_ref(), &self.name)?
                        .ct_eq(&signer.sign_for_guard(fresh.as_ref(), &self.name)?),
                )
            {
                return Err(crate::http::HttpError::Unauthenticated.into());
            }
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
        self.store_login_fingerprint(user.as_ref())?;
        if let Some(cookie) = cookie {
            Cookie::queue(cookie)?;
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
        })?;
        let options = CookieOptions {
            secure: self.request.is_secure().then_some(true),
            ..Default::default()
        };
        Cookie::make(&self.recaller, &payload, self.remember_minutes, &options)
            .map_err(AuthError::backend)
    }

    /// Changes the password fingerprint while keeping the current session authenticated.
    pub async fn logout_other_devices(&mut self, password: &str) -> AuthResult<()> {
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
                || Cookie::has_queued(&self.recaller, None)?);
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
            let config = owner.get::<dyn Repository>("config")?;
            let value = SessionFingerprint::for_guard(config.as_ref(), &self.name)?
                .sign_for_guard(user.as_ref(), &self.name)?;
            let key = SessionFingerprint::session_key(&self.name);
            self.session()?.put(&key, serde_json::json!(value))?;
        }
        if refresh_cookie
            && let Some(token) = user.get_remember_token().filter(|token| !token.is_empty())
        {
            let cookie = self.remember_cookie(id, token.to_owned(), changed)?;
            Cookie::queue(cookie)?;
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
    pub async fn confirm_password(&mut self, password: &str) -> AuthResult<bool> {
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
        self.session()?.password_confirmed()?;
        Ok(true)
    }

    /// Logs out, invalidates the application session and rotates its CSRF token.
    pub async fn logout_and_invalidate(&mut self) -> AuthResult<()> {
        self.logout().await?;
        let session = self.session()?;
        session.invalidate().await?;
        session.regenerate_token().map_err(AuthError::backend)
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

    pub async fn logout_current_device(&mut self) -> AuthResult<()> {
        self.end(true).await
    }

    fn clear_device(&mut self) -> AuthResult<()> {
        let key = self.session_key.clone();
        let password_key = SessionFingerprint::session_key(&self.name);
        self.session()?
            .forget(&[&key, &password_key, PASSWORD_CONFIRMED_AT]);
        if self.request.cookie_is_protected(&self.recaller) {
            Cookie::expire(&self.recaller, None, None)?;
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
            let Some(_resolution) = AuthContext::begin_resolution(self.request, &self.name) else {
                return Ok(self.state().and_then(|state| state.user.as_deref()));
            };
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
            self.set_user_async(user).await?;
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
            self.set_user_async(user).await?;
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
