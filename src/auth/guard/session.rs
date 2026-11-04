//! 请求作用域的 Session 认证与记住我恢复。

use super::{AuthUser, Credentials, Guard, StatefulGuard, UserProvider};
use crate::{
    Error, Result,
    core::BoxFuture,
    events::Events,
    http::cookie::{CookieJar, CookieOptions},
    session::Session,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2_11::Sha256;
use std::{
    fmt,
    sync::{Arc, RwLock},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

macro_rules! user_event {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone)]
        pub struct $name {
            /// 认证 Guard 名称。
            pub guard: String,
            /// 当前账户；调试输出隐藏其内容。
            pub user: AuthUser,
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("guard", &self.guard)
                    .field("user", &"[REDACTED]")
                    .finish()
            }
        }
    };
}
user_event!(Validated, "凭据验证成功事件。");
user_event!(Authenticated, "请求内账户已确定事件。");
/// 开始尝试凭据登录。
#[derive(Clone, Debug)]
pub struct Attempting {
    /// Guard 名称。
    pub guard: String,
    /// 原始凭据，Debug 隐藏内容。
    pub credentials: Credentials,
    /// 是否请求记住我。
    pub remember: bool,
}
/// 登录成功事件。
#[derive(Clone)]
pub struct Login {
    /// Guard 名称。
    pub guard: String,
    /// 当前账户。
    pub user: AuthUser,
    /// 是否启用记住我。
    pub remember: bool,
}
/// 凭据登录失败事件。
#[derive(Clone)]
pub struct Failed {
    /// Guard 名称。
    pub guard: String,
    /// 已检索账户，可能不存在。
    pub user: Option<AuthUser>,
    /// 原始凭据。
    pub credentials: Credentials,
}
/// 注销事件。
#[derive(Clone)]
pub struct Logout {
    /// Guard 名称。
    pub guard: String,
    /// 注销前账户，访客为 None。
    pub user: Option<AuthUser>,
}
macro_rules! redacted_debug {
    ($($name:ident),+) => {$(impl fmt::Debug for $name {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct(stringify!($name)).field("guard", &self.guard).finish_non_exhaustive()
        }
    })+};
}
redacted_debug!(Login, Failed, Logout);

// 这些独立标志对应上游的注销、记住我来源与尝试状态，并补充持久存储证明；不能合并成互斥状态。
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct State {
    user: Option<AuthUser>,
    logged_out: bool,
    via_remember: bool,
    recall_attempted: bool,
    epoch: u64,
    persistent_user: bool,
}

/// 使用共享 Provider、请求 Session 与受信任解密 Cookie 的有状态 Guard。
///
/// 实例只能属于单个请求。传入的 recaller 必须来自框架 Cookie 解密层。
pub struct SessionGuard {
    name: String,
    provider: Arc<dyn UserProvider>,
    provider_name: Option<String>,
    session: Session,
    cookies: CookieJar,
    recaller: Option<String>,
    events: Option<Events>,
    hash_key: String,
    state: RwLock<State>,
    mutation: Mutex<()>,
    rehash_on_login: bool,
    timebox: Duration,
    remember_minutes: i64,
    remember_cookie_protected: bool,
}
// 事件是可重入的公开扩展点；派发期间不得持有操作串行许可。
struct Operation<'a> {
    permit: Option<tokio::sync::MutexGuard<'a, ()>>,
    epoch: Option<u64>,
}

// 只撤销本 Guard 可控制的请求/Session/Cookie 状态，不宣称数据库 token 或旧 Session 已回滚。
struct Transition<'a> {
    guard: &'a SessionGuard,
    committed: bool,
    epoch: u64,
}
impl<'a> Transition<'a> {
    fn new(guard: &'a SessionGuard, operation: &mut Operation<'a>) -> Result<Self> {
        let epoch = guard.start_mutation(operation)?;
        Ok(Self {
            guard,
            committed: false,
            epoch,
        })
    }
    fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for Transition<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Ok(mut state) = self.guard.state.write() else {
            return;
        };
        if state.epoch != self.epoch {
            return;
        }
        // 同步持有短状态锁直至局部撤销完成，防止后继操作与旧 Drop 交错。
        state.user = None;
        state.logged_out = true;
        let _removed = self.guard.session.remove(&self.guard.session_name());
        let _unmarked = self
            .guard
            .session
            .mark_guard_authentication(&self.guard.session_name(), false);
        let _hash_removed = self
            .guard
            .session
            .remove(&format!("password_hash_{}", self.guard.name));
        let _unqueued = self
            .guard
            .cookies
            .unqueue(&self.guard.recaller_name(), None);
        // 已有入站记住我 Cookie 时仍需使客户端删除它；失败不恢复任何认证状态。
        if self.guard.recaller.is_some()
            && let Ok(cookie) = self
                .guard
                .cookies
                .factory()
                .forget(self.guard.recaller_name(), CookieOptions::default())
        {
            let _expired = self.guard.cookies.queue(cookie);
        }
    }
}

// Provider 必须观察新 token；但 Rust future 取消不能把尚未确认持久化的值遗留给下次登录。
struct TokenTransition<'a> {
    guard: &'a SessionGuard,
    user: AuthUser,
    previous: Option<String>,
    candidate: String,
    epoch: u64,
    committed: bool,
}
impl TokenTransition<'_> {
    fn restore(&mut self) -> Result<()> {
        if self.committed {
            return Ok(());
        }
        let state = self.guard.read()?;
        if state.epoch == self.epoch {
            // 多个 Guard 可以共享同一账户；比较与恢复必须由账户的一次原子写完成。
            let _restored = self
                .user
                .compare_and_set_remember_token(Some(&self.candidate), self.previous.clone())?;
        }
        self.committed = true;
        Ok(())
    }
    fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for TokenTransition<'_> {
    fn drop(&mut self) {
        let _restored = self.restore();
    }
}

