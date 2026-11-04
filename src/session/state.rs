//! 唯一请求状态：同步属性锁不跨await，I/O门控由取消可回收的guard持有。

use crate::support::{DataValue, ObjectEncoding};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    collections::HashSet,
    sync::{Arc, Condvar, Mutex, MutexGuard},
};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;

use super::{
    DEFAULT_MAX_BYTES, SessionId, SessionRecord, SessionRequestContext, SessionStore,
    SessionStoreSemantics, SessionWriteMode, SessionWriteOutcome,
};
use super::{
    attributes::Attributes,
    id::random_token,
    store::{invalid_record, limit_error},
};
use crate::{Error, Result};

/// 会话记录的截止策略；浏览器是否在关闭时删Cookie由最终Session配置决定。
#[derive(Debug, Clone, Copy)]
pub enum Expiry {
    /// 进程内显式Session使用默认120分钟；此项不暗改Provider的Cookie策略。
    OnSessionEnd,
    /// 从每次保存开始重新计算的活动期限。
    OnInactivity(Duration),
    /// 固定截止时间。
    AtDateTime(OffsetDateTime),
}

impl Expiry {
    fn date(self) -> Result<OffsetDateTime> {
        match self {
            Self::AtDateTime(value) => Ok(value),
            Self::OnSessionEnd => OffsetDateTime::now_utc()
                .checked_add(Duration::minutes(120))
                .ok_or_else(invalid_record),
            Self::OnInactivity(value) => OffsetDateTime::now_utc()
                .checked_add(value)
                .ok_or_else(invalid_record),
        }
    }
}

#[derive(Clone)]
struct Driver {
    identity: Arc<dyn SessionStore>,
    actual: Arc<dyn SessionStore>,
    needs_request: bool,
    bound: bool,
    accepts_request: bool,
    semantics: SessionStoreSemantics,
    activity_lifetime: Option<i64>,
}
impl Driver {
    fn new(store: Arc<dyn SessionStore>) -> Result<Self> {
        let needs_request = store.needs_request();
        let actual = store.for_session()?.unwrap_or_else(|| store.clone());
        if actual.guarantees() != store.guarantees() || actual.semantics() != store.semantics() {
            return Err(invalid_record());
        }
        let accepts_request = store.accepts_request();
        let semantics = store.semantics();
        let activity_lifetime = store.activity_lifetime_seconds();
        Ok(Self {
            identity: store,
            actual,
            needs_request,
            bound: !needs_request,
            accepts_request,
            semantics,
            activity_lifetime,
        })
    }
    #[cfg(feature = "auth")]
    fn require_authenticated(&self, name: &str) -> Result<()> {
        if !self.ready()?.supports_session_authentication(name) {
            return Err(extension_error(
                "session_cookie_protection_required",
                "Session 认证需要经过完整性验证的 Cookie 存储",
            ));
        }
        Ok(())
    }

    fn ready(&self) -> Result<Arc<dyn SessionStore>> {
        if !self.bound {
            return Err(extension_error(
                "session_request_unbound",
                "Session handler requires a request",
            ));
        }
        Ok(self.actual.clone())
    }
}
#[derive(Clone)]
struct Binding {
    id: SessionId,
    driver: Driver,
}
impl Binding {
    fn matches(&self, id: &SessionId, driver: &Driver) -> bool {
        self.id == *id && Arc::ptr_eq(&self.driver.identity, &driver.identity)
    }
}
struct PreparedSave {
    record: SessionRecord,
    mode: SessionWriteMode,
    driver: Driver,
    origin: Option<Binding>,
    intent: CreateIntent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateIntent {
    SystemRandom,
    Explicit,
}

#[allow(clippy::struct_excessive_bools, reason = "独立生命周期标记可并存")]
struct State {
    id: SessionId,
    name: String,
    attributes: Attributes,
    object_encoding: ObjectEncoding,
    #[cfg(feature = "view")]
    view_errors: Option<Arc<crate::view::RequestViewErrors>>,
    #[cfg(feature = "view")]
    errors_epoch: Arc<()>,
    #[cfg(feature = "view")]
    errors_restoring: bool,
    expiry: Option<Expiry>,
    started: bool,
    existing: bool,
    busy: bool,
    sync_owner: Option<std::thread::ThreadId>,
    revoked: bool,
    closed: bool,
    driver: Driver,
    origin: Option<Binding>,
    intent: CreateIntent,
    destroyed: HashSet<SessionId>,
}
struct Inner {
    state: Mutex<State>,
    io: AsyncMutex<()>,
    sync_idle: Condvar,
    #[cfg(test)]
    sync_waiters: AtomicUsize,
    max_bytes: usize,
    #[cfg(feature = "auth")]
    auth_scope: std::sync::OnceLock<std::sync::Weak<crate::core::container::Scope>>,
    error_bag_codecs: Arc<super::ErrorBagCodecs>,
}

#[cfg(feature = "view")]
struct RestoringErrors<'a>(&'a Inner);
#[cfg(feature = "view")]
impl Drop for RestoringErrors<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.errors_restoring = false;
        }
    }
}

/// 同一请求所有提取器与鉴权共享的Session句柄。
///
/// 属性方法不执行I/O；`start`/`save`等需显式await。I/O进行时同步写入返回
/// `session_operation_in_progress`，不会阻塞执行线程或悄悄丢失并发修改。
///
/// ```
/// use openrust::session::{MemorySessionStore, Session};
/// use std::sync::Arc;
/// # fn main() -> openrust::Result<()> {
/// # tokio::runtime::Runtime::new().map_err(openrust::Error::internal)?.block_on(async {
/// let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
/// session.start().await?;
/// session.put_typed("cart.quantity", 2)?;
/// session.flash_typed("notice", "已保存")?;
/// assert_eq!(session.get_typed::<u64>("cart.quantity")?, Some(2));
/// session.save().await?;
/// # Ok(())
/// # })
/// # }
/// ```
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session([REDACTED])")
    }
}

impl Session {
    /// 构造即有40字符ID，尚未读store；None使用系统随机源生成。
    pub fn new(
        id: Option<SessionId>,
        store: Arc<dyn SessionStore>,
        expiry: Option<Expiry>,
    ) -> Result<Self> {
        Self::with_options(
            id,
            store,
            expiry,
            DEFAULT_MAX_BYTES,
            "openrust_session".into(),
        )
    }

    pub(crate) fn with_options(
        id: Option<SessionId>,
        store: Arc<dyn SessionStore>,
        expiry: Option<Expiry>,
        max_bytes: usize,
        name: String,
    ) -> Result<Self> {
        Self::with_options_and_codecs(
            id,
            store,
            expiry,
            max_bytes,
            name,
            Arc::new(super::ErrorBagCodecs::new()),
        )
    }

