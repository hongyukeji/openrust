//! 唯一请求状态：同步属性锁不跨await，I/O门控由取消可回收的guard持有。

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;

use super::{
    DEFAULT_MAX_BYTES, SessionId, SessionRecord, SessionRequestContext, SessionStore,
    SessionWriteMode, SessionWriteOutcome,
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
}
impl Driver {
    fn new(store: Arc<dyn SessionStore>) -> Self {
        let needs_request = store.needs_request();
        Self {
            identity: store.clone(),
            actual: store,
            needs_request,
            bound: !needs_request,
        }
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
    expiry: Option<Expiry>,
    started: bool,
    existing: bool,
    busy: bool,
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
    max_bytes: usize,
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
/// session.put("cart.quantity", 2)?;
/// session.flash("notice", "已保存")?;
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
        if max_bytes == 0 || max_bytes > DEFAULT_MAX_BYTES {
            return Err(limit_error());
        }
        let id = match id {
            Some(id) => id,
            None => SessionId::generate()?,
        };
        let driver = Driver::new(store);
        Ok(Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    id,
                    name,
                    attributes: Attributes::new(Value::Array(Vec::new()))?,
                    expiry,
                    started: false,
                    existing: false,
                    busy: false,
                    revoked: false,
                    closed: false,
                    driver,
                    origin: None,
                    intent: CreateIntent::SystemRandom,
                    destroyed: HashSet::new(),
                }),
                io: AsyncMutex::new(()),
                max_bytes,
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
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        let mut candidate = state.attributes.clone();
        let value = apply(&mut candidate)?;
        SessionRecord {
            id: state.id.clone(),
            data: candidate.all(),
            expiry_date: state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
        }
        .encode(self.inner.max_bytes)?;
        state.attributes = candidate;
        Ok(value)
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
            _gate: gate,
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
            _gate: gate,
        })
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
        if state.destroyed.contains(&id) {
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
        {
            let state = self.lock()?;
            state.ensure_mutable()?;
            if Arc::ptr_eq(&state.driver.identity, &handler)
                || Arc::ptr_eq(&state.driver.actual, &handler)
            {
                return Ok(handler);
            }
        }
        // 用户驱动代码和旧 Arc 的最终释放均不在属性锁内。
        let driver = Driver::new(handler.clone());
        let old = {
            let mut state = self.lock()?;
            state.ensure_mutable()?;
            let old = std::mem::replace(&mut state.driver, driver);
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
    pub(crate) fn require_authenticated_storage(&self) -> Result<()> {
        let (driver, name) = {
            let state = self.lock()?;
            state.ensure_mutable()?;
            (state.driver.clone(), state.name.clone())
        };
        driver.require_authenticated(&name)
    }

    /// 当前配置驱动是否需要请求上下文。
    pub fn handler_needs_request(&self) -> Result<bool> {
        Ok(self.lock()?.driver.needs_request)
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
        if !driver.needs_request {
            return Ok(self);
        }
        let actual = driver.identity.bind_request(context)?.ok_or_else(|| {
            extension_error(
                "session_request_binding_invalid",
                "Session handler did not bind the request",
            )
        })?;
        if actual.guarantees() != driver.identity.guarantees() {
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

    /// 完整属性；返回有序PHP数组表示的独立值。
    pub fn all(&self) -> Result<Value> {
        Ok(self.lock()?.attributes.all())
    }
    /// `Arr::get` 读取；存在 null 返回 `Some(Null)`，星号不展开。
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        self.lock()?.attributes.get(key)
    }
    /// 读取 `_previous.url` 原值；沿 [`Self::get`] 区分缺失与显式 null。
    ///
    /// Rust 返回原始 `Value`，不将公开 `put` 写入的非字符串值强制转换为 URL。
    pub fn previous_url(&self) -> Result<Option<Value>> {
        self.get("_previous.url")
    }
    /// 写入上一请求 URL；沿 [`Self::put`] 保留空串并执行相同状态与预算检查。
    pub fn set_previous_url(&self, url: &str) -> Result<&Self> {
        self.put("_previous.url", url)
    }
    /// 读取 `_previous.route` 原值；缺失为 `None`，匿名路由显式 null 为 `Some(Null)`。
    ///
    /// 与 [`Self::previous_url`] 相同，保留原始 `Value`，不额外强制字符串类型。
    pub fn previous_route(&self) -> Result<Option<Value>> {
        self.get("_previous.route")
    }
    /// 写入上一请求路由名；Rust 的 `None` 对应持久属性中的显式 null，不删除键。
    pub fn set_previous_route(&self, route: Option<&str>) -> Result<&Self> {
        self.put("_previous.route", route)
    }
    /// 判断上一请求 URL 是否非 null；false、零和空串仍为有值，不解析或构造 URI。
    pub fn has_previous_uri(&self) -> Result<bool> {
        Ok(self.previous_url()?.is_some_and(|value| !value.is_null()))
    }
    /// 只有缺失才返回默认值；已有null保持null。
    pub fn get_or(&self, key: &str, default: Value) -> Result<Value> {
        Ok(self.get(key)?.unwrap_or(default))
    }
    /// 只有缺失才执行惰性默认回调；回调执行时不持属性锁。
    pub fn get_or_else(&self, key: &str, default: impl FnOnce() -> Value) -> Result<Value> {
        Ok(self.get(key)?.unwrap_or_else(default))
    }
    /// 非 null 原值直接返回；缺失/null 才计算并存储回调结果。
    ///
    /// 回调及序列化不持状态锁，允许重入；其自身副作用不回滚。最终写入重新检查
    /// 当前状态与完整记录预算，不承诺跨请求的分布式原子计算。
    pub fn remember<T: Serialize>(
        &self,
        key: &str,
        callback: impl FnOnce() -> Result<T>,
    ) -> Result<Value> {
        if let Some(value) = self.get(key)?.filter(|value| !value.is_null()) {
            return Ok(value);
        }
        self.lock()?.ensure_mutable()?;
        let value = super::store::value_from(&callback()?, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.put(key, value.clone()))?;
        Ok(value)
    }

    /// 显式Rust类型读取；不替换Laravel get的Value/null语义。
    pub fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        self.get(key)?
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
    pub fn only(&self, keys: &[&str]) -> Result<Value> {
        self.lock()?.attributes.only(keys)
    }
    /// 按 `Arr::forget` 排除指定键，完整 literal 键优先。
    pub fn except(&self, keys: &[&str]) -> Result<Value> {
        self.lock()?.attributes.except(keys)
    }
    /// 写入点路径；保持空白/空串，拒绝非有限数及越限值。
    pub fn put<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.put(key, value))
    }
    /// 按调用顺序批量dot写入，整个操作原子完成。
    pub fn put_many(&self, values: &[(String, Value)]) -> Result<&Self> {
        self.mutate(|attributes| attributes.put_many(values))
    }
    /// 固定Store的replace是put合并，不清除其他属性。
    pub fn replace(&self, values: &[(String, Value)]) -> Result<&Self> {
        self.put_many(values)
    }
    /// 删除键，不删除因此变空的父数组。
    pub fn forget(&self, keys: &[&str]) -> Result<&Self> {
        self.mutate(|attributes| attributes.forget(keys))
    }
    /// 读取并删除；null与缺失仍可区分。
    pub fn pull(&self, key: &str) -> Result<Option<Value>> {
        self.mutate_value(|attributes| attributes.pull(key))
    }
    /// Laravel remove 同样读取并删除；None 表示缺失，Some(Null) 保留已存 null。
    pub fn remove(&self, key: &str) -> Result<Option<Value>> {
        self.pull(key)
    }

    /// 追加 PHP 数组值；缺失、null 或 false 从空数组开始。
    pub fn push<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
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
    pub fn pull_or(&self, key: &str, default: Value) -> Result<Value> {
        Ok(self.pull(key)?.unwrap_or(default))
    }
    /// 清除全部属性；不自动再生token。
    pub fn flush(&self) -> Result<&Self> {
        self.mutate(Attributes::flush)
    }
    /// 标记下一请求仍可读取的flash值。
    pub fn flash<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.flash(key, value))
    }
    /// 仅当前保存前可见；本次save会删除，reflash可显式延长。
    pub fn now<T: Serialize>(&self, key: &str, value: T) -> Result<&Self> {
        let value = super::store::value_from(&value, self.inner.max_bytes)?;
        self.mutate(|attributes| attributes.now(key, value))
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
        self.get("_token")
    }
    /// 用系统随机源重新生成40位token。
    pub fn regenerate_token(&self) -> Result<&Self> {
        self.put("_token", random_token()?)
    }
    /// 当前显式期限策略。
    pub fn expiry(&self) -> Result<Option<Expiry>> {
        Ok(self.lock()?.expiry)
    }
    /// 修改后续保存使用的期限。
    pub fn set_expiry(&self, expiry: Option<Expiry>) -> Result<&Self> {
        let mut state = self.lock()?;
        state.ensure_mutable()?;
        SessionRecord {
            id: state.id.clone(),
            data: state.attributes.all(),
            expiry_date: expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
        }
        .encode(self.inner.max_bytes)?;
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
            if state.pending_transfer() {
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
        let old_origin = {
            let mut state = self.lock()?;
            let mut attributes = state.attributes.clone();
            let mut found = false;
            let mut budget_expiry = expiry;
            if let Some(record) = record {
                if record.id != id {
                    return Err(invalid_record());
                }
                record.encode(self.inner.max_bytes)?;
                if record.expiry_date > OffsetDateTime::now_utc() {
                    budget_expiry = record.expiry_date;
                    attributes =
                        Attributes::new(super::merge_attributes(attributes.all(), record.data)?)?;
                    found = true;
                } else if existing {
                    state.revoked = true;
                    return Err(write_conflict());
                }
            } else if existing {
                state.revoked = true;
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
                data: attributes.all(),
                expiry_date: budget_expiry,
            }
            .encode(self.inner.max_bytes)?;
            state.attributes = attributes;
            state.existing = found;
            state.started = true;
            let origin = found.then_some(Binding { id, driver });
            std::mem::replace(&mut state.origin, origin)
        };
        drop(old_origin);
        Ok(true)
    }

    /// 老化 flash 后保存；显式目标碰撞不覆盖、不换成其他随机 ID。
    pub async fn save(&self) -> Result<()> {
        let _operation = self.operation().await?;
        let (mut record, mode, driver, origin, intent) = {
            let mut state = self.lock()?;
            let driver = state.driver.clone();
            driver.ready()?;
            let mut candidate = state.attributes.clone();
            candidate.age()?;
            let record = SessionRecord {
                id: state.id.clone(),
                data: candidate.all(),
                expiry_date: state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
            };
            record.encode(self.inner.max_bytes)?;
            state.attributes = candidate;
            let origin = state
                .origin
                .as_ref()
                .filter(|origin| state.pending_transfer() && !state.destroyed.contains(&origin.id))
                .cloned();
            (
                record,
                if state.existing {
                    SessionWriteMode::Update
                } else {
                    SessionWriteMode::Insert
                },
                driver,
                origin,
                state.intent,
            )
        };
        #[cfg(feature = "auth")]
        if crate::support::data::get(&record.data, crate::auth::AUTH_USER_ID_KEY)
            .is_some_and(|value| !value.is_null())
        {
            let name = self.lock()?.name.clone();
            driver.require_authenticated(&name)?;
        }
        let store = driver.ready()?;
        if let Some(origin) = origin {
            let source = origin.driver.ready()?;
            let record = source
                .load(&origin.id, self.inner.max_bytes, record.expiry_date)
                .await?;
            let live = match record {
                Some(record) => {
                    if record.id != origin.id {
                        return Err(invalid_record());
                    }
                    record.encode(self.inner.max_bytes)?;
                    record.expiry_date > OffsetDateTime::now_utc()
                }
                None => false,
            };
            if !live {
                self.lock()?.revoked = true;
                return Err(write_conflict());
            }
        }
        // source 检查与目标写入不构成跨后端事务；显式复制不承诺线性撤销。
        for attempt in 0..3 {
            record.encode(self.inner.max_bytes)?;
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
        let (old_id, candidate, driver) = {
            let state = self.lock()?;
            if destroy && state.pending_transfer() {
                return Err(extension_error(
                    "session_rebind_pending",
                    "Save the explicit Session transfer before destroying",
                ));
            }
            if destroy && !state.destroyed.contains(&state.id) && state.destroyed.len() >= 1024 {
                return Err(limit_error());
            }
            if state.destroyed.contains(&id) {
                return Err(write_conflict());
            }
            let mut candidate = state.attributes.clone();
            if flush {
                candidate.flush()?;
            }
            if regenerate {
                candidate.put("_token", Value::String(random_token()?))?;
            }
            SessionRecord {
                id: id.clone(),
                data: candidate.all(),
                expiry_date: state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
            }
            .encode(self.inner.max_bytes)?;
            (state.id.clone(), candidate, state.driver.clone())
        };
        // 候选全部验证后才删除持久记录；IO失败本身不承诺回滚外部副作用。
        let store = driver.ready()?;
        if destroy {
            store.delete(&old_id).await?;
        }
        let old_origin = {
            let mut state = self.lock()?;
            if destroy {
                state.destroyed.insert(old_id);
            }
            state.id = id;
            state.attributes = candidate;
            state.existing = false;
            state.intent = CreateIntent::SystemRandom;
            // invalidate 已清空全部来源属性；tombstone 仍禁止回到销毁的 ID。
            if flush { state.origin.take() } else { None }
        };
        drop(old_origin);
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
        if self.busy {
            return Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_operation_in_progress",
                "Session I/O is in progress",
            ));
        }
        Ok(())
    }
}
struct Operation<'a> {
    inner: &'a Inner,
    _gate: tokio::sync::MutexGuard<'a, ()>,
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.busy = false;
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counted(Arc<AtomicUsize>, String);
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
        session.put("a", Counted(count.clone(), "a".repeat(450)))?;
        let before = session.all()?;
        let error = session
            .put("b", Counted(count.clone(), "b".repeat(450)))
            .err()
            .ok_or_else(invalid_record)?;
        assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(session.all()?, before);
        assert!(session.flash("b", "b".repeat(450)).is_err());
        assert_eq!(session.all()?, before);
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
        session.put("local", "l".repeat(450))?;
        let before = session.all()?;
        let error = session.start().await.err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.all()?, before);
        let exact = record.encode(DEFAULT_MAX_BYTES)?.len();
        let loaded =
            Session::with_options(Some(record.id.clone()), store, None, exact, "test".into())?;
        loaded.start().await?;
        assert_eq!(loaded.all()?, record.data);
        // 值不变、仅新增flash元数据也必须计入同一预算。
        assert!(loaded.flash("persisted", "p".repeat(450)).is_err());
        assert_eq!(loaded.all()?, record.data);
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
        assert_eq!(needs_token.all()?, serde_json::json!([]));
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
        session.put_many(&[
            ("0".into(), serde_json::json!(0)),
            ("1".into(), serde_json::json!("x".repeat(300))),
        ])?;
        let before = session.all()?;
        let error = session.pull("0").err().ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.all()?, before);
        let far = OffsetDateTime::from_unix_timestamp(253_402_300_000).map_err(Error::internal)?;
        let error = session
            .set_expiry(Some(Expiry::AtDateTime(far)))
            .err()
            .ok_or_else(invalid_record)?;
        assert_eq!(error.code(), "session_limits_exceeded");
        assert_eq!(session.expiry_date()?, expiry);
        assert_eq!(session.all()?, before);
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
        assert_eq!(session.all()?, record.data);
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
        assert_eq!(session.all()?, flash.data);
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