impl SessionGuard {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        name: String,
        provider: Arc<dyn UserProvider>,
        session: Session,
        cookies: CookieJar,
        recaller: Option<String>,
        events: Option<Events>,
        hash_key: Option<String>,
    ) -> Self {
        Self {
            name,
            provider,
            provider_name: None,
            session,
            cookies,
            recaller,
            events,
            hash_key: hash_key.unwrap_or_else(|| "base-key-for-password-hash-mac".into()),
            state: RwLock::new(State::default()),
            mutation: Mutex::new(()),
            rehash_on_login: true,
            timebox: Duration::from_millis(200),
            remember_minutes: 576_000,
            remember_cookie_protected: false,
        }
    }
    pub(crate) fn with_provider_name(mut self, name: Option<String>) -> Self {
        self.provider_name = name;
        self
    }
    pub(crate) fn with_decrypted_recaller(mut self, recaller: Option<String>) -> Self {
        self.recaller = recaller;
        self
    }
    pub(crate) fn with_remember_cookie_protected(mut self, protected: bool) -> Self {
        self.remember_cookie_protected = protected;
        self
    }
    fn ensure_remember_cookie_protected(&self) -> Result<()> {
        if self.remember_cookie_protected {
            Ok(())
        } else {
            Err(Error::custom(
                "auth_recaller_protection_required",
                "记住我 Cookie 必须启用加密且不得处于 except 名单",
            ))
        }
    }
    /// 设置登录时密码重哈希和失败凭据验证的最短时长。
    #[must_use]
    pub fn with_validation_options(mut self, rehash_on_login: bool, timebox: Duration) -> Self {
        self.rehash_on_login = rehash_on_login;
        self.timebox = timebox;
        self
    }
    /// 设置记住我 Cookie 的持续分钟数。
    #[must_use]
    pub fn with_remember_duration(mut self, minutes: i64) -> Self {
        self.remember_minutes = minutes;
        self
    }
    /// 返回隔离各具名 Guard 的 Session 键。
    pub fn session_name(&self) -> String {
        format!(
            "login_{}_ff9a88a790ea81b2b89656e56873ed257a77b366",
            self.name
        )
    }
    /// 返回记住我 Cookie 名称。
    pub fn recaller_name(&self) -> String {
        format!(
            "remember_{}_ff9a88a790ea81b2b89656e56873ed257a77b366",
            self.name
        )
    }
    /// 对密码哈希计算 Cookie 使用的 HMAC，不加密 Cookie。
    pub fn hash_password_for_cookie(&self, password: Option<&str>) -> Result<String> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.hash_key.as_bytes()).map_err(Error::internal)?;
        mac.update(password.unwrap_or("").as_bytes());
        let digest = mac.finalize().into_bytes();
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Ok(encoded)
    }
    async fn operation(&self) -> Operation<'_> {
        Operation {
            permit: Some(self.mutation.lock().await),
            epoch: None,
        }
    }
    fn start_mutation(&self, operation: &mut Operation<'_>) -> Result<u64> {
        let mut state = self.write()?;
        if let Some(epoch) = operation.epoch {
            if state.epoch != epoch {
                return Err(state_changed());
            }
            return Ok(epoch);
        }
        state.epoch = state.epoch.checked_add(1).ok_or_else(state_changed)?;
        operation.epoch = Some(state.epoch);
        Ok(state.epoch)
    }
    fn owns_operation(&self, operation: &Operation<'_>) -> Result<bool> {
        let Some(epoch) = operation.epoch else {
            return Ok(true);
        };
        Ok(self.read()?.epoch == epoch)
    }
    async fn emit<'a, E: Send + Sync + 'static>(
        &'a self,
        operation: &mut Operation<'a>,
        event: E,
    ) -> Result<()> {
        drop(operation.permit.take());
        let result = if let Some(events) = &self.events {
            events.dispatch(&event).await
        } else {
            Ok(())
        };
        operation.permit = Some(self.mutation.lock().await);
        if !self.owns_operation(operation)? {
            return Err(state_changed());
        }
        result
    }
    fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, State>> {
        self.state.read().map_err(|_| state_error())
    }
    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, State>> {
        self.state.write().map_err(|_| state_error())
    }
    async fn update_session(&self, user: &AuthUser) -> Result<()> {
        self.session.require_authenticated_storage()?;
        // 轮换失败或 future 被取消时，本请求保持拒绝认证；不宣称存储已回滚。
        {
            let mut state = self.write()?;
            state.user = None;
            state.logged_out = true;
        }
        self.session.put_typed(
            &self.session_name(),
            valid_identifier(user.auth_identifier()?)?,
        )?;
        self.session
            .mark_guard_authentication(&self.session_name(), true)?;
        self.session.regenerate(true).await?;
        self.session.require_authenticated_storage()?;
        self.write()?.logged_out = false;
        Ok(())
    }
    async fn set_user_inner<'a>(
        &'a self,
        operation: &mut Operation<'a>,
        user: AuthUser,
        persistent: bool,
    ) -> Result<()> {
        valid_identifier(user.auth_identifier()?)?;
        if persistent {
            self.session.require_authenticated_storage()?;
        }
        let transition = Transition::new(self, operation)?;
        {
            let mut state = self.write()?;
            state.user = Some(user.clone());
            state.persistent_user = persistent;
            state.logged_out = false;
        }
        self.emit(
            operation,
            Authenticated {
                guard: self.name.clone(),
                user,
            },
        )
        .await?;
        if persistent {
            self.session.require_authenticated_storage()?;
        }
        transition.commit();
        Ok(())
    }
    fn current_user(&self) -> Result<Option<AuthUser>> {
        let state = self.read()?;
        if !state.logged_out && state.persistent_user {
            self.session.require_authenticated_storage()?;
        }
        Ok(if state.logged_out {
            None
        } else {
            state.user.clone()
        })
    }
    async fn user_inner<'a>(&'a self, operation: &mut Operation<'a>) -> Result<Option<AuthUser>> {
        {
            let state = self.read()?;
            if state.logged_out {
                return Ok(None);
            }
            if let Some(user) = &state.user {
                if state.persistent_user {
                    self.session.require_authenticated_storage()?;
                }
                valid_identifier(user.auth_identifier()?)?;
                return Ok(Some(user.clone()));
            }
        }
        self.session.require_authenticated_storage()?;
        if let Some(id) = self.session.get_json(&self.session_name())? {
            valid_identifier(id.clone())?;
            if let Some(user) = self.provider.retrieve_by_id(&id).await? {
                valid_identifier(user.auth_identifier()?)?;
                let transition = Transition::new(self, operation)?;
                self.session
                    .mark_guard_authentication(&self.session_name(), true)?;
                {
                    let mut state = self.write()?;
                    state.user = Some(user.clone());
                    state.persistent_user = true;
                }
                let emitted = self
                    .emit(
                        operation,
                        Authenticated {
                            guard: self.name.clone(),
                            user: user.clone(),
                        },
                    )
                    .await;
                if !self.owns_operation(operation)? {
                    return self.current_user();
                }
                emitted?;
                let current = self.current_user()?;
                transition.commit();
                return Ok(current);
            }
        }
        if self.read()?.recall_attempted {
            return Ok(None);
        }
        let Some(raw) = &self.recaller else {
            return Ok(None);
        };
        self.ensure_remember_cookie_protected()?;
        let Some(recaller) = Recaller::parse(raw) else {
            return Ok(None);
        };
        self.write()?.recall_attempted = true;
        let Some(user) = self
            .provider
            .retrieve_by_token(&recaller.identifier, &recaller.token)
            .await?
        else {
            return Ok(None);
        };
        self.write()?.via_remember = true;
        let Some(password) = user.auth_password()? else {
            return Ok(None);
        };
        let mac = self.hash_password_for_cookie(Some(&password))?;
        if !bool::from(mac.as_bytes().ct_eq(recaller.hash.as_bytes()))
            && !bool::from(password.as_bytes().ct_eq(recaller.hash.as_bytes()))
        {
            return Ok(None);
        }
        let transition = Transition::new(self, operation)?;
        self.update_session(&user).await?;
        {
            let mut state = self.write()?;
            state.user = Some(user.clone());
            state.persistent_user = true;
        }
        let emitted = self
            .emit(
                operation,
                Login {
                    guard: self.name.clone(),
                    user: user.clone(),
                    remember: true,
                },
            )
            .await;
        if !self.owns_operation(operation)? {
            return self.current_user();
        }
        emitted?;
        let current = self.current_user()?;
        transition.commit();
        Ok(current)
    }
    async fn cycle_token(&self, operation: &Operation<'_>, user: &AuthUser) -> Result<()> {
        let epoch = operation.epoch.ok_or_else(state_changed)?;
        if !self.owns_operation(operation)? {
            return Err(state_changed());
        }
        let previous = user.remember_token()?;
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut token = String::with_capacity(60);
        while token.len() < 60 {
            let mut bytes = [0_u8; 64];
            getrandom::fill(&mut bytes).map_err(Error::internal)?;
            for byte in bytes {
                if byte < 248 && token.len() < 60 {
                    token.push(char::from(alphabet[usize::from(byte % 62)]));
                }
            }
        }
        if !user.compare_and_set_remember_token(previous.as_deref(), Some(token.clone()))? {
            return Err(Error::custom(
                "auth_remember_token_conflict",
                "账户记住我 token 已由并发操作修改",
            ));
        }
        let mut transition = TokenTransition {
            guard: self,
            user: user.clone(),
            previous,
            candidate: token.clone(),
            epoch,
            committed: false,
        };
        if let Err(error) = self
            .provider
            .update_remember_token(user.as_ref(), &token)
            .await
        {
            transition.restore()?;
            return Err(error);
        }
        transition.commit();
        Ok(())
    }
    async fn login_inner<'a>(
        &'a self,
        operation: &mut Operation<'a>,
        user: AuthUser,
        remember: bool,
    ) -> Result<()> {
        let transition = Transition::new(self, operation)?;
        self.update_session(&user).await?;
        if let Some(password) = user.auth_password()?.filter(|p| !p.is_empty()) {
            self.session.put_typed(
                &format!("password_hash_{}", self.name),
                self.hash_password_for_cookie(Some(&password))?,
            )?;
        }
        if remember {
            self.ensure_remember_cookie_protected()?;
            if user
                .remember_token()?
                .is_none_or(|token| token.is_empty() || token == "0")
            {
                self.cycle_token(operation, &user).await?;
            }
            let identifier = recaller_identifier(user.auth_identifier()?)?;
            let token = user.remember_token()?.unwrap_or_default();
            if token.contains('|') {
                return Err(Error::custom(
                    "auth_recaller_token_invalid",
                    "记住我 token 不支持分隔符",
                ));
            }
            let value = format!(
                "{identifier}|{token}|{}",
                self.hash_password_for_cookie(user.auth_password()?.as_deref())?
            );
            self.cookies.queue(self.cookies.factory().make(
                self.recaller_name(),
                value,
                self.remember_minutes,
                CookieOptions::default(),
            )?)?;
        }
        {
            let mut state = self.write()?;
            state.user = Some(user.clone());
            state.persistent_user = true;
            state.logged_out = false;
        }
        self.emit(
            operation,
            Login {
                guard: self.name.clone(),
                user: user.clone(),
                remember,
            },
        )
        .await?;
        self.set_user_inner(operation, user, true).await?;
        transition.commit();
        Ok(())
    }
    async fn validated<'a>(
        &'a self,
        operation: &mut Operation<'a>,
        credentials: &Credentials,
    ) -> Result<Option<AuthUser>> {
        let user = self.provider.retrieve_by_credentials(credentials).await?;
        if let Some(user) = user {
            valid_identifier(user.auth_identifier()?)?;
            if self
                .provider
                .validate_credentials(user.as_ref(), credentials)
                .await?
            {
                self.emit(
                    operation,
                    Validated {
                        guard: self.name.clone(),
                        user: user.clone(),
                    },
                )
                .await?;
                return Ok(Some(user));
            }
        }
        Ok(None)
    }
    async fn rehash(&self, user: &AuthUser, credentials: &Credentials) -> Result<()> {
        if self.rehash_on_login {
            self.provider
                .rehash_password_if_required(user.as_ref(), credentials, false)
                .await?;
        }
        Ok(())
    }
}
impl Guard for SessionGuard {
    fn provider_name(&self) -> Option<&str> {
        self.provider_name.as_deref()
    }
    fn as_stateful(&self) -> Option<&dyn StatefulGuard> {
        Some(self)
    }
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.user_inner(&mut operation).await
        })
    }
    fn validate<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            let start = tokio::time::Instant::now();
            let valid = self.validated(&mut operation, credentials).await?.is_some();
            if !valid {
                tokio::time::sleep_until(start + self.timebox).await;
            }
            Ok(valid)
        })
    }
    fn has_user(&self) -> Result<bool> {
        Ok(self.read()?.user.is_some())
    }
    fn set_user(&self, user: AuthUser) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.set_user_inner(&mut operation, user, false).await
        })
    }
    fn id(&self) -> BoxFuture<'_, Result<Option<Value>>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            if self.read()?.logged_out {
                return Ok(None);
            }
            if let Some(user) = self.user_inner(&mut operation).await? {
                return Ok(Some(user.auth_identifier()?));
            }
            if self.read()?.logged_out {
                return Ok(None);
            }
            self.session.require_authenticated_storage()?;
            self.session
                .get_json(&self.session_name())?
                .map(valid_identifier)
                .transpose()
        })
    }
}
impl StatefulGuard for SessionGuard {
    fn session_name(&self) -> Option<String> {
        Some(SessionGuard::session_name(self))
    }
    fn provider(&self) -> Arc<dyn UserProvider> {
        self.provider.clone()
    }
    fn attempt<'a>(
        &'a self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.start_mutation(&mut operation)?;
            let start = tokio::time::Instant::now();
            self.emit(
                &mut operation,
                Attempting {
                    guard: self.name.clone(),
                    credentials: credentials.clone(),
                    remember,
                },
            )
            .await?;
            let user = self.provider.retrieve_by_credentials(credentials).await?;
            if let Some(user) = &user {
                valid_identifier(user.auth_identifier()?)?;
                if self
                    .provider
                    .validate_credentials(user.as_ref(), credentials)
                    .await?
                {
                    self.emit(
                        &mut operation,
                        Validated {
                            guard: self.name.clone(),
                            user: user.clone(),
                        },
                    )
                    .await?;
                    self.rehash(user, credentials).await?;
                    self.login_inner(&mut operation, user.clone(), remember)
                        .await?;
                    return Ok(true);
                }
            }
            self.emit(
                &mut operation,
                Failed {
                    guard: self.name.clone(),
                    user,
                    credentials: credentials.clone(),
                },
            )
            .await?;
            tokio::time::sleep_until(start + self.timebox).await;
            Ok(false)
        })
    }
    fn once<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.start_mutation(&mut operation)?;
            self.emit(
                &mut operation,
                Attempting {
                    guard: self.name.clone(),
                    credentials: credentials.clone(),
                    remember: false,
                },
            )
            .await?;
            let start = tokio::time::Instant::now();
            let user = self.provider.retrieve_by_credentials(credentials).await?;
            if let Some(user) = &user {
                valid_identifier(user.auth_identifier()?)?;
                if self
                    .provider
                    .validate_credentials(user.as_ref(), credentials)
                    .await?
                {
                    self.emit(
                        &mut operation,
                        Validated {
                            guard: self.name.clone(),
                            user: user.clone(),
                        },
                    )
                    .await?;
                    self.rehash(user, credentials).await?;
                    self.set_user_inner(&mut operation, user.clone(), false)
                        .await?;
                    return Ok(true);
                }
            }
            tokio::time::sleep_until(start + self.timebox).await;
            self.emit(
                &mut operation,
                Failed {
                    guard: self.name.clone(),
                    user,
                    credentials: credentials.clone(),
                },
            )
            .await?;
            Ok(false)
        })
    }
    fn login(&self, user: AuthUser, remember: bool) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.login_inner(&mut operation, user, remember).await
        })
    }
    fn login_using_id<'a>(
        &'a self,
        identifier: &'a Value,
        remember: bool,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            let user = self.provider.retrieve_by_id(identifier).await?;
            if let Some(user) = &user {
                self.login_inner(&mut operation, user.clone(), remember)
                    .await?;
            }
            Ok(user)
        })
    }
    fn once_using_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            let user = self.provider.retrieve_by_id(identifier).await?;
            if let Some(user) = &user {
                self.set_user_inner(&mut operation, user.clone(), false)
                    .await?;
            }
            Ok(user)
        })
    }
    fn via_remember(&self) -> Result<bool> {
        Ok(self.read()?.via_remember)
    }
    fn logout(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            let transition = Transition::new(self, &mut operation)?;
            let user = self.user_inner(&mut operation).await?;
            self.session.remove(&self.session_name())?;
            self.session
                .mark_guard_authentication(&self.session_name(), false)?;
            self.cookies.unqueue(&self.recaller_name(), None)?;
            if self.recaller.is_some() {
                self.cookies.queue(
                    self.cookies
                        .factory()
                        .forget(self.recaller_name(), CookieOptions::default())?,
                )?;
            }
            if let Some(user) = &user
                && user
                    .remember_token()?
                    .is_some_and(|token| !token.is_empty() && token != "0")
            {
                self.cycle_token(&operation, user).await?;
            }
            self.emit(
                &mut operation,
                Logout {
                    guard: self.name.clone(),
                    user,
                },
            )
            .await?;
            let mut state = self.write()?;
            state.user = None;
            state.logged_out = true;
            drop(state);
            transition.commit();
            Ok(())
        })
    }
}
fn state_changed() -> Error {
    Error::custom(
        "auth_guard_state_changed",
        "认证操作期间身份已由后继操作替换",
    )
}
fn state_error() -> Error {
    Error::custom("auth_guard_state_poisoned", "认证 Guard 状态不可用")
}
fn valid_identifier(value: Value) -> Result<Value> {
    match &value {
        Value::String(id) if !id.trim().is_empty() => Ok(value),
        Value::Number(_) => Ok(value),
        _ => Err(Error::custom(
            "auth_identifier_invalid",
            "认证账户必须具有非空字符串或数字 ID",
        )),
    }
}