    pub(crate) fn with_options_and_codecs(
        id: Option<SessionId>,
        store: Arc<dyn SessionStore>,
        expiry: Option<Expiry>,
        max_bytes: usize,
        name: String,
        error_bag_codecs: Arc<super::ErrorBagCodecs>,
    ) -> Result<Self> {
        if max_bytes == 0 || max_bytes > DEFAULT_MAX_BYTES {
            return Err(limit_error());
        }
        let id = match id {
            Some(id) => id,
            None => SessionId::generate()?,
        };
        let driver = Driver::new(store)?;
        let expiry = if let Some(lifetime) = driver.activity_lifetime {
            match expiry {
                None | Some(Expiry::OnSessionEnd) => {
                    Some(Expiry::OnInactivity(Duration::seconds(lifetime)))
                }
                Some(Expiry::OnInactivity(value)) if value == Duration::seconds(lifetime) => expiry,
                _ => {
                    return Err(extension_error(
                        "session_expiry_unsupported",
                        "原生数据库活动期限来自最终配置，其他截止须选择增强 driver",
                    ));
                }
            }
        } else {
            expiry
        };
        Ok(Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    id,
                    name,
                    attributes: Attributes::new(Value::Array(Vec::new()))?,
                    object_encoding: ObjectEncoding::Reject,
                    #[cfg(feature = "view")]
                    view_errors: None,
                    #[cfg(feature = "view")]
                    errors_epoch: Arc::new(()),
                    #[cfg(feature = "view")]
                    errors_restoring: false,
                    expiry,
                    started: false,
                    existing: false,
                    busy: false,
                    sync_owner: None,
                    revoked: false,
                    closed: false,
                    driver,
                    origin: None,
                    intent: CreateIntent::SystemRandom,
                    destroyed: HashSet::new(),
                }),
                io: AsyncMutex::new(()),
                sync_idle: Condvar::new(),
                #[cfg(test)]
                sync_waiters: AtomicUsize::new(0),
                max_bytes,
                error_bag_codecs,
                #[cfg(feature = "auth")]
                auth_scope: std::sync::OnceLock::new(),
            }),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.inner.state.lock().map_err(|_| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_state_unavailable",
                "Session state is unavailable",
            )
        })
    }
    fn mutate(&self, apply: impl FnOnce(&mut Attributes) -> Result<()>) -> Result<&Self> {
        self.mutate_value(apply)?;
        Ok(self)
    }
    fn mutate_value<T>(&self, apply: impl FnOnce(&mut Attributes) -> Result<T>) -> Result<T> {
        self.mutate_errors(false, apply)
    }
    fn mutate_errors<T>(
        &self,
        replace_errors: bool,
        apply: impl FnOnce(&mut Attributes) -> Result<T>,
    ) -> Result<T> {
        // 回收槽须早于门控创建；失败、成功及 unwind 都先释放门控。
        let mut candidate = None;
        let mut retired = None;
        let mut returned = None;
        #[cfg(feature = "view")]
        let mut retired_errors = None;
        let operation = self.write_operation()?;
        let outcome = (|| -> Result<()> {
            let (snapshot, id, expiry, semantics) = {
                let state = self.lock()?;
                (
                    state.attributes.clone(),
                    state.id.clone(),
                    state.expiry,
                    state.driver.semantics,
                )
            };
            candidate = Some(snapshot);
            let current = candidate.as_mut().ok_or_else(invalid_record)?;
            returned = Some(apply(current)?);
            SessionRecord {
                id,
                data: current.budget_json()?,
                expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
            }
            .validate_for(semantics, self.inner.max_bytes)?;
            let mut state = self.lock()?;
            state.ensure_live()?;
            #[cfg(feature = "view")]
            {
                retired_errors = state.retire_view_errors(current, replace_errors)?;
            }
            #[cfg(not(feature = "view"))]
            let _ = replace_errors;
            let published = candidate.take().ok_or_else(invalid_record)?;
            retired = Some(std::mem::replace(&mut state.attributes, published));
            Ok(())
        })();
        drop(operation);
        drop(candidate);
        drop(retired);
        #[cfg(feature = "view")]
        drop(retired_errors);
        outcome?;
        returned.ok_or_else(invalid_record)
    }
    async fn operation(&self) -> Result<Operation<'_>> {
        let gate = self.inner.io.lock().await;
        {
            let mut state = self.lock()?;
            state.ensure_mutable()?;
            state.busy = true;
        }
        Ok(Operation {
            inner: &self.inner,
            gate: Some(gate),
        })
    }
    fn try_operation(&self) -> Result<Operation<'_>> {
        let gate = self.inner.io.try_lock().map_err(|_| {
            extension_error(
                "session_operation_in_progress",
                "Session I/O is in progress",
            )
        })?;
        {
            let mut state = self.lock()?;
            state.ensure_mutable()?;
            state.busy = true;
        }
        Ok(Operation {
            inner: &self.inner,
            gate: Some(gate),
        })
    }
    fn write_operation(&self) -> Result<Operation<'_>> {
        let current = std::thread::current().id();
        let mut state = self.lock()?;
        loop {
            state.ensure_live()?;
            if state.busy {
                if state.sync_owner.is_some_and(|owner| owner != current) {
                    #[cfg(test)]
                    self.inner.sync_waiters.fetch_add(1, Ordering::SeqCst);
                    let waited = self.inner.sync_idle.wait(state);
                    #[cfg(test)]
                    self.inner.sync_waiters.fetch_sub(1, Ordering::SeqCst);
                    state = waited.map_err(|_| {
                        extension_error("session_state_unavailable", "Session state is unavailable")
                    })?;
                    continue;
                }
                return Err(extension_error(
                    "session_operation_in_progress",
                    "Session I/O is in progress",
                ));
            }
            // state 锁内只尝试取得 I/O 门；成功时同步 owner 与 busy 原子登记。
            let gate = self.inner.io.try_lock().map_err(|_| {
                extension_error(
                    "session_operation_in_progress",
                    "Session I/O is in progress",
                )
            })?;
            state.busy = true;
            state.sync_owner = Some(current);
            drop(state);
            return Ok(Operation {
                inner: &self.inner,
                gate: Some(gate),
            });
        }
    }

    /// 当前ID在start/save前已可见；调用方不得写日志。
    pub fn id(&self) -> Result<SessionId> {
        Ok(self.lock()?.id.clone())
    }
    /// 当前Cookie名称。
    pub fn name(&self) -> Result<String> {
        Ok(self.lock()?.name.clone())
    }
    /// 是否已start且尚未完成一次save。
    pub fn is_started(&self) -> Result<bool> {
        Ok(self.lock()?.started)
    }
    /// 改变本请求的出站 Cookie 名称；不会自动清除旧名 Cookie 或改变 ID。
    pub fn set_name(&self, name: impl Into<String>) -> Result<&Self> {
        self.lock()?.ensure_mutable()?;
        let name = name.into();
        crate::http::CookieFactory::default()
            .make(
                name.clone(),
                self.id()?.to_string(),
                0,
                crate::http::CookieOptions::default(),
            )?
            .to_header(false)?;
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        state.name = name;
        Ok(self)
    }
    /// 即时替换 ID；非法或缺失输入生成随机 ID，不加载目标或清除属性。
    pub fn set_id(&self, id: Option<&str>) -> Result<&Self> {
        self.lock()?.ensure_mutable()?;
        let (id, intent) = match id.and_then(|id| SessionId::parse(id).ok()) {
            Some(id) => (id, CreateIntent::Explicit),
            None => (SessionId::generate()?, CreateIntent::SystemRandom),
        };
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        if state.driver.semantics == SessionStoreSemantics::Guarded && state.destroyed.contains(&id)
        {
            return Err(write_conflict());
        }
        if state.id != id {
            state.id = id;
            state.intent = intent;
            state.restore_proof();
        } else if intent == CreateIntent::Explicit {
            state.intent = CreateIntent::Explicit;
        }
        Ok(self)
    }

    /// 替换并返回配置驱动；保留属性和 started，不自动注入请求。
    pub fn set_handler(&self, handler: Arc<dyn SessionStore>) -> Result<Arc<dyn SessionStore>> {
        let semantics = handler.semantics();
        {
            let state = self.lock()?;
            state.ensure_mutable()?;
            if Arc::ptr_eq(&state.driver.identity, &handler)
                || Arc::ptr_eq(&state.driver.actual, &handler)
            {
                return Ok(handler);
            }
            if semantics != state.driver.semantics && state.has_transfer_data()? {
                return Err(extension_error(
                    "session_transfer_unsupported",
                    "已有会话不能跨原生与增强语义转移期限及撤销授权",
                ));
            }
        }
        // 用户驱动代码和旧 Arc 的最终释放均不在属性锁内。
        let driver = Driver::new(handler.clone())?;
        let old = {
            let mut state = self.lock()?;
            state.ensure_mutable()?;
            if driver.semantics != state.driver.semantics && state.has_transfer_data()? {
                return Err(extension_error(
                    "session_transfer_unsupported",
                    "已有会话不能跨原生与增强语义转移期限及撤销授权",
                ));
            }
            let old = std::mem::replace(&mut state.driver, driver);
            if let Some(lifetime) = state.driver.activity_lifetime {
                state.expiry = Some(Expiry::OnInactivity(Duration::seconds(lifetime)));
            }
            state.intent = CreateIntent::Explicit;
            state.restore_proof();
            old
        };
        drop(old);
        Ok(handler)
    }

    /// 返回当前实际 I/O 驱动；直接调用不具有受管理状态机的保证。
    pub fn handler(&self) -> Result<Arc<dyn SessionStore>> {
        Ok(self.lock()?.driver.actual.clone())
    }

    #[cfg(feature = "auth")]
    pub(crate) fn bind_auth_scope(&self, scope: &Arc<crate::core::container::Scope>) -> Result<()> {
        self.inner
            .auth_scope
            .set(Arc::downgrade(scope))
            .map_err(|_| extension_error("session_auth_scope_bound", "Session认证请求scope已绑定"))
    }

    #[cfg(feature = "auth")]
    async fn project_guard_identity(&self) -> Result<()> {
        if self.lock()?.driver.semantics != SessionStoreSemantics::NativeDatabase {
            return Ok(());
        }
        let Some(scope) = self.inner.auth_scope.get() else {
            return Ok(());
        };
        let scope = scope.upgrade().ok_or_else(|| {
            extension_error("session_auth_scope_closed", "Session认证请求scope已释放")
        })?;
        if let Some(id) = crate::auth::session_user_id(&scope).await? {
            let driver = self.lock()?.driver.clone();
            if driver.semantics == SessionStoreSemantics::NativeDatabase
                && !driver.ready()?.set_database_user_id(id)?
            {
                return Err(extension_error(
                    "session_auth_metadata_unsupported",
                    "数据库Session驱动未实现Guard身份元数据投影",
                ));
            }
        }
        Ok(())
    }

    #[cfg(feature = "auth")]
    pub(crate) fn require_authenticated_storage(&self) -> Result<()> {
        let (driver, name) = {
            let state = self.lock()?;
            state.ensure_mutable()?;
            (state.driver.clone(), state.name.clone())
        };
        driver.require_authenticated(&name)
    }

    #[cfg(feature = "auth")]
    pub(crate) fn mark_guard_authentication(&self, key: &str, authenticated: bool) -> Result<()> {
        // 与属性变更共用同一短锁，多个命名 Guard 不覆盖彼此的保存保护。
        self.mutate(|attributes| {
            let mut keys = match attributes.get("_openrust_guard_bindings")? {
                None => Vec::new(),
                Some(Value::Array(values)) if values.iter().all(Value::is_string) => values,
                Some(_) => return Err(invalid_record()),
            };
            keys.retain(|value| value.as_str() != Some(key));
            if authenticated {
                keys.push(Value::String(key.to_owned()));
            }
            if keys.is_empty() {
                attributes.forget(&["_openrust_guard_bindings"])
            } else {
                attributes.put("_openrust_guard_bindings", Value::Array(keys))
            }
        })?;
        Ok(())
    }

    /// 当前配置驱动是否需要请求上下文。
    pub fn handler_needs_request(&self) -> Result<bool> {
        Ok(self.lock()?.driver.needs_request)
    }

    pub(crate) fn handler_accepts_request(&self) -> Result<bool> {
        Ok(self.lock()?.driver.accepts_request)
    }

    /// 在 I/O 门控下创建请求内实例；普通驱动不调用绑定回调。
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "公开绑定生命周期使用异步接口，但忙碌时必须立即拒绝而不能等待"
    )]
    pub async fn set_request_on_handler(&self, context: SessionRequestContext) -> Result<&Self> {
        let _operation = self.try_operation()?;
        let driver = self.lock()?.driver.clone();
        if !driver.accepts_request {
            return Ok(self);
        }
        let actual = driver.identity.bind_request(context)?.ok_or_else(|| {
            extension_error(
                "session_request_binding_invalid",
                "Session handler did not bind the request",
            )
        })?;
        if actual.guarantees() != driver.identity.guarantees()
            || actual.semantics() != driver.semantics
        {
            return Err(extension_error(
                "session_request_binding_invalid",
                "Session 请求绑定改变了存储保证",
            ));
        }
        let old = {
            let mut state = self.lock()?;
            let old = std::mem::replace(&mut state.driver.actual, actual);
            state.driver.bound = true;
            old
        };
        drop(old);
        Ok(self)
    }

    /// 完整属性；对象快照共享原有身份。
    pub fn all(&self) -> Result<DataValue> {
        Ok(self.lock()?.attributes.all_value())
    }
    /// 显式纯数据投影；包含对象时返回错误。
    pub fn all_json(&self) -> Result<Value> {
        self.lock()?.attributes.all()
    }
    /// `Arr::get` 读取；存在 null 返回 `Some(Null)`，对象共享身份。
    pub fn get(&self, key: &str) -> Result<Option<DataValue>> {
        self.lock()?.attributes.get_value(key)
    }
    /// 显式纯数据投影；包含对象时返回错误。
    pub fn get_json(&self, key: &str) -> Result<Option<Value>> {
        self.lock()?.attributes.get(key)
    }
    /// 设置原始对象或纯数据；未编码对象只在保存阶段按策略判断。
    pub fn put(&self, key: &str, value: DataValue) -> Result<&Self> {
        let keepalive = value.clone();
        let outcome = self.mutate_errors(key == "errors", |attributes| {
            attributes.put_value(key, value)
        });
        drop(keepalive);
        outcome?;
        Ok(self)
    }
    /// 锁外恢复已登记的命名错误包并返回独立读取快照；旧非错误包类型视作空包。
    ///
    /// 请求内共享修改使用 `RequestInfo::put_view_error_bag`；此返回值的修改不会写回 Session。
    pub fn read_errors(&self) -> Result<crate::support::ViewErrorBag> {
        let raw = self.get_json("errors")?;
        self.inner.error_bag_codecs.decode_view(raw)
    }

    #[cfg(feature = "view")]
    pub(crate) fn shared_view_errors(&self) -> Result<Arc<crate::view::RequestViewErrors>> {
        let (raw, epoch) = {
            let mut state = self.lock()?;
            if let Some(shared) = &state.view_errors {
                return Ok(Arc::clone(shared));
            }
            if state.errors_restoring {
                return Err(extension_error("view_errors_busy", "本请求错误包正在恢复"));
            }
            let raw = state.attributes.get("errors")?;
            state.errors_restoring = true;
            (raw, Arc::clone(&state.errors_epoch))
        };
        let _restoring = RestoringErrors(&self.inner);
        let existing = raw
            .as_ref()
            .map(|raw| self.inner.error_bag_codecs.validate_envelope(raw))
            .transpose()?
            .unwrap_or(false);
        let restored = self.inner.error_bag_codecs.decode_view(raw)?;
        if !existing {
            return Ok(Arc::new(crate::view::RequestViewErrors::from_bag(restored)));
        }
        let weak = Arc::downgrade(&self.inner);
        let shared = Arc::new(
            crate::view::RequestViewErrors::from_bag(restored).with_persistence(
                move |source, name, bag, flash| {
                    let Some(inner) = weak.upgrade() else {
                        return Ok(Box::new(|| Ok(())));
                    };
                    Session { inner }.prepare_view_error_update(source.identity(), name, bag, flash)
                },
            ),
        );
        let mut state = self.lock()?;
        if !Arc::ptr_eq(&epoch, &state.errors_epoch) {
            return Err(extension_error(
                "session_errors_changed",
                "错误包恢复期间 Session errors 已被替换",
            ));
        }
        if let Some(current) = &state.view_errors {
            return Ok(Arc::clone(current));
        }
        state.view_errors = Some(Arc::clone(&shared));
        Ok(shared)
    }

    #[cfg(feature = "view")]
    fn prepare_view_error_update(
        &self,
        identity: Arc<()>,
        name: &crate::support::ArrayKey,
        bag: &dyn crate::support::MessageBagContract,
        flash: bool,
    ) -> Result<crate::view::PreparedPersistence> {
        let attached = self
            .lock()?
            .view_errors
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.identity(), &identity));
        if !attached {
            if flash {
                return Err(extension_error(
                    "session_errors_changed",
                    "闪存写入期间 Session errors 已被替换",
                ));
            }
            return Ok(Box::new(|| Ok(())));
        }
        // 用户 codec 可读取 Session 或尝试重新进入错误包；编码不持有任一状态锁。
        let encoded = self.inner.error_bag_codecs.encode_bag(bag)?;
        let name = name.clone();
        let session = self.clone();
        Ok(Box::new(move || {
            let mut candidate = None;
            let mut retired = None;
            let operation = session.write_operation()?;
            let outcome = (|| -> Result<()> {
                let (snapshot, id, expiry, semantics) = {
                    let state = session.lock()?;
                    if !state
                        .view_errors
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(&current.identity(), &identity))
                    {
                        if flash {
                            return Err(extension_error(
                                "session_errors_changed",
                                "闪存写入期间 Session errors 已被替换",
                            ));
                        }
                        return Ok(());
                    }
                    (
                        state.attributes.clone(),
                        state.id.clone(),
                        state.expiry,
                        state.driver.semantics,
                    )
                };
                candidate = Some(snapshot);
                let current = candidate.as_mut().ok_or_else(invalid_record)?;
                let next = session.inner.error_bag_codecs.put_encoded(
                    current.get("errors")?,
                    &name,
                    &encoded,
                )?;
                if flash {
                    current.flash("errors", next)?;
                } else {
                    current.put("errors", next)?;
                }
                SessionRecord {
                    id,
                    data: current.budget_json()?,
                    expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
                }
                .validate_for(semantics, session.inner.max_bytes)?;
                let mut state = session.lock()?;
                state.ensure_live()?;
                if !state
                    .view_errors
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.identity(), &identity))
                {
                    return Err(extension_error(
                        "session_errors_changed",
                        "闪存写入期间 Session errors 已被替换",
                    ));
                }
                let published = candidate.take().ok_or_else(invalid_record)?;
                retired = Some(std::mem::replace(&mut state.attributes, published));
                Ok(())
            })();
            drop(operation);
            drop(candidate);
            drop(retired);
            outcome
        }))
    }

    /// 把一个命名错误包原子写入 `errors` 并标记为下一请求可读。
    ///
    /// 自定义 codec 的编码在 Session 状态锁外执行；其他包的原始 payload 原样保留。
    pub fn flash_errors(
        &self,
        name: impl Into<crate::support::ArrayKey>,
        bag: &dyn crate::support::MessageBagContract,
    ) -> Result<&Self> {
        let encoded = self.inner.error_bag_codecs.encode_bag(bag)?;
        let name = name.into();
        #[cfg(feature = "view")]
        {
            let shared = self.lock()?.view_errors.clone();
            if let Some(shared) = shared {
                let restored = self.inner.error_bag_codecs.decode_bag(&encoded)?;
                shared.put_with_flash(name, restored, true)?;
                return Ok(self);
            }
        }
        self.mutate(|attributes| {
            let existing = attributes.get("errors")?;
            let next = self
                .inner
                .error_bag_codecs
                .put_encoded(existing, &name, &encoded)?;
            attributes.flash("errors", next)
        })
    }
    /// 读取上一请求的输入；无键时返回整个 bag，缺失 bag 为 `[]`。
    ///
    /// 指定键时先读取 `_old_input`，再在该 bag 内按完整字面键优先、其次 dot 路径读取。
    /// 缺失与显式 null 分别返回 `None` 和 `Some(Null)`。
    pub fn get_old_input(&self, key: Option<&str>) -> Result<Option<DataValue>> {
        self.lock()?.attributes.old_input_value(key)
    }
    /// 显式读取纯数据旧输入；包含对象时返回错误。
    pub fn get_old_input_json(&self, key: Option<&str>) -> Result<Option<Value>> {
        self.lock()?.attributes.old_input(key)
    }
    /// 只有指定键缺失时才采用默认值；已有 null 保持 null。
    pub fn get_old_input_or(&self, key: &str, default: DataValue) -> Result<DataValue> {
        Ok(self.get_old_input(Some(key))?.unwrap_or(default))
    }
    /// 无键时判断旧输入 bag 是否非空；指定键时判断值是否非 null。
    pub fn has_old_input(&self, key: Option<&str>) -> Result<bool> {
        let value = self.get_old_input(key)?;
        Ok(match key {
            None => value.is_some_and(|value| match value {
                DataValue::Array(values) => !values.is_empty(),
                _ => false,
            }),
            Some(_) => value.is_some_and(|value| !value.is_null()),
        })
    }
    /// 读取 `_previous.url` 原值；沿 [`Self::get`] 区分缺失与显式 null。
    ///
    /// Rust 返回原始 `Value`，不将公开 `put` 写入的非字符串值强制转换为 URL。
    pub fn previous_url(&self) -> Result<Option<Value>> {
        self.get_json("_previous.url")
    }
    /// 写入上一请求 URL；沿 [`Self::put`] 保留空串并执行相同状态与预算检查。
    pub fn set_previous_url(&self, url: &str) -> Result<&Self> {
        self.put_typed("_previous.url", url)
    }
    /// 读取 `_previous.route` 原值；缺失为 `None`，匿名路由显式 null 为 `Some(Null)`。
    ///
    /// 与 [`Self::previous_url`] 相同，保留原始 `Value`，不额外强制字符串类型。
    pub fn previous_route(&self) -> Result<Option<Value>> {
        self.get_json("_previous.route")
    }
    /// 写入上一请求路由名；Rust 的 `None` 对应持久属性中的显式 null，不删除键。
    pub fn set_previous_route(&self, route: Option<&str>) -> Result<&Self> {
        self.put_typed("_previous.route", route)
    }
    /// 判断上一请求 URL 是否非 null；false、零和空串仍为有值，不解析或构造 URI。
    pub fn has_previous_uri(&self) -> Result<bool> {
        Ok(self.previous_url()?.is_some_and(|value| !value.is_null()))
    }
    /// 只有缺失才返回默认值；已有null保持null。
    pub fn get_or(&self, key: &str, default: DataValue) -> Result<DataValue> {
        Ok(self.get(key)?.unwrap_or(default))
    }
    /// 只有缺失才执行惰性默认回调；回调执行时不持属性锁。
    pub fn get_or_else(&self, key: &str, default: impl FnOnce() -> DataValue) -> Result<DataValue> {
        Ok(self.get(key)?.unwrap_or_else(default))
    }
    /// 非 null 原值直接返回；缺失/null 才计算并存储回调结果。
    ///
    /// 回调及序列化不持状态锁，允许重入；其自身副作用不回滚。最终写入重新检查
    /// 当前状态与完整记录预算，不承诺跨请求的分布式原子计算。
    pub fn remember(
        &self,
        key: &str,
        callback: impl FnOnce() -> Result<DataValue>,
    ) -> Result<DataValue> {
        if let Some(value) = self.get(key)?.filter(|value| !value.is_null()) {
            return Ok(value);
        }
        self.lock()?.ensure_mutable()?;
        let value = callback()?;
        self.put(key, value.clone())?;
        Ok(value)
    }

    /// 纯数据工厂便利入口；对象应使用 [`Self::remember`]。
    pub fn remember_typed<T: Serialize>(
        &self,
        key: &str,
        callback: impl FnOnce() -> Result<T>,
    ) -> Result<DataValue> {
        self.remember(key, || {
            DataValue::from_json(super::store::value_from(
                &callback()?,
                self.inner.max_bytes,
            )?)
        })
    }

    /// 显式Rust类型读取；不替换Laravel get的Value/null语义。
    pub fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.get_json(key)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| invalid_record())
    }
    /// 所有指定键存在（包括null）；空集合为true。
    pub fn exists(&self, keys: &[&str]) -> Result<bool> {
        self.lock()?.attributes.exists(keys)
    }
    /// 所有指定键非null；false/0/空串均计为有值。
    pub fn has(&self, keys: &[&str]) -> Result<bool> {
        self.lock()?.attributes.has(keys)
    }
    /// 至少一个指定键非null。
    pub fn has_any(&self, keys: &[&str]) -> Result<bool> {
        self.lock()?.attributes.has_any(keys)
    }
    /// 至少一个指定键不存在。
    pub fn missing(&self, keys: &[&str]) -> Result<bool> {
        Ok(!self.exists(keys)?)
    }
    /// 顶层键选择，保持原属性顺序。
    pub fn only(&self, keys: &[&str]) -> Result<DataValue> {
        self.lock()?.attributes.only_value(keys)
    }
    /// 显式纯数据顶层选择。
    pub fn only_json(&self, keys: &[&str]) -> Result<Value> {
        self.lock()?.attributes.only(keys)
    }
    /// 按 `Arr::forget` 排除指定键，完整 literal 键优先。
    pub fn except(&self, keys: &[&str]) -> Result<DataValue> {
        self.lock()?.attributes.except_value(keys)
    }
    /// 显式纯数据路径排除。
    pub fn except_json(&self, keys: &[&str]) -> Result<Value> {
        self.lock()?.attributes.except(keys)
    }
    /// 纯数据便利写入；对象使用 [`Self::put`]。
    pub fn put_typed<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate_errors(key == "errors", |attributes| attributes.put(key, value))?;
        Ok(self)
    }
    /// 按调用顺序批量dot写入，整个操作原子完成。
    pub fn put_many(&self, values: &[(String, DataValue)]) -> Result<&Self> {
        self.mutate_errors(
            values.iter().any(|(key, _)| key == "errors"),
            |attributes| attributes.put_many_value(values),
        )?;
        Ok(self)
    }
    /// 纯数据批量写入便利入口。
    pub fn put_many_typed(&self, values: &[(String, Value)]) -> Result<&Self> {
        self.mutate_errors(
            values.iter().any(|(key, _)| key == "errors"),
            |attributes| attributes.put_many(values),
        )?;
        Ok(self)
    }
    /// 固定Store的replace是put合并，不清除其他属性。
    pub fn replace(&self, values: &[(String, DataValue)]) -> Result<&Self> {
        self.put_many(values)
    }
    /// 纯数据 replace 便利入口。
    pub fn replace_typed(&self, values: &[(String, Value)]) -> Result<&Self> {
        self.put_many_typed(values)
    }
    /// 删除键，不删除因此变空的父数组。
    pub fn forget(&self, keys: &[&str]) -> Result<&Self> {
        self.mutate(|attributes| attributes.forget(keys))
    }
    /// 读取并删除；null与缺失仍可区分。
    pub fn pull(&self, key: &str) -> Result<Option<DataValue>> {
        self.mutate_value(|attributes| attributes.pull_value(key))
    }
    /// 显式纯数据读取并删除；对象存在时保持原属性不变并返回错误。
    pub fn pull_json(&self, key: &str) -> Result<Option<Value>> {
        self.mutate_value(|attributes| attributes.pull(key))
    }
    /// Laravel remove 同样读取并删除；None 表示缺失，Some(Null) 保留已存 null。
    pub fn remove(&self, key: &str) -> Result<Option<DataValue>> {
        self.pull(key)
    }

    /// 追加 PHP 数组值；缺失、null 或 false 从空数组开始。
    pub fn push(&self, key: &str, value: DataValue) -> Result<&Self> {
        let keepalive = value.clone();
        let outcome = self.mutate(|attributes| attributes.push_value(key, value));
        drop(keepalive);
        outcome
    }
    /// 纯数据数组追加便利入口。
    pub fn push_typed<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.push(key, value))
    }

    /// 默认加一，返回并存储结果；缺失从整数零开始。
    pub fn increment(&self, key: &str) -> Result<Value> {
        self.increment_by(key, 1)
    }
    /// 按 PHP 加法增加给定值；错误或不可表示的非有限结果不改写属性。
    pub fn increment_by<T: Serialize>(&self, key: &str, amount: T) -> Result<Value> {
        let amount = super::store::value_from(&amount, self.inner.max_bytes)?;
        self.mutate_value(|attributes| attributes.increment(key, &amount))
    }
    /// 默认减一，返回并存储结果。
    pub fn decrement(&self, key: &str) -> Result<Value> {
        self.decrement_by(key, 1)
    }
    /// 先按 PHP 计算 amount * -1 再相加；不以饱和整数转换代替运算。
    pub fn decrement_by<T: Serialize>(&self, key: &str, amount: T) -> Result<Value> {
        let amount = super::store::value_from(&amount, self.inner.max_bytes)?;
        let amount = crate::support::scalars::negate(&amount).ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_data_invalid",
                "Session 数值运算失败",
            )
        })?;
        self.mutate_value(|attributes| attributes.increment(key, &amount))
    }

    /// 读取并删除，只有缺失才使用默认。
    pub fn pull_or(&self, key: &str, default: DataValue) -> Result<DataValue> {
        Ok(self.pull(key)?.unwrap_or(default))
    }
    /// 清除全部属性；不自动再生token。
    pub fn flush(&self) -> Result<&Self> {
        self.mutate(Attributes::flush)
    }
    /// 标记下一请求仍可读取的flash值。
    pub fn flash(&self, key: &str, value: DataValue) -> Result<&Self> {
        let keepalive = value.clone();
        let outcome = self.mutate_errors(key == "errors", |attributes| {
            attributes.flash_value(key, value)
        });
        drop(keepalive);
        outcome?;
        Ok(self)
    }
    /// 纯数据 flash 便利入口。
    pub fn flash_typed<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate_errors(key == "errors", |attributes| attributes.flash(key, value))?;
        Ok(self)
    }
    /// 将数组或对象作为下一请求的旧输入；状态和记录预算失败时原子回滚。
    pub fn flash_input(&self, value: DataValue) -> Result<&Self> {
        let keepalive = value.clone();
        let outcome = self.mutate(|attributes| attributes.flash_input_value(value));
        drop(keepalive);
        outcome
    }
    /// 纯数据旧输入便利入口。
    pub fn flash_input_typed<T: Serialize>(&self, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.flash_input(value))
    }
    /// 设置保存对象策略；默认拒绝对象，JSON 策略要求对象显式投影。
    pub fn set_object_encoding(&self, encoding: ObjectEncoding) -> Result<&Self> {
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        state.object_encoding = encoding;
        Ok(self)
    }
    /// 仅当前保存前可见；本次save会删除，reflash可显式延长。
    pub fn now(&self, key: &str, value: DataValue) -> Result<&Self> {
        let keepalive = value.clone();
        let outcome = self.mutate_errors(key == "errors", |attributes| {
            attributes.now_value(key, value)
        });
        drop(keepalive);
        outcome?;
        Ok(self)
    }
    /// 纯数据 now 便利入口。
    pub fn now_typed<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate_errors(key == "errors", |attributes| attributes.now(key, value))?;
        Ok(self)
    }
    /// 将old flash延长一请求。
    pub fn reflash(&self) -> Result<&Self> {
        self.mutate(Attributes::reflash)
    }
    /// 保留指定flash；空集合不等于reflash。
    pub fn keep(&self, keys: &[&str]) -> Result<&Self> {
        self.mutate(|attributes| attributes.keep(keys))
    }
    /// 当前token原值；不对业务写入的非null值强制类型或长度。
    pub fn token(&self) -> Result<Option<Value>> {
        self.get_json("_token")
    }
    /// 用系统随机源重新生成40位token。
    pub fn regenerate_token(&self) -> Result<&Self> {
        self.put_typed("_token", random_token()?)
    }
    /// 当前显式期限策略。
    pub fn expiry(&self) -> Result<Option<Expiry>> {
        Ok(self.lock()?.expiry)
    }
    /// 修改后续保存使用的期限。
    pub fn set_expiry(&self, expiry: Option<Expiry>) -> Result<&Self> {
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        if let Some(lifetime) = state.driver.activity_lifetime {
            let compatible = match expiry {
                None | Some(Expiry::OnSessionEnd) => lifetime == 120 * 60,
                Some(Expiry::OnInactivity(duration)) => duration == Duration::seconds(lifetime),
                Some(Expiry::AtDateTime(_)) => false,
            };
            if !compatible {
                return Err(extension_error(
                    "session_expiry_unsupported",
                    "原生数据库期限来自最终配置；绝对截止须显式选择增强 driver",
                ));
            }
        }
        SessionRecord {
            id: state.id.clone(),
            data: state.attributes.budget_json()?,
            expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
        }
        .validate_for(state.driver.semantics, self.inner.max_bytes)?;
        state.expiry = expiry;
        Ok(self)
    }
    /// 按当前策略计算的持久截止，不会续写store。
    pub fn expiry_date(&self) -> Result<OffsetDateTime> {
        self.lock()?.expiry.unwrap_or(Expiry::OnSessionEnd).date()
    }

    pub(crate) async fn collect_garbage(&self, lifetime_seconds: i64) -> Result<u64> {
        let _operation = self.operation().await?;
        let driver = self.lock()?.driver.clone();
        driver.ready()?.gc(lifetime_seconds).await
    }

    /// 立即读取当前绑定，与已有属性合并；待保存的跨绑定转移禁止混读目标。
    pub async fn start(&self) -> Result<bool> {
        let _operation = self.operation().await?;
        let (id, expiry, driver, existing) = {
            let state = self.lock()?;
            if state.driver.semantics == SessionStoreSemantics::Guarded && state.pending_transfer()
            {
                return Err(extension_error(
                    "session_rebind_pending",
                    "Save the explicit Session transfer before starting",
                ));
            }
            (
                state.id.clone(),
                state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
                state.driver.clone(),
                state.existing,
            )
        };
        let store = driver.ready()?;
        let record = store.load(&id, self.inner.max_bytes, expiry).await?;
        let (mut attributes, encoding) = {
            let state = self.lock()?;
            (state.attributes.clone(), state.object_encoding)
        };
        let mut found = false;
        #[cfg(feature = "view")]
        let mut loaded_errors = false;
        let mut budget_expiry = expiry;
        if let Some(record) = record {
            if record.id != id {
                return Err(invalid_record());
            }
            record.validate_for(driver.semantics, self.inner.max_bytes)?;
            if record.expiry_date > OffsetDateTime::now_utc() {
                budget_expiry = record.expiry_date;
                #[cfg(feature = "view")]
                {
                    loaded_errors = crate::support::data::get(&record.data, "errors").is_some();
                }
                let incoming = if encoding == ObjectEncoding::JsonProjection {
                    crate::support::data::cast_json(record.data)
                } else {
                    record.data
                };
                attributes.merge_loaded(incoming)?;
                found = true;
            } else if existing && driver.semantics == SessionStoreSemantics::Guarded {
                self.lock()?.revoked = true;
                return Err(write_conflict());
            }
        } else if existing && driver.semantics == SessionStoreSemantics::Guarded {
            self.lock()?.revoked = true;
            return Err(write_conflict());
        }
        if attributes
            .get("_token")?
            .is_none_or(|value| value.is_null())
        {
            attributes.put("_token", Value::String(random_token()?))?;
        }
        SessionRecord {
            id: id.clone(),
            data: attributes.budget_json()?,
            expiry_date: budget_expiry,
        }
        .validate_for(driver.semantics, self.inner.max_bytes)?;
        let retired = {
            let mut state = self.lock()?;
            #[cfg(feature = "view")]
            let retired_errors = state.retire_view_errors(&attributes, loaded_errors)?;
            #[cfg(not(feature = "view"))]
            let retired_errors = ();
            let old_attributes = std::mem::replace(&mut state.attributes, attributes);
            state.existing = found;
            state.started = true;
            let origin = found.then_some(Binding { id, driver });
            let old_origin = std::mem::replace(&mut state.origin, origin);
            (old_attributes, old_origin, retired_errors)
        };
        drop(retired);
        Ok(true)
    }

    fn prepare_save(&self) -> Result<PreparedSave> {
        let (mut candidate, driver, id, expiry, encoding, mode, origin, intent) = {
            let state = self.lock()?;
            let driver = state.driver.clone();
            driver.ready()?;
            let origin = state
                .origin
                .as_ref()
                .filter(|origin| state.pending_transfer() && !state.destroyed.contains(&origin.id))
                .cloned();
            (
                state.attributes.clone(),
                driver,
                state.id.clone(),
                state.expiry,
                state.object_encoding,
                if state.existing {
                    SessionWriteMode::Update
                } else {
                    SessionWriteMode::Insert
                },
                origin,
                state.intent,
            )
        };
        candidate.age()?;
        // Laravel Store::save 先 ageFlashData，序列化失败后内存 flash 仍已老化。
        let retired = {
            let mut state = self.lock()?;
            #[cfg(feature = "view")]
            let retired_errors = state.retire_view_errors(&candidate, false)?;
            #[cfg(not(feature = "view"))]
            let retired_errors = ();
            let old = std::mem::replace(&mut state.attributes, candidate.clone());
            (old, retired_errors)
        };
        drop(retired);
        let record = SessionRecord {
            id,
            data: candidate.all_value().project(encoding)?,
            expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
        };
        record.validate_for(driver.semantics, self.inner.max_bytes)?;
        Ok(PreparedSave {
            record,
            mode,
            driver,
            origin,
            intent,
        })
    }

    /// 老化 flash 后保存；显式目标碰撞不覆盖、不换成其他随机 ID。
    pub async fn save(&self) -> Result<()> {
        // Guard 回调可能读取 Session 或通过 recaller 轮换 ID，不能持有 I/O 门再解析。
        #[cfg(feature = "auth")]
        self.project_guard_identity().await?;
        let _operation = self.operation().await?;
        let PreparedSave {
            mut record,
            mode,
            driver,
            origin,
            intent,
        } = self.prepare_save()?;
        #[cfg(feature = "auth")]
        if crate::support::data::get(&record.data, crate::auth::AUTH_USER_ID_KEY)
            .is_some_and(|value| !value.is_null())
            || crate::support::data::get(&record.data, "_openrust_guard_bindings")
                .is_some_and(|value| value.as_array().is_none_or(|keys| !keys.is_empty()))
        {
            let name = self.lock()?.name.clone();
            driver.require_authenticated(&name)?;
        }
        let store = driver.ready()?;
        if let Some(origin) = origin {
            let native = driver.semantics == SessionStoreSemantics::NativeDatabase
                || origin.driver.semantics == SessionStoreSemantics::NativeDatabase;
            if native && driver.semantics != origin.driver.semantics {
                return Err(extension_error(
                    "session_transfer_unsupported",
                    "跨 driver 期限及撤销转移须显式选择增强存储",
                ));
            }
            if !native {
                let source = origin.driver.ready()?;
                let record = source
                    .load(&origin.id, self.inner.max_bytes, record.expiry_date)
                    .await?;
                let live = match record {
                    Some(record) => {
                        if record.id != origin.id {
                            return Err(invalid_record());
                        }
                        record.validate_for(origin.driver.semantics, self.inner.max_bytes)?;
                        record.expiry_date > OffsetDateTime::now_utc()
                    }
                    None => false,
                };
                if !live {
                    self.lock()?.revoked = true;
                    return Err(write_conflict());
                }
            }
        }
        // source 检查与目标写入不构成跨后端事务；显式复制不承诺线性撤销。
        for attempt in 0..3 {
            record.validate_for(driver.semantics, self.inner.max_bytes)?;
            match store.write(&record, mode, self.inner.max_bytes).await? {
                SessionWriteOutcome::Written => {
                    let old = {
                        let mut state = self.lock()?;
                        state.existing = true;
                        state.started = false;
                        state.intent = CreateIntent::SystemRandom;
                        state.origin.replace(Binding {
                            id: record.id.clone(),
                            driver: driver.clone(),
                        })
                    };
                    drop(old);
                    return Ok(());
                }
                SessionWriteOutcome::Conflict
                    if mode == SessionWriteMode::Insert
                        && intent == CreateIntent::SystemRandom
                        && attempt < 2 =>
                {
                    let id = SessionId::generate()?;
                    let mut state = self.lock()?;
                    if state.destroyed.contains(&id) {
                        return Err(write_conflict());
                    }
                    state.id = id.clone();
                    record.id = id;
                }
                SessionWriteOutcome::Missing | SessionWriteOutcome::Conflict => {
                    if mode == SessionWriteMode::Update || intent == CreateIntent::SystemRandom {
                        self.lock()?.revoked = true;
                    }
                    return Err(write_conflict());
                }
            }
        }
        Err(write_conflict())
    }

    async fn change_id(&self, destroy: bool, regenerate: bool, flush: bool) -> Result<bool> {
        let _operation = self.operation().await?;
        let id = SessionId::generate()?;
        let (old_id, mut candidate, driver, expiry) = {
            let state = self.lock()?;
            if destroy
                && state.driver.semantics == SessionStoreSemantics::Guarded
                && state.pending_transfer()
            {
                return Err(extension_error(
                    "session_rebind_pending",
                    "Save the explicit Session transfer before destroying",
                ));
            }
            if destroy
                && state.driver.semantics == SessionStoreSemantics::Guarded
                && !state.destroyed.contains(&state.id)
                && state.destroyed.len() >= 1024
            {
                return Err(limit_error());
            }
            if state.driver.semantics == SessionStoreSemantics::Guarded
                && state.destroyed.contains(&id)
            {
                return Err(write_conflict());
            }
            (
                state.id.clone(),
                state.attributes.clone(),
                state.driver.clone(),
                state.expiry,
            )
        };
        if flush {
            candidate.flush()?;
        }
        if regenerate {
            candidate.put("_token", Value::String(random_token()?))?;
        }
        SessionRecord {
            id: id.clone(),
            data: candidate.budget_json()?,
            expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
        }
        .validate_for(driver.semantics, self.inner.max_bytes)?;
        // 候选全部验证后才删除持久记录；IO失败本身不承诺回滚外部副作用。
        let store = driver.ready()?;
        if destroy {
            store.delete(&old_id).await?;
        }
        // 同一 I/O 门控内同步重置，取消不能留下“新 ID + 旧 exists”的组合。
        store.reset_exists()?;
        let retired = {
            let mut state = self.lock()?;
            #[cfg(feature = "view")]
            let retired_errors = state.retire_view_errors(&candidate, flush)?;
            #[cfg(not(feature = "view"))]
            let retired_errors = ();
            if destroy && driver.semantics == SessionStoreSemantics::Guarded {
                state.destroyed.insert(old_id);
            }
            state.id = id;
            let old_attributes = std::mem::replace(&mut state.attributes, candidate);
            state.existing = false;
            state.intent = CreateIntent::SystemRandom;
            // invalidate 已清空全部来源属性；tombstone 仍禁止回到销毁的 ID。
            let old_origin = if flush { state.origin.take() } else { None };
            (old_attributes, old_origin, retired_errors)
        };
        drop(retired);
        Ok(true)
    }
    /// 只迁移ID；destroy控制是否立即撤销旧记录。
    pub async fn migrate(&self, destroy: bool) -> Result<bool> {
        self.change_id(destroy, false, false).await
    }
    /// 迁移 ID 并再生 token。
    pub async fn regenerate(&self, destroy: bool) -> Result<bool> {
        self.change_id(destroy, true, false).await
    }
    /// 清空属性、撤销旧ID并换ID；不立即再生token。
    pub async fn invalidate(&self) -> Result<bool> {
        self.change_id(true, false, true).await
    }

    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.closed = true;
        }
    }
}

