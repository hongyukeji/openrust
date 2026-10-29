//! 唯一请求状态：同步属性锁不跨await，I/O门控由取消可回收的guard持有。

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;

use super::{
    DEFAULT_MAX_BYTES, SessionId, SessionRecord, SessionStore, SessionWriteMode,
    SessionWriteOutcome,
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

#[allow(
    clippy::struct_excessive_bools,
    reason = "启动、持久存在、I/O占用、撤销和关闭是可并存的独立生命周期状态"
)]
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
}
struct Inner {
    state: Mutex<State>,
    io: AsyncMutex<()>,
    store: Arc<dyn SessionStore>,
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
                }),
                io: AsyncMutex::new(()),
                store,
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
    /// 返回当前实际驱动的同一 Arc，供可信应用公开扩展。
    ///
    /// 直接操作驱动不参与此句柄的属性、忙碌和撤销状态机，也不提供托管操作的原子性。
    #[must_use]
    pub fn handler(&self) -> Arc<dyn SessionStore> {
        self.inner.store.clone()
    }

    /// 完整属性；返回有序PHP数组表示的独立值。
    pub fn all(&self) -> Result<Value> {
        Ok(self.lock()?.attributes.all())
    }
    /// `Arr::get` 读取；存在 null 返回 `Some(Null)`，星号不展开。
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        self.lock()?.attributes.get(key)
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

    /// 立即读取store，与已有内存属性顶层合并；已有持久键覆盖同名内存键。
    pub async fn start(&self) -> Result<bool> {
        let _operation = self.operation().await?;
        let (id, expiry) = {
            let state = self.lock()?;
            (
                state.id.clone(),
                state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
            )
        };
        let record = self
            .inner
            .store
            .load(&id, self.inner.max_bytes, expiry)
            .await?;
        let mut state = self.lock()?;
        let mut attributes = state.attributes.clone();
        let existing;
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
                existing = true;
            } else if state.existing {
                state.revoked = true;
                return Err(write_conflict());
            } else {
                existing = false;
            }
        } else if state.existing {
            // 重复start不能把已撤销的旧认证bag变成一个可Insert的新记录。
            state.revoked = true;
            return Err(write_conflict());
        } else {
            existing = false;
        }
        if attributes
            .get("_token")?
            .is_none_or(|value| value.is_null())
        {
            attributes.put("_token", Value::String(random_token()?))?;
        }
        SessionRecord {
            id: state.id.clone(),
            data: attributes.all(),
            expiry_date: budget_expiry,
        }
        .encode(self.inner.max_bytes)?;
        state.attributes = attributes;
        state.existing = existing;
        state.started = true;
        Ok(true)
    }

    /// 每次调用都先老化flash再持久化；未修改/5xx也不跳过。
    pub async fn save(&self) -> Result<()> {
        let _operation = self.operation().await?;
        let (mut record, mode) = {
            let mut state = self.lock()?;
            let mut candidate = state.attributes.clone();
            candidate.age()?;
            let record = SessionRecord {
                id: state.id.clone(),
                data: candidate.all(),
                expiry_date: state.expiry.unwrap_or(Expiry::OnSessionEnd).date()?,
            };
            record.encode(self.inner.max_bytes)?;
            // 资源检查通过才发布age；真正write失败仍保留与源相同的已老化状态。
            state.attributes = candidate;
            (
                record,
                if state.existing {
                    SessionWriteMode::Update
                } else {
                    SessionWriteMode::Insert
                },
            )
        };
        for attempt in 0..3 {
            record.encode(self.inner.max_bytes)?;
            match self
                .inner
                .store
                .write(&record, mode, self.inner.max_bytes)
                .await?
            {
                SessionWriteOutcome::Written => {
                    let mut state = self.lock()?;
                    state.existing = true;
                    state.started = false;
                    return Ok(());
                }
                SessionWriteOutcome::Conflict
                    if mode == SessionWriteMode::Insert && attempt < 2 =>
                {
                    record.id = SessionId::generate()?;
                    self.lock()?.id = record.id.clone();
                }
                SessionWriteOutcome::Missing | SessionWriteOutcome::Conflict => {
                    self.lock()?.revoked = true;
                    return Err(write_conflict());
                }
            }
        }
        Err(write_conflict())
    }

    async fn change_id(&self, destroy: bool, regenerate: bool, flush: bool) -> Result<bool> {
        let _operation = self.operation().await?;
        let id = SessionId::generate()?;
        let (old_id, candidate) = {
            let state = self.lock()?;
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
            (state.id.clone(), candidate)
        };
        // 候选全部验证后才删除持久记录；IO失败本身不承诺回滚外部副作用。
        if destroy {
            self.inner.store.delete(&old_id).await?;
        }
        let mut state = self.lock()?;
        state.id = id;
        state.attributes = candidate;
        state.existing = false;
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
