//! 请求作用域的 Session 认证与记住我恢复。

use super::proof::session_factor_key;
use super::{AuthUser, Credentials, Guard, StatefulGuard, UserProvider};
use super::{FactorManagementContext, FactorReceipt, SessionFactorProof};
use crate::{
    Error, Result,
    events::Dispatcher,
    http::cookie::{CookieJar, CookieOptions},
    session::Session,
    support::BoxFuture,
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

impl crate::events::Event for Validated {
    const NAME: &'static str = r"Illuminate\Auth\Events\Validated";
}

user_event!(Authenticated, "请求内账户已确定事件。");

impl crate::events::Event for Authenticated {
    const NAME: &'static str = r"Illuminate\Auth\Events\Authenticated";
}

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

impl crate::events::Event for Attempting {
    const NAME: &'static str = r"Illuminate\Auth\Events\Attempting";
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

impl crate::events::Event for Login {
    const NAME: &'static str = r"Illuminate\Auth\Events\Login";
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

impl crate::events::Event for Failed {
    const NAME: &'static str = r"Illuminate\Auth\Events\Failed";
}

/// 注销事件。
#[derive(Clone)]
pub struct Logout {
    /// Guard 名称。
    pub guard: String,
    /// 注销前账户，访客为 None。
    pub user: Option<AuthUser>,
}

impl crate::events::Event for Logout {
    const NAME: &'static str = r"Illuminate\Auth\Events\Logout";
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
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RecallStatus {
    #[default]
    None,
    Active,
    Rejected,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct State {
    user: Option<AuthUser>,
    logged_out: bool,
    via_remember: bool,
    recalled_identity: RecallStatus,
    recall_attempted: bool,
    epoch: u64,
    persistent_user: bool,
    authentication_owner: Arc<()>,
}

/// 使用共享 Provider、请求 Session 与受信任解密 Cookie 的有状态 Guard。
///
/// 实例只能属于单个请求。传入的 recaller 必须来自框架 Cookie 解密层。
// 重哈希、Cookie保护、authority必需和确认选项是四项独立配置，不能合成互斥状态。
#[allow(clippy::struct_excessive_bools)]
pub struct SessionGuard {
    name: String,
    provider: Arc<dyn UserProvider>,
    provider_name: Option<String>,
    session: Session,
    cookies: CookieJar,
    recaller: Option<String>,
    events: Option<Dispatcher>,
    hash_key: String,
    state: RwLock<State>,
    mutation: Mutex<()>,
    rehash_on_login: bool,
    timebox: Duration,
    remember_minutes: i64,
    remember_cookie_protected: bool,
    #[cfg(feature = "auth-fortify")]
    factor_authority_required: bool,
    #[cfg(feature = "auth-fortify")]
    factor_confirm: bool,
    #[cfg(feature = "auth-fortify")]
    factor_config: Option<crate::config::repository::ConfigRepository>,
}
// 事件是可重入的公开扩展点；派发期间不得持有操作串行许可。
struct Operation<'a> {
    permit: Option<tokio::sync::MutexGuard<'a, ()>>,
    epoch: Option<u64>,
    lease_owner: Option<Arc<()>>,
}

// 只撤销本 Guard 可控制的请求/Session/Cookie 状态，不宣称数据库 token 或旧 Session 已回滚。
struct Transition<'a> {
    guard: &'a SessionGuard,
    committed: bool,
    epoch: u64,
    lease_owner: Arc<()>,
}
impl<'a> Transition<'a> {
    fn new(guard: &'a SessionGuard, operation: &mut Operation<'a>) -> Result<Self> {
        let epoch = guard.start_mutation(operation)?;
        let lease_owner = guard.read()?.authentication_owner.clone();
        guard.session.begin_authentication_transition(
            &guard.session_name(),
            &format!("password_hash_{}", guard.name),
            lease_owner.clone(),
        )?;
        operation.lease_owner = Some(lease_owner.clone());
        Ok(Self {
            guard,
            committed: false,
            epoch,
            lease_owner,
        })
    }
    fn commit(mut self) -> Result<()> {
        self.guard
            .session
            .commit_authentication_transition(&self.guard.session_name(), &self.lease_owner)?;
        self.committed = true;
        Ok(())
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
        let Ok(lease_owned) = self
            .guard
            .session
            .abort_authentication_transition(&self.guard.session_name(), &self.lease_owner)
        else {
            let retired = if state.epoch == self.epoch {
                state.logged_out = true;
                state.user.take()
            } else {
                None
            };
            drop(state);
            drop(retired);
            return;
        };
        if !lease_owned {
            let retired = if state.epoch == self.epoch {
                state.logged_out = true;
                state.user.take()
            } else {
                None
            };
            drop(state);
            drop(retired);
            return;
        }
        state.logged_out = true;
        let retired = state.user.take();
        // 原生 Cookie 队列无用户析构回调；同一 Guard 的后继操作不能插入此撤销区间。
        let _unqueued = self
            .guard
            .cookies
            .unqueue(&self.guard.recaller_name(), None);
        if self.guard.recaller.is_some()
            && let Ok(cookie) = self
                .guard
                .cookies
                .factory()
                .forget(self.guard.recaller_name(), CookieOptions::default())
        {
            let _expired = self.guard.cookies.queue(cookie);
        }
        drop(state);
        drop(retired);
        // 所有权检查与属性清理共用 Session I/O 门；禁止清理已由后继租约替换的身份。
        // busy 时仅保留上面的同步 fence，下一显式转换将完整清理候选属性。
        let _cleaned = self
            .guard
            .session
            .cleanup_authentication_transition(&self.guard.session_name(), &self.lease_owner);
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
        events: Option<Dispatcher>,
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
            #[cfg(feature = "auth-fortify")]
            factor_authority_required: false,
            #[cfg(feature = "auth-fortify")]
            factor_confirm: false,
            #[cfg(feature = "auth-fortify")]
            factor_config: None,
        }
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn with_factor_authority_policy(mut self, required: bool, confirm: bool) -> Self {
        self.factor_authority_required = required;
        self.factor_confirm = confirm;
        self
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn with_factor_config(
        mut self,
        config: crate::config::repository::ConfigRepository,
    ) -> Self {
        self.factor_config = Some(config);
        self
    }
    #[cfg(feature = "auth-fortify")]
    fn current_factor_confirmation(&self) -> Result<bool> {
        self.factor_config.as_ref().map_or(
            Ok(self.factor_confirm),
            crate::auth::fortify::configured_confirmation,
        )
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
            lease_owner: None,
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
        // 后继转换拥有新的结果；不能让旧策略拒绝原因遮蔽这次取消或未知失败。
        state.recalled_identity = RecallStatus::None;
        state.authentication_owner = Arc::new(());
        operation.epoch = Some(state.epoch);
        Ok(state.epoch)
    }
    fn owns_operation(&self, operation: &Operation<'_>) -> Result<bool> {
        let Some(epoch) = operation.epoch else {
            return Ok(true);
        };
        if self.read()?.epoch != epoch {
            return Ok(false);
        }
        if let Some(owner) = &operation.lease_owner {
            return self
                .session
                .owns_authentication_transition(&self.session_name(), owner);
        }
        Ok(true)
    }
    async fn emit<'a, E: crate::events::Event>(
        &'a self,
        operation: &mut Operation<'a>,
        event: E,
    ) -> Result<()> {
        drop(operation.permit.take());
        let result = if let Some(events) = &self.events {
            events.dispatch(&event).await.map(|_| ())
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
    fn read_factor_proof(&self) -> Result<Option<SessionFactorProof>> {
        self.session
            .get_json(&session_factor_key(&self.session_name()))?
            .map(SessionFactorProof::from_value)
            .transpose()
    }

    fn retain_matching_factor_proof(&self, user: &AuthUser) -> Result<Option<SessionFactorProof>> {
        let key = session_factor_key(&self.session_name());
        let Some(raw) = self.session.get_json(&key)? else {
            return Ok(None);
        };
        let identifier = user.auth_identifier()?;
        let proof = SessionFactorProof::from_value(raw).ok();
        if let Some(proof) = proof.filter(|proof| {
            proof.matches_identity(&identifier, &self.name, self.provider_name.as_deref())
        }) {
            return Ok(Some(proof));
        }
        self.session.remove(&key)?;
        Ok(None)
    }
    async fn update_session(&self, user: &AuthUser) -> Result<()> {
        self.session.require_authenticated_storage()?;
        // 轮换失败或 future 被取消时，本请求保持拒绝认证；不宣称存储已回滚。
        {
            let mut state = self.write()?;
            state.user = None;
            state.logged_out = true;
        }
        self.session
            .remove(&session_factor_key(&self.session_name()))?;
        self.session.put_typed(
            &self.session_name(),
            valid_identifier(user.auth_identifier()?)?,
        )?;
        self.session
            .mark_guard_authentication(&self.session_name(), true)?;
        self.session.regenerate(true).await?;
        #[cfg(feature = "auth-fortify")]
        self.session
            .record_new_credential_login(&self.session_name())?;
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
        if !persistent {
            self.session
                .remove(&session_factor_key(&self.session_name()))?;
        }
        {
            let mut state = self.write()?;
            state.user = Some(user.clone());
            state.persistent_user = persistent;
            state.recalled_identity = RecallStatus::None;
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
        transition.commit()?;
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
    // Session恢复与recaller恢复共用epoch/lease，保留事件重入后重核及取消清理的可审查顺序。
    #[allow(clippy::too_many_lines)]
    async fn user_inner<'a>(&'a self, operation: &mut Operation<'a>) -> Result<Option<AuthUser>> {
        let cached = {
            let state = self.read()?;
            if state.logged_out {
                return Ok(None);
            }
            state.user.clone().map(|user| {
                (
                    user,
                    state.persistent_user,
                    state.recalled_identity,
                    state.epoch,
                    state.authentication_owner.clone(),
                )
            })
        };
        if let Some((user, persistent, recalled, epoch, owner)) = cached {
            if persistent {
                self.session.require_authenticated_storage()?;
            }
            valid_identifier(user.auth_identifier()?)?;
            // 记住我恢复后的缓存身份仍须满足当前因子策略；普通密码Session保持原管理语义。
            if recalled == RecallStatus::Active && operation.epoch.is_none() {
                let proof = self.read_factor_proof()?;
                let validation = self
                    .validate_recalled_factor_proof(&user, proof.as_ref())
                    .await;
                if self.read()?.epoch != epoch {
                    return self.current_user();
                }
                if !self
                    .session
                    .owns_authentication_transition(&self.session_name(), &owner)?
                {
                    return Ok(None);
                }
                if !matches!(&validation, Ok(true)) {
                    if matches!(&validation, Ok(false)) {
                        self.write()?.recalled_identity = RecallStatus::Rejected;
                    }
                    // 只撤销本次缓存身份原有的租约；不新建租约抢占另一Guard的后继登录。
                    drop(Transition {
                        guard: self,
                        committed: false,
                        epoch,
                        lease_owner: owner,
                    });
                    validation?;
                    return Ok(None);
                }
            }
            return Ok(Some(user));
        }
        self.session.require_authenticated_storage()?;
        if let Some(id) = self.session.get_json(&self.session_name())? {
            valid_identifier(id.clone())?;
            if let Some(user) = self.provider.retrieve_by_id(&id).await? {
                valid_identifier(user.auth_identifier()?)?;
                let transition = Transition::new(self, operation)?;
                self.retain_matching_factor_proof(&user)?;
                self.session
                    .mark_guard_authentication(&self.session_name(), true)?;
                {
                    let mut state = self.write()?;
                    state.user = Some(user.clone());
                    state.persistent_user = true;
                    state.recalled_identity = RecallStatus::None;
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
                    drop(transition);
                    return self.current_user();
                }
                emitted?;
                let current = self.current_user()?;
                transition.commit()?;
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
        if !self
            .validate_recalled_factor_proof(&user, recaller.factor_proof.as_ref())
            .await?
        {
            return Ok(None);
        }
        let transition = Transition::new(self, operation)?;
        self.update_session(&user).await?;
        if let Some(proof) = recaller.factor_proof {
            self.session
                .put_typed(&session_factor_key(&self.session_name()), proof)?;
        }
        {
            let mut state = self.write()?;
            state.user = Some(user.clone());
            state.persistent_user = true;
            state.recalled_identity = RecallStatus::Active;
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
            drop(transition);
            return self.current_user();
        }
        emitted?;
        let proof = self.read_factor_proof()?;
        let valid = self
            .validate_recalled_factor_proof(&user, proof.as_ref())
            .await?;
        if !self.owns_operation(operation)? {
            drop(transition);
            return self.current_user();
        }
        if !valid {
            self.write()?.recalled_identity = RecallStatus::Rejected;
            drop(transition);
            return Ok(None);
        }
        let current = self.current_user()?;
        transition.commit()?;
        Ok(current)
    }
    // 只有Fortify分支需要await主库authority；关闭feature仍保留同一严格拒绝接口。
    #[cfg_attr(
        not(feature = "auth-fortify"),
        allow(clippy::unused_async, clippy::unused_async_trait_impl)
    )]
    async fn validate_recalled_factor_proof(
        &self,
        user: &AuthUser,
        proof: Option<&SessionFactorProof>,
    ) -> Result<bool> {
        if let Some(proof) = proof
            && !proof.matches_identity(
                &user.auth_identifier()?,
                &self.name,
                self.provider_name.as_deref(),
            )
        {
            return Ok(false);
        }
        #[cfg(feature = "auth-fortify")]
        {
            let Some(model) = user.as_two_factor() else {
                return Ok(!self.factor_authority_required && proof.is_none());
            };
            let Some(authority) = model.two_factor_authority() else {
                return Ok(!self.factor_authority_required && proof.is_none());
            };
            authority
                .validate_account(user, self.provider.as_ref())
                .await?;
            let snapshot = authority.load().await?;
            Ok(match proof {
                Some(proof) => proof.matches(
                    &user.auth_identifier()?,
                    &self.name,
                    self.provider_name.as_deref(),
                    &snapshot.source,
                    &snapshot.generation,
                ),
                None => !snapshot.challenge_required(self.current_factor_confirmation()?),
            })
        }
        #[cfg(not(feature = "auth-fortify"))]
        {
            Ok(proof.is_none())
        }
    }
    async fn cycle_token(&self, operation: &Operation<'_>, user: &AuthUser) -> Result<()> {
        operation.epoch.ok_or_else(state_changed)?;
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
        let Some(mut update) = user
            .begin_remember_token_update(previous.as_deref(), Some(token.clone()))
            .await?
        else {
            return Err(Error::custom_with_status(
                http::StatusCode::CONFLICT,
                "auth_remember_token_conflict",
                "账户记住我 token 已由并发操作修改",
            ));
        };
        // 新增的异步setter边界之后仍需持有本次Guard转换；返回时收据负责恢复。
        if !self.owns_operation(operation)? {
            return Err(state_changed());
        }
        if let Err(error) = self
            .provider
            .update_remember_token(user.as_ref(), &token)
            .await
        {
            update.rollback()?;
            return Err(error);
        }
        update.commit()?;
        Ok(())
    }
    async fn login_inner<'a>(
        &'a self,
        operation: &mut Operation<'a>,
        user: AuthUser,
        remember: bool,
        extra_regeneration: bool,
        factor_proof: Option<SessionFactorProof>,
    ) -> Result<()> {
        let transition = Transition::new(self, operation)?;
        self.update_session(&user).await?;
        if let Some(proof) = &factor_proof {
            if !proof.matches_identity(
                &user.auth_identifier()?,
                &self.name,
                self.provider_name.as_deref(),
            ) {
                return Err(Error::custom(
                    "auth_factor_receipt_mismatch",
                    "因子收据与当前 Guard 身份不匹配",
                ));
            }
            self.session
                .put_typed(&session_factor_key(&self.session_name()), proof)?;
        }
        if let Some(password) = user.auth_password()?.filter(|p| !p.is_empty()) {
            self.session.put_typed(
                &format!("password_hash_{}", self.name),
                self.hash_password_for_cookie(Some(&password))?,
            )?;
        }
        if remember {
            self.ensure_remember_cookie_protected()?;
            if user
                .remember_token_for_cookie()?
                .is_none_or(|token| token.is_empty() || token == "0")
            {
                self.cycle_token(operation, &user).await?;
            }
            let identifier = recaller_identifier(user.auth_identifier()?)?;
            let token = user
                .remember_token_for_cookie()?
                .filter(|token| !token.trim().is_empty() && token != "0" && !token.contains('|'))
                .ok_or_else(|| {
                    Error::custom_with_status(
                        http::StatusCode::CONFLICT,
                        "auth_remember_token_conflict",
                        "记住我token在Cookie签发前已变化或不可用",
                    )
                })?;
            let mut value = format!(
                "{identifier}|{token}|{}",
                self.hash_password_for_cookie(user.auth_password()?.as_deref())?
            );
            if let Some(proof) = &factor_proof {
                let json = serde_json::to_vec(proof).map_err(Error::internal)?;
                if json.len() > RECALLER_PROOF_LIMIT {
                    return Err(Error::custom(
                        "auth_recaller_proof_too_large",
                        "记住我因子证明超过输入界限",
                    ));
                }
                value.push('|');
                value.push_str(RECALLER_PROOF_PREFIX);
                value.push_str(&URL_SAFE_NO_PAD.encode(json));
            }
            // 解析器按完整明文载荷限长，必须包含身份、哈希及可选因子证明。
            if value.len() > RECALLER_LIMIT {
                return Err(Error::custom(
                    "auth_recaller_too_large",
                    "记住我Cookie载荷超过输入界限",
                ));
            }
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
            state.recalled_identity = RecallStatus::None;
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
        if extra_regeneration {
            // Fortify PrepareAuthenticatedSession / challenge controller 的第二次轮换。
            // 同一个 Transition 在此完成前不提交，错误或取消仍撤销本操作身份。
            self.session.regenerate(false).await?;
            #[cfg(feature = "auth-fortify")]
            self.session
                .record_new_credential_login(&self.session_name())?;
            self.session.require_authenticated_storage()?;
            if !self.owns_operation(operation)? {
                return Err(state_changed());
            }
        }
        transition.commit()
    }
    async fn attempt_inner<'a>(
        &'a self,
        operation: &mut Operation<'a>,
        credentials: &Credentials,
        remember: bool,
        extra_regeneration: bool,
    ) -> Result<bool> {
        self.start_mutation(operation)?;
        let start = tokio::time::Instant::now();
        self.emit(
            operation,
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
                    operation,
                    Validated {
                        guard: self.name.clone(),
                        user: user.clone(),
                    },
                )
                .await?;
                self.rehash(user, credentials).await?;
                self.login_inner(operation, user.clone(), remember, extra_regeneration, None)
                    .await?;
                return Ok(true);
            }
        }
        self.emit(
            operation,
            Failed {
                guard: self.name.clone(),
                user,
                credentials: credentials.clone(),
            },
        )
        .await?;
        tokio::time::sleep_until(start + self.timebox).await;
        Ok(false)
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
    fn supports_session_regeneration_login(&self) -> bool {
        true
    }
    fn session_name(&self) -> Option<String> {
        Some(SessionGuard::session_name(self))
    }
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_session(&self) -> Option<(&str, crate::session::Session)> {
        Some((&self.name, self.session.clone()))
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
            self.attempt_inner(&mut operation, credentials, remember, false)
                .await
        })
    }
    fn attempt_with_session_regeneration<'a>(
        &'a self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.attempt_inner(&mut operation, credentials, remember, true)
                .await
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
            self.login_inner(&mut operation, user, remember, false, None)
                .await
        })
    }
    fn login_with_session_regeneration(
        &self,
        user: AuthUser,
        remember: bool,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            self.login_inner(&mut operation, user, remember, true, None)
                .await
        })
    }
    fn login_with_factor_receipt(
        &self,
        user: AuthUser,
        remember: bool,
        receipt: FactorReceipt,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let proof = receipt.into_proof();
            if !proof.matches_identity(
                &user.auth_identifier()?,
                &self.name,
                self.provider_name.as_deref(),
            ) {
                return Err(Error::custom(
                    "auth_factor_receipt_mismatch",
                    "因子收据与当前 Guard 身份不匹配",
                ));
            }
            let mut operation = self.operation().await;
            self.login_inner(&mut operation, user, remember, true, Some(proof))
                .await
        })
    }
    fn factor_management_context(&self) -> BoxFuture<'_, Result<FactorManagementContext>> {
        Box::pin(async move {
            use sha2_11::Digest as _;
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut operation = self.operation().await;
            if !self
                .session
                .authentication_transition_committed(&self.session_name())?
            {
                return Err(state_changed());
            }
            let user = self
                .user_inner(&mut operation)
                .await?
                .ok_or(Error::Unauthorized)?;
            self.session.require_authenticated_storage()?;
            let identifier = user.auth_identifier()?;
            if !self.read()?.persistent_user
                || !self
                    .session
                    .authentication_transition_committed(&self.session_name())?
                || self.session.get_json(&self.session_name())? != Some(identifier.clone())
            {
                return Err(Error::Unauthorized);
            }
            let binding = serde_json::to_string(&(
                self.session.id()?,
                &self.name,
                self.provider_name.as_deref(),
                &identifier,
            ))
            .map_err(Error::internal)?;
            let mut session_binding = String::with_capacity(64);
            for byte in Sha256::digest(binding.as_bytes()) {
                session_binding.push(char::from(HEX[usize::from(byte >> 4)]));
                session_binding.push(char::from(HEX[usize::from(byte & 15)]));
            }
            FactorManagementContext::issue(
                identifier,
                self.name.clone(),
                self.provider_name.clone(),
                session_binding,
                self.retain_matching_factor_proof(&user)?,
            )
        })
    }
    fn attest_factor_receipt(&self, receipt: FactorReceipt) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            if !self
                .session
                .authentication_transition_committed(&self.session_name())?
            {
                return Err(state_changed());
            }
            let user = self
                .user_inner(&mut operation)
                .await?
                .ok_or(Error::Unauthorized)?;
            self.session.require_authenticated_storage()?;
            if !self.read()?.persistent_user
                || !self
                    .session
                    .authentication_transition_committed(&self.session_name())?
                || self.session.get_json(&self.session_name())? != Some(user.auth_identifier()?)
            {
                return Err(Error::Unauthorized);
            }
            let proof = receipt.into_proof();
            if !proof.matches_identity(
                &user.auth_identifier()?,
                &self.name,
                self.provider_name.as_deref(),
            ) {
                return Err(Error::custom(
                    "auth_factor_receipt_mismatch",
                    "因子收据与当前Guard身份不匹配",
                ));
            }
            let transition = Transition::new(self, &mut operation)?;
            self.session
                .put_typed(&session_factor_key(&self.session_name()), proof)?;
            transition.commit()
        })
    }
    fn session_factor_proof(&self) -> BoxFuture<'_, Result<Option<SessionFactorProof>>> {
        Box::pin(async move {
            let mut operation = self.operation().await;
            // 只有明确的策略拒绝返回无证明；取消或未知失败继续保留原fence错误。
            {
                let state = self.read()?;
                if state.logged_out && state.recalled_identity == RecallStatus::Rejected {
                    return Ok(None);
                }
            }
            if !self
                .session
                .authentication_transition_committed(&self.session_name())?
            {
                return Ok(None);
            }
            let Some(user) = self.user_inner(&mut operation).await? else {
                return Ok(None);
            };
            if !self.read()?.persistent_user {
                return Ok(None);
            }
            self.session.require_authenticated_storage()?;
            if !self
                .session
                .authentication_transition_committed(&self.session_name())?
            {
                return Ok(None);
            }
            let Some(identifier) = self.session.get_json(&self.session_name())? else {
                return Ok(None);
            };
            if identifier != user.auth_identifier()? {
                return Ok(None);
            }
            self.retain_matching_factor_proof(&user)
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
                self.login_inner(&mut operation, user.clone(), remember, false, None)
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
            self.session
                .remove(&session_factor_key(&self.session_name()))?;
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
            transition.commit()?;
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
const RECALLER_PROOF_LIMIT: usize = 4_096;
const RECALLER_PROOF_PREFIX: &str = "~or-factor-v1~";
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
    // 常规标识保留原记住我格式；分隔符与保留前缀使用有界、带类型的版本编码。
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
    factor_proof: Option<SessionFactorProof>,
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
        let factor_proof = match segments.next() {
            Some(segment) if segment.starts_with("~or-factor-") => {
                let encoded = segment.strip_prefix(RECALLER_PROOF_PREFIX)?;
                if encoded.len() > RECALLER_PROOF_LIMIT * 4 / 3 + 4 {
                    return None;
                }
                let json = URL_SAFE_NO_PAD.decode(encoded).ok()?;
                if json.len() > RECALLER_PROOF_LIMIT || URL_SAFE_NO_PAD.encode(&json) != encoded {
                    return None;
                }
                Some(SessionFactorProof::from_value(serde_json::from_slice(&json).ok()?).ok()?)
            }
            _ => None,
        };
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
            factor_proof,
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
#[path = "../../../tests/unit/auth/guard_session.rs"]
mod tests;