impl State {
    fn has_transfer_data(&self) -> Result<bool> {
        if self.started || self.existing || self.origin.is_some() {
            return Ok(true);
        }
        match self.attributes.budget_json()? {
            Value::Array(values) => Ok(!values.is_empty()),
            Value::Object(values) => Ok(!values.is_empty()),
            _ => Err(invalid_record()),
        }
    }

    #[cfg(feature = "view")]
    fn retire_view_errors(
        &mut self,
        candidate: &Attributes,
        force: bool,
    ) -> Result<Option<Arc<crate::view::RequestViewErrors>>> {
        if force || self.attributes.get("errors")? != candidate.get("errors")? {
            self.errors_epoch = Arc::new(());
            Ok(self.view_errors.take())
        } else {
            Ok(None)
        }
    }

    fn restore_proof(&mut self) {
        self.existing = self
            .origin
            .as_ref()
            .is_some_and(|origin| origin.matches(&self.id, &self.driver));
    }
    fn pending_transfer(&self) -> bool {
        self.origin
            .as_ref()
            .is_some_and(|origin| !origin.matches(&self.id, &self.driver))
    }
    fn ensure_mutable(&self) -> Result<()> {
        self.ensure_live()?;
        if self.busy {
            return Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_operation_in_progress",
                "Session I/O is in progress",
            ));
        }
        Ok(())
    }
    fn ensure_live(&self) -> Result<()> {
        if self.revoked {
            return Err(write_conflict());
        }
        if self.closed {
            return Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_closed",
                "Session request scope is closed",
            ));
        }
        Ok(())
    }
}
struct Operation<'a> {
    inner: &'a Inner,
    gate: Option<tokio::sync::MutexGuard<'a, ()>>,
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            // 先释放 I/O 门再清 owner，不能让等待者看到无 owner 的同步退出窗口。
            drop(self.gate.take());
            state.busy = false;
            state.sync_owner = None;
            drop(state);
            self.inner.sync_idle.notify_all();
        }
    }
}
fn write_conflict() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_store_write_conflict",
        "Session could not be saved after concurrent revocation",
    )
}