const RECALLER_LIMIT: usize = 16_384;
const IDENTIFIER_LIMIT: usize = 4_096;
const IDENTIFIER_PREFIX: &str = "~or1~s";
fn recaller_identifier(value: Value) -> Result<String> {
    let value = valid_identifier(value)?;
    let identifier = match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        _ => return Err(Error::custom("auth_identifier_invalid", "账户 ID 类型无效")),
    };
    if identifier.len() > IDENTIFIER_LIMIT {
        return Err(Error::custom(
            "auth_recaller_identifier_invalid",
            "记住我 ID 超过长度限制",
        ));
    }
    // 常规标识保留 Laravel 格式；分隔符与保留前缀使用有界、带类型的版本编码。
    if identifier.contains('|') || identifier.starts_with("~or1~") {
        Ok(format!(
            "{IDENTIFIER_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(identifier.as_bytes())
        ))
    } else {
        Ok(identifier)
    }
}
struct Recaller {
    identifier: Value,
    token: String,
    hash: String,
}
impl Recaller {
    fn parse(raw: &str) -> Option<Self> {
        if raw.len() > RECALLER_LIMIT {
            return None;
        }
        let raw = php_serialized_string(raw).unwrap_or(raw);
        let mut segments = raw.split('|');
        let identifier = segments.next()?;
        let token = segments.next()?;
        let hash = segments.next()?;
        if identifier.trim().is_empty()
            || token.trim().is_empty()
            || identifier.len() > IDENTIFIER_LIMIT * 2
        {
            return None;
        }
        let identifier = if let Some(encoded) = identifier.strip_prefix(IDENTIFIER_PREFIX) {
            let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
            if decoded.len() > IDENTIFIER_LIMIT || URL_SAFE_NO_PAD.encode(&decoded) != encoded {
                return None;
            }
            String::from_utf8(decoded).ok()?
        } else {
            if identifier.starts_with("~or1~") || identifier.len() > IDENTIFIER_LIMIT {
                return None;
            }
            identifier.to_owned()
        };
        let identifier = valid_identifier(Value::String(identifier)).ok()?;
        Some(Self {
            identifier,
            token: token.to_owned(),
            hash: hash.to_owned(),
        })
    }
}
// 只解析 PHP 字符串字面量；不执行对象、引用或任意反序列化代码。
fn php_serialized_string(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix("s:")?;
    let (length, value) = rest.split_once(":\"")?;
    if length.is_empty() || length.len() > 5 || !length.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let length: usize = length.parse().ok()?;
    let value = value.strip_suffix("\";")?;
    if length <= RECALLER_LIMIT && value.len() == length {
        Some(value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recaller_identifiers_reject_ambiguous_values_without_losing_numeric_precision() -> Result<()>
    {
        assert_eq!(recaller_identifier(json!(u64::MAX))?, u64::MAX.to_string());
        assert_eq!(
            recaller_identifier(json!("tenant:user-42"))?,
            "tenant:user-42"
        );
        for invalid in [Value::Null, json!({"id": 42}), json!([42]), json!(" ")] {
            assert!(recaller_identifier(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn failed_events_hide_all_credentials_and_user_details() {
        let event = Failed {
            guard: "web".into(),
            user: None,
            credentials: Credentials::new(serde_json::Map::from_iter([(
                "password".into(),
                json!("private-password"),
            )])),
        };
        let output = format!("{event:?}");
        assert!(!output.contains("private-password"));
        assert!(!output.contains("credentials"));
    }
    struct EmptyProvider(bool);
    impl UserProvider for EmptyProvider {
        fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async move { Ok(self.0.then(account)) })
        }
        fn retrieve_by_token<'a>(
            &'a self,
            _: &'a Value,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn update_remember_token<'a>(
            &'a self,
            _: &'a dyn super::super::Authenticatable,
            _: &'a str,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                if self.0 {
                    std::future::pending::<()>().await;
                }
                Ok(())
            })
        }
        fn retrieve_by_credentials<'a>(
            &'a self,
            _: &'a Credentials,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn validate_credentials<'a>(
            &'a self,
            _: &'a dyn super::super::Authenticatable,
            _: &'a Credentials,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn rehash_password_if_required<'a>(
            &'a self,
            _: &'a dyn super::super::Authenticatable,
            _: &'a Credentials,
            _: bool,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn login_and_authenticated_listeners_can_read_same_guard() -> Result<()> {
        use crate::{
            core::ProviderBuilder, events::EventsServiceProvider, http::cookie::CookieFactory,
            session::MemorySessionStore,
        };
        use std::sync::{
            Weak,
            atomic::{AtomicUsize, Ordering},
        };
        let slot = Arc::new(RwLock::new(Weak::<SessionGuard>::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut events = EventsServiceProvider::new();
        let capture = slot.clone();
        let observed = calls.clone();
        events.listen::<Login, _>(move |event, _| {
            let capture = capture.clone();
            let observed = observed.clone();
            let identifier = event.user.auth_identifier();
            Box::pin(async move {
                let guard = capture
                    .read()
                    .map_err(|_| state_error())?
                    .upgrade()
                    .ok_or_else(state_error)?;
                assert_eq!(guard.id().await?, Some(identifier?));
                assert!(guard.user().await?.is_some());
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        let capture = slot.clone();
        let observed = calls.clone();
        events.listen::<Authenticated, _>(move |_, _| {
            let capture = capture.clone();
            let observed = observed.clone();
            Box::pin(async move {
                let guard = capture
                    .read()
                    .map_err(|_| state_error())?
                    .upgrade()
                    .ok_or_else(state_error)?;
                assert!(guard.user().await?.is_some());
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        let mut builder = ProviderBuilder::new();
        builder.provider(events);
        let runtime = builder.start(figment::Figment::new()).await?;
        let dispatcher = (*runtime.container().resolve::<Events>().await?).clone();
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
        let guard = Arc::new(SessionGuard::new(
            "web".into(),
            Arc::new(EmptyProvider(false)),
            session,
            CookieJar::new(CookieFactory::default()),
            None,
            Some(dispatcher),
            None,
        ));
        *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
        let user: AuthUser = Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
            [("id".into(), json!(42))],
        )));
        tokio::time::timeout(Duration::from_secs(2), guard.login(user, false))
            .await
            .map_err(Error::internal)??;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        Ok(())
    }
    async fn failure_guard(
        provider: Arc<dyn UserProvider>,
        events: Option<Events>,
    ) -> Result<SessionGuard> {
        let session = Session::new(
            None,
            Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )?;
        session.start().await?;
        session.put_typed("unrelated", "retained")?;
        Ok(SessionGuard::new(
            "web".into(),
            provider,
            session,
            CookieJar::new(crate::http::cookie::CookieFactory::default()),
            None,
            events,
            None,
        )
        .with_remember_cookie_protected(true))
    }
    fn account() -> AuthUser {
        Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
            [("id".into(), json!(42)), ("password".into(), json!("hash"))],
        )))
    }
    async fn assert_failed_closed(guard: &SessionGuard) -> Result<()> {
        assert!(guard.user().await?.is_none());
        assert!(guard.id().await?.is_none());
        assert!(!guard.has_user()?);
        assert!(guard.session.get_json(&guard.session_name())?.is_none());
        assert!(guard.session.get_json("password_hash_web")?.is_none());
        assert!(!guard.cookies.has_queued(&guard.recaller_name(), None)?);
        assert_eq!(
            guard.session.get_json("unrelated")?,
            Some(json!("retained"))
        );
        Ok(())
    }
    #[tokio::test]
    async fn cancelled_token_persistence_removes_partial_login() -> Result<()> {
        let guard = failure_guard(Arc::new(EmptyProvider(true)), None).await?;
        let user = account();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), guard.login(user.clone(), true))
                .await
                .is_err()
        );
        assert!(user.remember_token()?.is_none());
        assert_failed_closed(&guard).await
    }
    #[tokio::test]
    async fn cookie_creation_failure_removes_partial_login() -> Result<()> {
        let guard = failure_guard(Arc::new(EmptyProvider(false)), None)
            .await?
            .with_remember_duration(i64::MAX);
        assert!(guard.login(account(), true).await.is_err());
        assert_failed_closed(&guard).await
    }
    #[tokio::test]
    async fn login_event_error_removes_identity_and_queued_recaller() -> Result<()> {
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Login, _>(|_, _| {
            Box::pin(async { Err(Error::custom("listener_failed", "测试监听器失败")) })
        });
        let mut builder = crate::core::ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime.container().resolve::<Events>().await?).clone();
        let guard = failure_guard(Arc::new(EmptyProvider(false)), Some(events)).await?;
        assert!(guard.login(account(), true).await.is_err());
        assert_failed_closed(&guard).await
    }
    #[tokio::test]
    async fn logout_event_error_still_removes_identity_and_cookie() -> Result<()> {
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Logout, _>(|_, _| {
            Box::pin(async { Err(Error::custom("listener_failed", "测试监听器失败")) })
        });
        let mut builder = crate::core::ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime.container().resolve::<Events>().await?).clone();
        let guard = failure_guard(Arc::new(EmptyProvider(false)), Some(events)).await?;
        guard.login(account(), true).await?;
        assert!(guard.logout().await.is_err());
        assert_failed_closed(&guard).await
    }
    #[tokio::test]
    async fn authenticated_listener_logout_is_observed_by_outer_user() -> Result<()> {
        let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
        let captured = slot.clone();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Authenticated, _>(move |_, _| {
            let captured = captured.clone();
            Box::pin(async move {
                let guard = captured
                    .read()
                    .map_err(|_| state_error())?
                    .upgrade()
                    .ok_or_else(state_error)?;
                guard.logout().await
            })
        });
        let mut builder = crate::core::ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(figment::Figment::new()).await?;
        let events = (*runtime.container().resolve::<Events>().await?).clone();
        let guard = Arc::new(failure_guard(Arc::new(EmptyProvider(true)), Some(events)).await?);
        guard.session.put_typed(&guard.session_name(), 42)?;
        *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
        let resolved = tokio::time::timeout(Duration::from_secs(2), guard.user())
            .await
            .map_err(Error::internal)??;
        assert!(resolved.is_none());
        assert!(guard.user().await?.is_none());
        Ok(())
    }
    #[test]
    fn recallers_support_legacy_serialized_strings_extra_segments_and_encoded_identifiers()
    -> Result<()> {
        for identifier in ["tenant|user", "~or1~reserved", "用户|42", "normal-id"] {
            let encoded = recaller_identifier(json!(identifier))?;
            let raw = format!("{encoded}|token|hash|ignored");
            let parsed = Recaller::parse(&raw).ok_or_else(state_error)?;
            assert_eq!(parsed.identifier, json!(identifier));
            assert_eq!(parsed.token, "token");
            assert_eq!(parsed.hash, "hash");
            let serialized = format!("s:{}:\"{}\";", raw.len(), raw);
            assert_eq!(
                Recaller::parse(&serialized)
                    .ok_or_else(state_error)?
                    .identifier,
                json!(identifier)
            );
        }
        assert!(php_serialized_string("s:2:\"long\";").is_none());
        assert!(php_serialized_string("O:8:\"stdClass\":0:{}").is_none());
        assert!(Recaller::parse("~or1~s!!!|token|hash").is_none());
        assert!(Recaller::parse(&"x".repeat(RECALLER_LIMIT + 1)).is_none());
        Ok(())
    }
    #[tokio::test]
    async fn all_direct_user_entry_points_reject_missing_id_without_email_requirement() -> Result<()>
    {
        for identifier in [Value::Null, json!({"id":42}), json!([42])] {
            let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
            let user: AuthUser = Arc::new(super::super::GenericUser::new(
                serde_json::Map::from_iter([("id".into(), identifier)]),
            ));
            assert!(guard.set_user(user.clone()).await.is_err());
            assert!(guard.login(user, false).await.is_err());
            assert_failed_closed(&guard).await?;
        }
        for identifier in [
            json!(42),
            json!("f4733e92-bc37-4196-80a8-92b0dd5736d3"),
            json!("tenant|user"),
        ] {
            let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
            let user: AuthUser = Arc::new(super::super::GenericUser::new(
                serde_json::Map::from_iter([("id".into(), identifier.clone())]),
            ));
            guard.login(user, true).await?;
            assert_eq!(guard.id().await?, Some(identifier));
        }
        Ok(())
    }
    #[tokio::test]
    async fn remember_requires_trusted_cookie_protection_but_plain_session_does_not() -> Result<()>
    {
        let guard = failure_guard(Arc::new(EmptyProvider(false)), None)
            .await?
            .with_remember_cookie_protected(false);
        assert!(guard.login(account(), true).await.is_err());
        assert_failed_closed(&guard).await?;
        guard.login(account(), false).await?;
        assert_eq!(guard.id().await?, Some(json!(42)));
        let mut recall = failure_guard(Arc::new(EmptyProvider(false)), None)
            .await?
            .with_remember_cookie_protected(false);
        recall.recaller = Some("42|token|hash".into());
        assert!(recall.user().await.is_err());
        Ok(())
    }
    fn identified_account(identifier: i64) -> AuthUser {
        Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
            [("id".into(), json!(identifier))],
        )))
    }
    async fn test_dispatcher(provider: crate::events::EventsServiceProvider) -> Result<Events> {
        let mut builder = crate::core::ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(figment::Figment::new()).await?;
        Ok((*runtime.container().resolve::<Events>().await?).clone())
    }
    #[tokio::test]
    async fn cancelled_old_login_does_not_clear_newer_successful_login() -> Result<()> {
        let entered = Arc::new(tokio::sync::Notify::new());
        let capture = entered.clone();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Login, _>(move |event, _| {
            let capture = capture.clone();
            let identifier = event.user.auth_identifier();
            Box::pin(async move {
                if identifier? == json!(42) {
                    capture.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(())
            })
        });
        let guard = Arc::new(
            failure_guard(
                Arc::new(EmptyProvider(false)),
                Some(test_dispatcher(provider).await?),
            )
            .await?,
        );
        let original = guard.clone();
        let worker =
            tokio::spawn(async move { original.login(identified_account(42), false).await });
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .map_err(Error::internal)?;
        guard.login(identified_account(7), false).await?;
        worker.abort();
        assert!(worker.await.is_err());
        assert_eq!(guard.id().await?, Some(json!(7)));
        assert_eq!(
            guard.session.get_json(&guard.session_name())?,
            Some(json!(7))
        );
        Ok(())
    }
    #[tokio::test]
    async fn listener_replacement_login_invalidates_outer_login_without_overwriting_new_user()
    -> Result<()> {
        let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
        let capture = slot.clone();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Login, _>(move |event, _| {
            let capture = capture.clone();
            let identifier = event.user.auth_identifier();
            Box::pin(async move {
                if identifier? == json!(42) {
                    let guard = capture
                        .read()
                        .map_err(|_| state_error())?
                        .upgrade()
                        .ok_or_else(state_error)?;
                    guard.login(identified_account(7), false).await?;
                }
                Ok(())
            })
        });
        let guard = Arc::new(
            failure_guard(
                Arc::new(EmptyProvider(false)),
                Some(test_dispatcher(provider).await?),
            )
            .await?,
        );
        *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
        let outer = tokio::time::timeout(
            Duration::from_secs(2),
            guard.login(identified_account(42), false),
        )
        .await
        .map_err(Error::internal)?;
        assert!(outer.is_err());
        assert_eq!(guard.id().await?, Some(json!(7)));
        assert_eq!(
            guard.session.get_json(&guard.session_name())?,
            Some(json!(7))
        );
        Ok(())
    }
    #[tokio::test]
    async fn login_listener_logout_invalidates_outer_login_without_resurrection() -> Result<()> {
        let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
        let capture = slot.clone();
        let mut provider = crate::events::EventsServiceProvider::new();
        provider.listen::<Login, _>(move |_, _| {
            let capture = capture.clone();
            Box::pin(async move {
                let guard = capture
                    .read()
                    .map_err(|_| state_error())?
                    .upgrade()
                    .ok_or_else(state_error)?;
                guard.logout().await
            })
        });
        let guard = Arc::new(
            failure_guard(
                Arc::new(EmptyProvider(false)),
                Some(test_dispatcher(provider).await?),
            )
            .await?,
        );
        *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
        let outer = tokio::time::timeout(
            Duration::from_secs(2),
            guard.login(identified_account(42), false),
        )
        .await
        .map_err(Error::internal)?;
        assert!(outer.is_err());
        assert!(guard.user().await?.is_none());
        assert!(guard.session.get_json(&guard.session_name())?.is_none());
        Ok(())
    }
    #[tokio::test]
    async fn forged_plain_or_except_cookie_session_cannot_authenticate() -> Result<()> {
        use crate::session::{CookieSessionStore, SessionId, SessionRequestContext};
        let id = SessionId::parse(&"A".repeat(40))?;
        for (encrypted, except) in [
            (false, vec![]),
            (true, vec!["openrust_session".to_owned()]),
            (true, vec![id.as_str().to_owned()]),
        ] {
            let cookies = CookieJar::new(crate::http::CookieFactory::default());
            let session = Session::new(
                Some(id.clone()),
                Arc::new(CookieSessionStore::new(5, false)?),
                None,
            )?;
            let guard = SessionGuard::new(
                "web".into(),
                Arc::new(EmptyProvider(true)),
                session,
                cookies.clone(),
                None,
                None,
                None,
            )
            .with_remember_cookie_protected(true);
            let data = serde_json::to_string(&json!({guard.session_name(): 42}))
                .map_err(Error::internal)?;
            let payload = serde_json::to_string(&json!({"data": data, "expires": time::OffsetDateTime::now_utc().unix_timestamp() + 300})).map_err(Error::internal)?;
            let context = SessionRequestContext::from_request(
                json!({id.as_str(): payload}),
                cookies,
                Some(crate::http::cookie::CookieProtection { encrypted, except }),
            )?;
            guard.session.set_request_on_handler(context).await?;
            guard.session.start().await?;
            // 确认恶意账户键确实已被载入，不能仅靠空 Session 得到负例。
            assert_eq!(
                guard.session.get_json(&guard.session_name())?,
                Some(json!(42))
            );
            assert!(guard.user().await.is_err());
            assert!(guard.login(account(), false).await.is_err());
            // 可信调用方单次认证不依赖持久 Session 的安全证明。
            guard.set_user(account()).await?;
            assert_eq!(guard.id().await?, Some(json!(42)));
        }
        Ok(())
    }
    #[tokio::test]
    async fn cached_persistent_user_rechecks_handler_and_cookie_name() -> Result<()> {
        let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
        guard.login(account(), false).await?;
        guard
            .session
            .set_handler(Arc::new(crate::session::CookieSessionStore::new(5, false)?))?;
        assert!(guard.user().await.is_err());
        // 即使应用不再读取 Guard，自动保存也必须拒绝降级后的持久化。
        assert!(guard.session.save().await.is_err());

        let cookies = CookieJar::new(crate::http::CookieFactory::default());
        let session = Session::new(
            None,
            Arc::new(crate::session::CookieSessionStore::new(5, false)?),
            None,
        )?;
        let context = crate::session::SessionRequestContext::from_request(
            json!({}),
            cookies.clone(),
            Some(crate::http::cookie::CookieProtection {
                encrypted: true,
                except: vec!["later_plain_name".into()],
            }),
        )?;
        session.set_request_on_handler(context).await?;
        session.start().await?;
        let guard = SessionGuard::new(
            "web".into(),
            Arc::new(EmptyProvider(false)),
            session,
            cookies,
            None,
            None,
            None,
        );
        guard.login(account(), false).await?;
        guard.session.set_name("later_plain_name")?;
        assert!(guard.user().await.is_err());
        assert!(guard.session.save().await.is_err());
        Ok(())
    }
    #[derive(Default)]
    struct TokenProvider {
        calls: std::sync::atomic::AtomicUsize,
        pending: std::sync::atomic::AtomicBool,
        fail: std::sync::atomic::AtomicBool,
        stored: std::sync::Mutex<Option<String>>,
        entered: tokio::sync::Notify,
    }
    impl UserProvider for TokenProvider {
        fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn retrieve_by_token<'a>(
            &'a self,
            _: &'a Value,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn update_remember_token<'a>(
            &'a self,
            user: &'a dyn super::super::Authenticatable,
            token: &'a str,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                use std::sync::atomic::Ordering;
                assert_eq!(user.remember_token()?.as_deref(), Some(token));
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.entered.notify_one();
                if self.pending.load(Ordering::SeqCst) {
                    std::future::pending::<()>().await;
                }
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::custom("token_write_failed", "测试 token 持久化失败"));
                }
                *self.stored.lock().map_err(|_| state_error())? = Some(token.to_owned());
                Ok(())
            })
        }
        fn retrieve_by_credentials<'a>(
            &'a self,
            _: &'a Credentials,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn validate_credentials<'a>(
            &'a self,
            _: &'a dyn super::super::Authenticatable,
            _: &'a Credentials,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn rehash_password_if_required<'a>(
            &'a self,
            _: &'a dyn super::super::Authenticatable,
            _: &'a Credentials,
            _: bool,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn failed_token_update_restores_user_before_retrying_same_account() -> Result<()> {
        use std::sync::atomic::Ordering;
        let provider = Arc::new(TokenProvider::default());
        provider.fail.store(true, Ordering::SeqCst);
        let guard = failure_guard(provider.clone(), None).await?;
        let user = account();
        assert!(guard.login(user.clone(), true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(user.remember_token()?.is_none());
        assert_failed_closed(&guard).await?;
        provider.fail.store(false, Ordering::SeqCst);
        guard.login(user.clone(), true).await?;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            user.remember_token()?,
            *provider.stored.lock().map_err(|_| state_error())?
        );
        Ok(())
    }
    #[tokio::test]
    async fn cancelled_token_update_restores_user_before_retrying_same_account() -> Result<()> {
        use std::sync::atomic::Ordering;
        let provider = Arc::new(TokenProvider::default());
        provider.pending.store(true, Ordering::SeqCst);
        let guard = failure_guard(provider.clone(), None).await?;
        let user = account();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), guard.login(user.clone(), true))
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(user.remember_token()?.is_none());
        assert_failed_closed(&guard).await?;
        provider.pending.store(false, Ordering::SeqCst);
        guard.login(user.clone(), true).await?;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            user.remember_token()?,
            *provider.stored.lock().map_err(|_| state_error())?
        );
        Ok(())
    }
    #[tokio::test]
    async fn failed_logout_token_rotation_restores_previous_user_token() -> Result<()> {
        use std::sync::atomic::Ordering;
        let provider = Arc::new(TokenProvider::default());
        let guard = failure_guard(provider.clone(), None).await?;
        let user = account();
        guard.login(user.clone(), true).await?;
        let previous = user.remember_token()?;
        provider.fail.store(true, Ordering::SeqCst);
        assert!(guard.logout().await.is_err());
        assert_eq!(user.remember_token()?, previous);
        assert_failed_closed(&guard).await
    }
    #[tokio::test]
    async fn stale_token_cleanup_cannot_replace_newer_committed_token() -> Result<()> {
        let provider = Arc::new(TokenProvider::default());
        let guard = failure_guard(provider.clone(), None).await?;
        let user = account();
        user.set_remember_token(Some("old".into()))?;
        let mut first = guard.operation().await;
        let epoch = guard.start_mutation(&mut first)?;
        user.set_remember_token(Some("pending".into()))?;
        let stale = TokenTransition {
            guard: &guard,
            user: user.clone(),
            previous: Some("old".into()),
            candidate: "pending".into(),
            epoch,
            committed: false,
        };
        drop(first);
        guard.set_user(user.clone()).await?;
        guard.logout().await?;
        let committed = user.remember_token()?;
        assert_ne!(committed.as_deref(), Some("pending"));
        assert_eq!(
            committed,
            *provider.stored.lock().map_err(|_| state_error())?
        );
        drop(stale);
        assert_eq!(user.remember_token()?, committed);
        guard.login(user.clone(), true).await?;
        assert_eq!(guard.id().await?, Some(json!(42)));
        Ok(())
    }
    struct RestoreBarrierUser {
        user: super::super::GenericUser,
        install_conflict: bool,
        restore_entered: tokio::sync::Notify,
        restore_gate: (std::sync::Mutex<bool>, std::sync::Condvar),
    }
    impl super::super::Authenticatable for RestoreBarrierUser {
        fn auth_identifier_name(&self) -> &str {
            self.user.auth_identifier_name()
        }
        fn auth_identifier(&self) -> Result<Value> {
            self.user.auth_identifier()
        }
        fn auth_password_name(&self) -> &str {
            self.user.auth_password_name()
        }
        fn auth_password(&self) -> Result<Option<String>> {
            self.user.auth_password()
        }
        fn remember_token(&self) -> Result<Option<String>> {
            self.user.remember_token()
        }
        fn set_remember_token(&self, value: Option<String>) -> Result<()> {
            self.user.set_remember_token(value)
        }
        fn remember_token_name(&self) -> &str {
            self.user.remember_token_name()
        }
        fn compare_and_set_remember_token(
            &self,
            expected: Option<&str>,
            replacement: Option<String>,
        ) -> Result<bool> {
            if self.install_conflict && replacement.is_some() {
                return Ok(false);
            }
            if replacement.is_none() {
                // 暂停旧 Guard 的撤销，使另一 Guard 在原子比较之前提交新 token。
                self.restore_entered.notify_one();
                let (gate, signal) = &self.restore_gate;
                let released = gate.lock().map_err(|_| state_error())?;
                let (released, timeout) = signal
                    .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                    .map_err(|_| state_error())?;
                if timeout.timed_out() && !*released {
                    return Err(state_error());
                }
            }
            self.user
                .compare_and_set_remember_token(expected, replacement)
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_guard_restore_cannot_overwrite_other_guards_committed_token() -> Result<()> {
        use std::sync::atomic::Ordering;
        let shared = Arc::new(RestoreBarrierUser {
            install_conflict: false,
            user: super::super::GenericUser::new(serde_json::Map::from_iter([
                ("id".into(), json!(42)),
                ("password".into(), json!("hash")),
            ])),
            restore_entered: tokio::sync::Notify::new(),
            restore_gate: (std::sync::Mutex::new(false), std::sync::Condvar::new()),
        });
        let user: AuthUser = shared.clone();
        let provider_a = Arc::new(TokenProvider::default());
        provider_a.pending.store(true, Ordering::SeqCst);
        let guard_a = Arc::new(failure_guard(provider_a.clone(), None).await?);
        let provider_b = Arc::new(TokenProvider::default());
        let guard_b = failure_guard(provider_b.clone(), None).await?;
        let worker_guard = guard_a.clone();
        let worker_user = user.clone();
        let worker = tokio::spawn(async move { worker_guard.login(worker_user, true).await });
        tokio::time::timeout(Duration::from_secs(2), provider_a.entered.notified())
            .await
            .map_err(Error::internal)?;
        worker.abort();
        tokio::time::timeout(Duration::from_secs(2), shared.restore_entered.notified())
            .await
            .map_err(Error::internal)?;
        guard_b.set_user(user.clone()).await?;
        guard_b.logout().await?;
        let committed = user.remember_token()?;
        assert_eq!(
            committed,
            *provider_b.stored.lock().map_err(|_| state_error())?
        );
        guard_b.login(user.clone(), true).await?;
        {
            let (gate, signal) = &shared.restore_gate;
            *gate.lock().map_err(|_| state_error())? = true;
            signal.notify_all();
        }
        assert!(worker.await.is_err());
        assert_eq!(user.remember_token()?, committed);
        assert_eq!(guard_b.id().await?, Some(json!(42)));
        assert_eq!(
            guard_b.session.get_json(&guard_b.session_name())?,
            Some(json!(42))
        );
        assert_failed_closed(&guard_a).await?;
        Ok(())
    }
    #[tokio::test]
    async fn token_install_conflict_fails_before_provider_write() -> Result<()> {
        use std::sync::atomic::Ordering;
        let provider = Arc::new(TokenProvider::default());
        let guard = failure_guard(provider.clone(), None).await?;
        let user: AuthUser = Arc::new(RestoreBarrierUser {
            user: super::super::GenericUser::new(serde_json::Map::from_iter([(
                "id".into(),
                json!(42),
            )])),
            install_conflict: true,
            restore_entered: tokio::sync::Notify::new(),
            restore_gate: (std::sync::Mutex::new(true), std::sync::Condvar::new()),
        });
        assert!(guard.login(user.clone(), true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(user.remember_token()?.is_none());
        assert_failed_closed(&guard).await
    }
}