fn extension_error(code: &'static str, message: &'static str) -> Error {
    Error::custom_with_status(http::StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use std::any::Any;
    use std::sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct Counted(Arc<AtomicUsize>, String);
    struct UploadedResource;
    struct ReentrantDrop(Weak<Inner>, Arc<AtomicUsize>);
    impl crate::support::DataObject for ReentrantDrop {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    impl Drop for ReentrantDrop {
        fn drop(&mut self) {
            if let Some(inner) = self.0.upgrade() {
                let session = Session { inner };
                if session.get("_token").is_ok() && session.put_typed("from_drop", true).is_ok() {
                    self.1.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    }
    struct ThreadedDrop(Weak<Inner>, Arc<Mutex<Vec<String>>>, &'static str);
    impl crate::support::DataObject for ThreadedDrop {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    impl Drop for ThreadedDrop {
        fn drop(&mut self) {
            if let Some(inner) = self.0.upgrade() {
                let key = self.2;
                let (sent, received) = mpsc::channel();
                let worker = std::thread::spawn(move || {
                    let result = Session { inner }
                        .put_typed(key, true)
                        .map_or_else(|error| error.code().to_owned(), |_| "ok".to_owned());
                    let _ = sent.send(result);
                });
                let result = received
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or_else(|_| "timeout".to_owned());
                if result != "timeout" {
                    let _ = worker.join();
                }
                if let Ok(mut results) = self.1.lock() {
                    results.push(result);
                }
            }
        }
    }
    struct ReentrantProjection(Weak<Inner>, Arc<AtomicUsize>);
    impl crate::support::DataObject for ReentrantProjection {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn json_projection(&self) -> Option<Value> {
            if let Some(inner) = self.0.upgrade() {
                let session = Session { inner };
                if session.get("_token").is_ok()
                    && session.put_typed("from_projection", true).is_err()
                {
                    self.1.fetch_add(1, Ordering::SeqCst);
                }
            }
            Some(serde_json::json!({"stored":true}))
        }
    }
    impl crate::support::DataObject for UploadedResource {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn json_projection(&self) -> Option<Value> {
            Some(serde_json::json!({}))
        }
    }

    #[tokio::test]
    async fn object_identity_and_save_codec_follow_store_boundary() -> Result<()> {
        let store = Arc::new(super::super::MemorySessionStore::default());
        let native = Session::new(None, store.clone(), None)?;
        native.start().await?;
        let object = DataValue::object(UploadedResource);
        let mut input = DataValue::Array(Vec::new());
        input.set("profile.avatar", object.clone())?;
        native.flash_input(input)?;
        let before = native
            .get("_old_input.profile.avatar")?
            .ok_or_else(invalid_record)?;
        assert!(before.same_object(&object));
        let error = native.save().await.err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "data_object_not_serializable");
        assert!(
            native
                .get("_old_input.profile.avatar")?
                .is_some_and(|value| value.same_object(&object))
        );
        assert_eq!(
            native.get_json("_flash.old")?,
            Some(serde_json::json!(["_old_input"]))
        );
        assert!(
            store
                .load(&native.id()?, DEFAULT_MAX_BYTES, OffsetDateTime::now_utc())
                .await?
                .is_none()
        );
        native.save().await?;
        assert!(native.get("_old_input")?.is_none());

        let now = Session::new(None, store.clone(), None)?;
        now.start().await?;
        now.now("transient", object.clone())?;
        now.save().await?;
        assert!(now.get("transient")?.is_none());

        let json = Session::new(None, store.clone(), None)?;
        json.set_object_encoding(ObjectEncoding::JsonProjection)?;
        json.start().await?;
        json.flash_input(DataValue::Array(vec![(
            crate::support::ArrayKey::from("avatar"),
            object.clone(),
        )]))?;
        assert!(
            json.get_old_input(Some("avatar"))?
                .is_some_and(|value| value.same_object(&object))
        );
        json.save().await?;
        let reloaded = Session::new(Some(json.id()?), store, None)?;
        reloaded.set_object_encoding(ObjectEncoding::JsonProjection)?;
        reloaded.start().await?;
        assert_eq!(
            reloaded.get_old_input_json(Some("avatar"))?,
            Some(serde_json::json!([]))
        );
        Ok(())
    }

    #[tokio::test]
    async fn object_drop_and_projection_can_reenter_session_without_lock() -> Result<()> {
        let store = Arc::new(super::super::MemorySessionStore::default());
        let session = Session::new(None, store.clone(), None)?;
        session.start().await?;
        let drops = Arc::new(AtomicUsize::new(0));
        session.put(
            "resource",
            DataValue::object(ReentrantDrop(Arc::downgrade(&session.inner), drops.clone())),
        )?;
        session.forget(&["resource"])?;
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(session.get_json("from_drop")?, Some(Value::Bool(true)));

        let limited = Session::with_options(None, store.clone(), None, 1024, "test".into())?;
        limited.start().await?;
        let rejected_drops = Arc::new(AtomicUsize::new(0));
        let rejected = limited.mutate_errors(false, |attributes| {
            attributes.put_value(
                "resource",
                DataValue::object(ReentrantDrop(
                    Arc::downgrade(&limited.inner),
                    rejected_drops.clone(),
                )),
            )?;
            attributes.put_value("large", DataValue::String("z".repeat(900)))
        });
        assert!(rejected.is_err());
        assert_eq!(rejected_drops.load(Ordering::SeqCst), 1);
        assert!(limited.get("resource")?.is_none());
        assert_eq!(limited.get_json("from_drop")?, Some(Value::Bool(true)));

        session.set_object_encoding(ObjectEncoding::JsonProjection)?;
        let projections = Arc::new(AtomicUsize::new(0));
        session.put(
            "resource",
            DataValue::object(ReentrantProjection(
                Arc::downgrade(&session.inner),
                projections.clone(),
            )),
        )?;
        session.save().await?;
        assert_eq!(projections.load(Ordering::SeqCst), 1);
        assert!(session.get("from_projection")?.is_none());
        let stored = store
            .load(&session.id()?, DEFAULT_MAX_BYTES, OffsetDateTime::now_utc())
            .await?
            .ok_or_else(invalid_record)?;
        assert_eq!(
            crate::support::data::get(&stored.data, "resource.stored"),
            Some(Value::Bool(true))
        );
        Ok(())
    }

    #[test]
    fn concurrent_sync_writes_wait_for_gate_release() -> Result<()> {
        let session = Session::new(
            None,
            Arc::new(super::super::MemorySessionStore::default()),
            None,
        )?;
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        std::thread::scope(|scope| -> Result<()> {
            let first = session.clone();
            let first = scope.spawn(move || -> Result<()> {
                let operation = first.write_operation()?;
                entered_tx.send(()).map_err(Error::internal)?;
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(Error::internal)?;
                drop(operation);
                Ok(())
            });
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(Error::internal)?;
            let (started_tx, started_rx) = mpsc::channel();
            let second = session.clone();
            let second = scope.spawn(move || {
                let _ = started_tx.send(());
                second.put_typed("after_drop", true).map(|_| ())
            });
            started_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(Error::internal)?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while session.inner.sync_waiters.load(Ordering::SeqCst) == 0 {
                if std::time::Instant::now() >= deadline {
                    return Err(invalid_record());
                }
                std::thread::yield_now();
            }
            assert!(session.get("after_drop")?.is_none());
            release_tx.send(()).map_err(Error::internal)?;
            first.join().map_err(|_| invalid_record())??;
            second.join().map_err(|_| invalid_record())??;
            Ok(())
        })?;
        assert_eq!(session.get_json("after_drop")?, Some(Value::Bool(true)));
        Ok(())
    }

    #[test]
    fn cross_thread_object_destructors_write_after_commit_and_failed_candidate() -> Result<()> {
        let store = Arc::new(super::super::MemorySessionStore::default());
        let session = Session::new(None, store.clone(), None)?;
        let committed = Arc::new(Mutex::new(Vec::new()));
        session.put(
            "resource",
            DataValue::object(ThreadedDrop(
                Arc::downgrade(&session.inner),
                committed.clone(),
                "after_commit",
            )),
        )?;
        session.forget(&["resource"])?;
        assert_eq!(session.get_json("after_commit")?, Some(Value::Bool(true)));
        assert_eq!(
            committed.lock().map_err(|_| invalid_record())?.clone(),
            vec!["ok".to_owned()]
        );

        let limited = Session::with_options(None, store, None, 1024, "test".into())?;
        let rejected = Arc::new(Mutex::new(Vec::new()));
        let outcome = limited.mutate_errors(false, |attributes| {
            attributes.put_value(
                "resource",
                DataValue::object(ThreadedDrop(
                    Arc::downgrade(&limited.inner),
                    rejected.clone(),
                    "after_rejection",
                )),
            )?;
            attributes.put_value("large", DataValue::String("z".repeat(900)))
        });
        assert_eq!(
            outcome.err().map(|error| error.code().to_owned()),
            Some("session_limits_exceeded".to_owned())
        );
        assert!(limited.get("resource")?.is_none());
        assert!(limited.get("large")?.is_none());
        assert_eq!(
            limited.get_json("after_rejection")?,
            Some(Value::Bool(true))
        );
        assert_eq!(
            rejected.lock().map_err(|_| invalid_record())?.clone(),
            vec!["ok".to_owned()]
        );

        let input_drop = Arc::new(Mutex::new(Vec::new()));
        let overdeep = format!("{}z", "a.".repeat(65));
        assert!(
            limited
                .put(
                    &overdeep,
                    DataValue::object(ThreadedDrop(
                        Arc::downgrade(&limited.inner),
                        input_drop.clone(),
                        "after_input_rejection",
                    )),
                )
                .is_err()
        );
        assert_eq!(
            limited.get_json("after_input_rejection")?,
            Some(Value::Bool(true))
        );
        assert_eq!(
            input_drop.lock().map_err(|_| invalid_record())?.clone(),
            vec!["ok".to_owned()]
        );
        Ok(())
    }

    #[tokio::test]
    async fn async_operation_rejects_sync_write_without_waiting() -> Result<()> {
        let session = Session::new(
            None,
            Arc::new(super::super::MemorySessionStore::default()),
            None,
        )?;
        let operation = session.operation().await?;
        tokio::task::yield_now().await;
        assert_eq!(
            session
                .put_typed("busy", true)
                .err()
                .map(|error| error.code().to_owned()),
            Some("session_operation_in_progress".to_owned())
        );
        drop(operation);
        session.put_typed("ready", true)?;
        assert_eq!(session.get_json("ready")?, Some(Value::Bool(true)));
        Ok(())
    }

    #[test]
    fn object_snapshots_survive_overwrite_pull_flush_and_parallel_reads() -> Result<()> {
        let session = Session::new(
            None,
            Arc::new(super::super::MemorySessionStore::default()),
            None,
        )?;
        let original = DataValue::object(UploadedResource);
        session.put("profile.avatar", original.clone())?;
        let snapshot = session.all()?;
        session.push("list", original.clone())?;
        assert!(
            session
                .get("list.0")?
                .is_some_and(|value| value.same_object(&original))
        );
        assert!(
            session
                .only(&["profile"])?
                .get("profile.avatar")?
                .is_some_and(|value| value.same_object(&original))
        );
        assert!(
            session
                .except(&["list"])?
                .get("profile.avatar")?
                .is_some_and(|value| value.same_object(&original))
        );
        assert!(
            session
                .get("profile.avatar")?
                .is_some_and(|value| value.same_object(&original))
        );
        assert_eq!(
            session
                .get_typed::<Value>("profile.avatar")
                .err()
                .map(|error| error.code().to_owned()),
            Some("data_object_not_serializable".to_owned())
        );
        let other = session.clone();
        let expected = original.clone();
        let observed = std::thread::spawn(move || {
            other
                .get("profile.avatar")
                .ok()
                .flatten()
                .is_some_and(|value| value.same_object(&expected))
        });
        assert!(observed.join().map_err(|_| invalid_record())?);
        let removed = session.pull("profile.avatar")?.ok_or_else(invalid_record)?;
        assert!(removed.same_object(&original));
        session.put("profile.avatar", DataValue::object(UploadedResource))?;
        session.flush()?;
        assert!(session.get("profile.avatar")?.is_none());
        assert!(
            snapshot
                .get("profile.avatar")?
                .is_some_and(|value| value.same_object(&original))
        );
        Ok(())
    }
    impl Serialize for Counted {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            serializer.serialize_str(&self.1)
        }
    }
    #[tokio::test]
    async fn whole_budget_rejects_atomic_mutations_without_reserializing_business_values()
    -> Result<()> {
        let session = Session::with_options(
            None,
            Arc::new(super::super::MemorySessionStore::default()),
            None,
            1024,
            "test".into(),
        )?;
        session.start().await?;
        let count = Arc::new(AtomicUsize::new(0));
        session.put_typed("a", Counted(count.clone(), "a".repeat(450)))?;
        let before = session.all_json()?;
        let error = session
            .put_typed("b", Counted(count.clone(), "b".repeat(450)))
            .err()
            .ok_or_else(invalid_record)?;
        assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(session.all_json()?, before);
        assert!(session.flash_typed("b", "b".repeat(450)).is_err());
        assert_eq!(session.all_json()?, before);
        Ok(())
    }
    #[tokio::test]
    async fn start_merge_and_missing_token_count_toward_whole_budget_atomically() -> Result<()> {
        let store = Arc::new(super::super::MemorySessionStore::default());
        let id = SessionId::generate()?;
        let record = SessionRecord {
            id: id.clone(),
            data: serde_json::json!({"persisted":"p".repeat(450),"_token":""}),
            expiry_date: OffsetDateTime::now_utc() + Duration::hours(1),
        };
        store.write(&record, SessionWriteMode::Insert, 1024).await?;
        let session = Session::with_options(Some(id), store.clone(), None, 1024, "test".into())?;
        session.put_typed("local", "l".repeat(450))?;
        let before = session.all_json()?;
        let error = session.start().await.err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.all_json()?, before);
        let exact = record.encode(DEFAULT_MAX_BYTES)?.len();
        let loaded =
            Session::with_options(Some(record.id.clone()), store, None, exact, "test".into())?;
        loaded.start().await?;
        assert_eq!(loaded.all_json()?, record.data);
        // 值不变、仅新增flash元数据也必须计入同一预算。
        assert!(loaded.flash_typed("persisted", "p".repeat(450)).is_err());
        assert_eq!(loaded.all_json()?, record.data);
        let no_token = SessionRecord {
            id: SessionId::generate()?,
            data: serde_json::json!({"persisted":"p".repeat(450)}),
            expiry_date: record.expiry_date,
        };
        let exact = no_token.encode(DEFAULT_MAX_BYTES)?.len();
        let token_store = Arc::new(super::super::MemorySessionStore::default());
        token_store
            .write(&no_token, SessionWriteMode::Insert, 1024)
            .await?;
        let needs_token =
            Session::with_options(Some(no_token.id), token_store, None, exact, "test".into())?;
        let error = needs_token.start().await.err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(needs_token.all_json()?, serde_json::json!([]));
        Ok(())
    }
    fn fixed_expiry() -> Result<OffsetDateTime> {
        (OffsetDateTime::now_utc() + Duration::hours(1))
            .replace_nanosecond(0)
            .map_err(Error::internal)
    }
    #[tokio::test]
    async fn pull_sparse_shape_and_expiry_growth_are_atomic_at_exact_budget() -> Result<()> {
        let expiry = fixed_expiry()?;
        let record = SessionRecord {
            id: SessionId::generate()?,
            data: serde_json::json!([0, "x".repeat(300)]),
            expiry_date: expiry,
        };
        let max = record.encode(DEFAULT_MAX_BYTES)?.len();
        let session = Session::with_options(
            Some(record.id),
            Arc::new(super::super::MemorySessionStore::default()),
            Some(Expiry::AtDateTime(expiry)),
            max,
            "test".into(),
        )?;
        session.put_many_typed(&[
            ("0".into(), serde_json::json!(0)),
            ("1".into(), serde_json::json!("x".repeat(300))),
        ])?;
        let before = session.all_json()?;
        let error = session.pull("0").err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.all_json()?, before);
        let far = OffsetDateTime::from_unix_timestamp(253_402_300_000).map_err(Error::internal)?;
        let error = session
            .set_expiry(Some(Expiry::AtDateTime(far)))
            .err()
            .ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.expiry_date()?, expiry);
        assert_eq!(session.all_json()?, before);
        Ok(())
    }
    #[tokio::test]
    async fn regeneration_and_flash_aging_validate_before_delete_or_publication() -> Result<()> {
        let store = Arc::new(super::super::MemorySessionStore::default());
        let expiry = fixed_expiry()?;
        let record = SessionRecord {
            id: SessionId::generate()?,
            data: serde_json::json!({"_token":"","value":"x".repeat(300)}),
            expiry_date: expiry,
        };
        let max = record.encode(DEFAULT_MAX_BYTES)?.len();
        store.write(&record, SessionWriteMode::Insert, max).await?;
        let session = Session::with_options(
            Some(record.id.clone()),
            store.clone(),
            Some(Expiry::AtDateTime(expiry)),
            max,
            "test".into(),
        )?;
        session.start().await?;
        let error = session
            .regenerate(true)
            .await
            .err()
            .ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.id()?, record.id);
        assert_eq!(session.all_json()?, record.data);
        assert!(store.load(&record.id, max, expiry).await?.is_some());
        let flash = SessionRecord {
            id: SessionId::generate()?,
            data: serde_json::json!({"_token":"","_flash":{"new":["x"]},"x":"x".repeat(300)}),
            expiry_date: expiry,
        };
        let max = flash.encode(DEFAULT_MAX_BYTES)?.len();
        store.write(&flash, SessionWriteMode::Insert, max).await?;
        let session = Session::with_options(
            Some(flash.id.clone()),
            store.clone(),
            Some(Expiry::AtDateTime(expiry)),
            max,
            "test".into(),
        )?;
        session.start().await?;
        let error = session.save().await.err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(
            session.get_json("_flash.old")?,
            Some(serde_json::json!(["x"]))
        );
        assert_eq!(session.get_json("_flash.new")?, Some(serde_json::json!([])));
        assert_eq!(
            store
                .load(&flash.id, max, expiry)
                .await?
                .ok_or_else(invalid_record)?
                .data,
            flash.data
        );
        Ok(())
    }
}
