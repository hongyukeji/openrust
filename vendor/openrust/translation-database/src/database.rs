//! 数据库翻译来源：有界MVCC物化、同事务修订与提交后尽力失效通知。

use openrust::{
    Error, Result,
    cache::CacheStore,
    database::{
        DatabaseTransaction, Db,
        sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement},
    },
    events::Events,
    support::BoxFuture,
    translation::{
        Locale, TranslationLines, TranslationLoadSession, TranslationLoader, TranslationResource,
        parse_translation_lines, translation_session,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// 管理的翻译变更；禁用行与删除都表示来源缺失，空字符串仍是有效覆盖。
#[derive(Clone, Debug)]
pub enum TranslationMutation {
    /// 写入或更新一个精确资源键。
    Put {
        /// 所属语言、组与命名空间。
        resource: TranslationResource,
        /// 资源内键。
        key: String,
        /// Catalog消息值。
        value: Value,
        /// false表示来源禁用，不返回该条目。
        enabled: bool,
    },
    /// 删除一个精确资源键。
    Delete {
        /// 所属资源。
        resource: TranslationResource,
        /// 资源内键。
        key: String,
    },
}
/// 已提交的修订变化；既有事件监听器可把提示转发到应用选择的共享传输。
#[derive(Clone, Debug)]
pub struct TranslationSourceChanged {
    /// 数据库已提交修订号；不是消息正文或数据库凭据。
    pub revision: u64,
}

impl openrust::events::Event for TranslationSourceChanged {
    const NAME: &'static str = r"OpenRust\TranslationDatabase\Events\TranslationSourceChanged";
}

/// 已提交变更的回执；通知失败不改变数据库提交结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslationWriteOutcome {
    /// 已持久化的修订号。
    pub revision: u64,
    /// 已配置通道均接受提示；队列以入列为准，不表示worker或远端已完成。
    /// false时仍依修订核对收敛。
    pub notifications_delivered: bool,
}

#[derive(Default)]
struct State {
    revision: Option<u64>,
    observed_revision: Option<u64>,
    checked: Option<Instant>,
    checked_hint: u64,
    failed: bool,
}

struct DatabaseSession {
    inner: Arc<dyn TranslationLoadSession>,
    revision: u64,
    owner: Arc<()>,
}
impl TranslationLoadSession for DatabaseSession {
    fn resources(&self) -> &[TranslationResource] {
        self.inner.resources()
    }
    fn load(
        &self,
        locale: &Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines> {
        self.inner.load(locale, group, namespace)
    }
}
/// 显式数据库Loader；没有常驻任务，每次快照单条SELECT读取修订和消息。
#[derive(Clone)]
pub struct DatabaseTranslationLoader {
    db: Db,
    max_rows: usize,
    check_interval: Duration,
    events: Option<Events>,
    notification_cache: Option<(CacheStore, String)>,
    state: Arc<Mutex<State>>,
    hint: Arc<AtomicU64>,
    owner: Arc<()>,
}
impl DatabaseTranslationLoader {
    /// 创建显式来源；默认最多10000行，修订兜底核对间隔30秒。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            max_rows: 10_000,
            check_interval: Duration::from_secs(30),
            events: None,
            notification_cache: None,
            state: Arc::new(Mutex::new(State::default())),
            hint: Arc::new(AtomicU64::new(0)),
            owner: Arc::new(()),
        }
    }
    /// 配置非零快照行数上限及非零核对间隔；不是全局额度或后台定时器。
    pub fn with_limits(mut self, max_rows: usize, check_interval: Duration) -> Result<Self> {
        if max_rows == 0
            || max_rows
                .checked_add(1)
                .and_then(|limit| i64::try_from(limit).ok())
                .is_none()
            || check_interval.is_zero()
        {
            return Err(invalid_resource());
        }
        self.max_rows = max_rows;
        self.check_interval = check_interval;
        Ok(self)
    }
    /// 复用既有事件派发器；在本管理API提交成功后按标准机制派发监听器。
    #[must_use]
    pub fn with_events(mut self, events: Events) -> Self {
        self.events = Some(events);
        self
    }
    /// 复用可选共享缓存传递修订提示；key须由应用按来源隔离，不视缓存为数据库真值。
    pub fn with_shared_cache(mut self, cache: CacheStore, key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        if key.is_empty() || key.len() > 255 || key.chars().any(char::is_control) {
            return Err(invalid_resource());
        }
        self.notification_cache = Some((cache, key));
        Ok(self)
    }
    /// 接收外部通知提示；乱序或重复消息不会令修订倒退，真实性仍由数据库核验。
    pub fn notify_revision(&self, revision: u64) {
        self.hint.fetch_max(revision, Ordering::SeqCst);
    }
    /// 显式重新物化；可用于外部SQL未推进修订但应用已明确知道变更的场景。
    pub async fn refresh(&self) -> Result<Arc<dyn TranslationLoadSession>> {
        self.begin_snapshot().await
    }
    /// 自有事务内原子执行一批文案变更与revision更新；不接受外部pending事务。
    pub async fn write(
        &self,
        mutations: Vec<TranslationMutation>,
    ) -> Result<TranslationWriteOutcome> {
        if mutations.is_empty() || mutations.len() > self.max_rows {
            return Err(invalid_resource());
        }
        for mutation in &mutations {
            validate_mutation(mutation)?;
        }
        let db = self.db.clone();
        let revision = self
            .db
            .transaction(move |transaction| {
                Box::pin(async move { apply_mutations(&db, transaction, mutations).await })
            })
            .await
            .map_err(|_| database_error())?;
        Ok(self.publish_notifications(revision).await)
    }
    /// 重试已提交修订的通知；先向数据库核验，不修改文案或再次增加修订。
    pub async fn retry_notifications(&self, revision: u64) -> Result<TranslationWriteOutcome> {
        if self.current_revision().await? < revision {
            return Err(database_error());
        }
        Ok(self.publish_notifications(revision).await)
    }
    async fn publish_notifications(&self, revision: u64) -> TranslationWriteOutcome {
        self.notify_revision(revision);
        let mut delivered = true;
        if let Some(events) = &self.events {
            // 原事务已提交；尊重应用经公开Events登记的直接及队列监听器。
            delivered &= events
                .dispatch(&TranslationSourceChanged { revision })
                .await
                .is_ok();
        }
        if let Some((cache, key)) = &self.notification_cache {
            delivered &= cache.set(key, &revision, None).await.is_ok();
        }
        TranslationWriteOutcome {
            revision,
            notifications_delivered: delivered,
        }
    }
    async fn materialize(&self) -> Result<(u64, Arc<dyn TranslationLoadSession>)> {
        let revision_table = self.db.quoted_table_name("translation_revisions")?;
        let lines = self.db.quoted_table_name("translation_lines")?;
        let enabled = if self.db.backend()? == DbBackend::Postgres {
            "TRUE"
        } else {
            "1"
        };
        let limit = self.max_rows + 1;
        let sql = format!(
            "SELECT r.revision AS source_revision,l.identity_hash,l.locale,l.group_name,l.namespace,l.line_key,l.value_json FROM {revision_table} r LEFT JOIN {lines} l ON l.enabled={enabled} WHERE r.id=1 ORDER BY l.identity_hash LIMIT {limit}"
        );
        let rows = self
            .db
            .connection()?
            .query_all_raw(Statement::from_string(self.db.backend()?, sql))
            .await
            .map_err(|_| database_error())?;
        materialize_rows(rows, self.max_rows)
    }
    async fn current_revision(&self) -> Result<u64> {
        // 快照与修订高水位来自同一主库；读副本延迟不能被误判成来源修订倒退。
        let value = self
            .db
            .with_primary(async {
                self.db
                    .table("translation_revisions")?
                    .where_eq("id", 1_i32)?
                    .value::<i64>("revision")
                    .await
                    .map_err(|_| database_error())?
                    .ok_or_else(database_error)
            })
            .await??;
        u64::try_from(value).map_err(|_| invalid_resource())
    }

    async fn check_refresh(&self, published: Option<u64>) -> Result<bool> {
        {
            let state = self.state.lock().await;
            if !revision_check_due(
                &state,
                published.or(state.revision),
                self.hint.load(Ordering::SeqCst),
                self.check_interval,
            )? {
                return Ok(false);
            }
        }
        // 外部缓存仅提供提示；用户后端在状态锁外执行，不取代数据库核验。
        if let Some((cache, key)) = &self.notification_cache
            && let Ok(Some(revision)) = cache.get::<u64>(key).await
        {
            self.notify_revision(revision);
        }
        let mut state = self.state.lock().await;
        let applied = published.or(state.revision);
        if !revision_check_due(
            &state,
            applied,
            self.hint.load(Ordering::SeqCst),
            self.check_interval,
        )? {
            return Ok(false);
        }
        let Some(applied) = applied else {
            return Ok(true);
        };
        let current = match self.current_revision().await {
            Ok(revision) => revision,
            Err(error) => {
                state.checked = Some(Instant::now());
                state.failed = true;
                return Err(error);
            }
        };
        if current < applied
            || state.revision.is_some_and(|revision| current < revision)
            || state
                .observed_revision
                .is_some_and(|revision| current < revision)
        {
            state.checked = Some(Instant::now());
            state.failed = true;
            return Err(database_error());
        }
        state.checked = Some(Instant::now());
        state.failed = false;
        state.checked_hint = self.hint.load(Ordering::SeqCst);
        // 这是已核验的来源高水位，不是任一 I18n 父实例的发布游标。
        state.observed_revision = Some(current);
        Ok(current > applied)
    }
}
impl TranslationLoader for DatabaseTranslationLoader {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            let (revision, session) = match self.materialize().await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    state.checked = Some(Instant::now());
                    state.failed = true;
                    return Err(error);
                }
            };
            if state.revision.is_some_and(|old| revision < old)
                || state.observed_revision.is_some_and(|old| revision < old)
            {
                state.checked = Some(Instant::now());
                state.failed = true;
                return Err(database_error());
            }
            state.revision = Some(revision);
            state.observed_revision = Some(revision);
            state.checked = Some(Instant::now());
            state.failed = false;
            Ok(Arc::new(DatabaseSession {
                inner: session,
                revision,
                owner: Arc::clone(&self.owner),
            }) as Arc<dyn TranslationLoadSession>)
        })
    }
    fn needs_refresh_since<'a>(
        &'a self,
        published: &'a dyn TranslationLoadSession,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let session = (published as &dyn std::any::Any)
                .downcast_ref::<DatabaseSession>()
                .ok_or_else(database_error)?;
            if !Arc::ptr_eq(&self.owner, &session.owner) {
                return Err(database_error());
            }
            self.check_refresh(Some(session.revision)).await
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(self.check_refresh(None))
    }
}
fn revision_check_due(
    state: &State,
    applied: Option<u64>,
    hint: u64,
    interval: Duration,
) -> Result<bool> {
    let due = state.checked.is_none_or(|time| time.elapsed() >= interval);
    if state.failed && !due {
        return Err(database_error());
    }
    Ok(due
        || applied.is_none_or(|applied| {
            state
                .observed_revision
                .is_some_and(|current| current > applied)
                || hint > applied.max(state.checked_hint)
        }))
}

fn materialize_rows(
    rows: Vec<QueryResult>,
    max_rows: usize,
) -> Result<(u64, Arc<dyn TranslationLoadSession>)> {
    if rows.is_empty() {
        return Err(database_error());
    }
    if rows.len() > max_rows {
        return Err(Error::custom(
            "i18n_database_row_limit",
            "数据库翻译超过配置行数上限",
        ));
    }
    let mut revision = None;
    let mut seen = BTreeSet::new();
    let mut resources: BTreeMap<TranslationResource, TranslationLines> = BTreeMap::new();
    for row in rows {
        let current: i64 = row
            .try_get("", "source_revision")
            .map_err(|_| database_error())?;
        let current = u64::try_from(current).map_err(|_| invalid_resource())?;
        if revision.is_some_and(|old| old != current) {
            return Err(invalid_resource());
        }
        revision = Some(current);
        let hash: Option<String> = row
            .try_get("", "identity_hash")
            .map_err(|_| database_error())?;
        let Some(hash) = hash else {
            continue;
        };
        let raw_locale = text(&row, "locale")?;
        let locale = Locale::new(&raw_locale).map_err(|_| invalid_resource())?;
        if raw_locale != locale.as_str() {
            return Err(invalid_resource());
        }
        let namespace = text(&row, "namespace")?;
        let resource = TranslationResource {
            locale,
            group: text(&row, "group_name")?,
            namespace: if namespace.is_empty() {
                None
            } else {
                Some(namespace)
            },
        };
        let key = text(&row, "line_key")?;
        if hash != identity_hash(&resource, &key)? || !seen.insert(hash) {
            return Err(invalid_resource());
        }
        let value = parse_value(text(&row, "value_json")?.as_bytes())?;
        if resources
            .entry(resource)
            .or_default()
            .insert(key, value)
            .is_some()
        {
            return Err(invalid_resource());
        }
    }
    Ok((
        revision.ok_or_else(database_error)?,
        translation_session(resources)?,
    ))
}
fn text(row: &QueryResult, field: &str) -> Result<String> {
    row.try_get("", field).map_err(|_| database_error())
}
fn identity_hash(resource: &TranslationResource, key: &str) -> Result<String> {
    let canonical = serde_json::to_vec(&(
        resource.locale.as_str(),
        &resource.group,
        resource.namespace.as_deref().unwrap_or(""),
        key,
    ))
    .map_err(|_| invalid_resource())?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}
fn validate_mutation(mutation: &TranslationMutation) -> Result<()> {
    let (resource, key, value) = match mutation {
        TranslationMutation::Put {
            resource,
            key,
            value,
            ..
        } => (resource, key, value.clone()),
        TranslationMutation::Delete { resource, key } => {
            (resource, key, Value::String(String::new()))
        }
    };
    translation_session(BTreeMap::from([(
        resource.clone(),
        BTreeMap::from([(key.clone(), value)]),
    )]))
    .map(|_| ())
}
async fn apply_mutations(
    db: &Db,
    transaction: &DatabaseTransaction,
    mutations: Vec<TranslationMutation>,
) -> Result<u64> {
    let revision_table = db.quoted_table_name("translation_revisions")?;
    let backend = transaction.get_database_backend();
    let result = transaction
        .execute_unprepared(&format!(
            "UPDATE {revision_table} SET revision=revision+1 WHERE id=1 AND revision>=0 AND revision<9223372036854775807"
        ))
        .await
        .map_err(|_| database_error())?;
    if result.rows_affected() != 1 {
        return Err(database_error());
    }
    for mutation in mutations {
        let (resource, key) = match &mutation {
            TranslationMutation::Put { resource, key, .. }
            | TranslationMutation::Delete { resource, key } => (resource, key),
        };
        let hash = identity_hash(resource, key)?;
        let existing = db
            .table("translation_lines")?
            .using_transaction(transaction)
            .where_eq("identity_hash", hash.clone())?
            .first()
            .await?;
        if let Some(existing) = existing
            && (text(&existing, "identity_hash")? != hash
                || text(&existing, "locale")? != resource.locale.as_str()
                || text(&existing, "group_name")? != resource.group
                || text(&existing, "namespace")? != resource.namespace.as_deref().unwrap_or("")
                || text(&existing, "line_key")? != *key)
        {
            return Err(invalid_resource());
        }
        match mutation {
            TranslationMutation::Delete { .. } => {
                db.table("translation_lines")?
                    .using_transaction(transaction)
                    .where_eq("identity_hash", hash)?
                    .delete()
                    .await?;
            }
            TranslationMutation::Put {
                resource,
                key,
                value,
                enabled,
            } => {
                let encoded = serde_json::to_string(&value).map_err(|_| invalid_resource())?;
                db.table("translation_lines")?
                    .using_transaction(transaction)
                    .upsert(
                        vec![vec![
                            ("identity_hash", hash.into()),
                            ("locale", resource.locale.as_str().into()),
                            ("group_name", resource.group.into()),
                            ("namespace", resource.namespace.unwrap_or_default().into()),
                            ("line_key", key.into()),
                            ("value_json", encoded.into()),
                            ("enabled", enabled.into()),
                        ]],
                        &["identity_hash"],
                        &["value_json", "enabled"],
                    )
                    .await?;
            }
        }
    }
    let row = transaction
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT revision FROM {revision_table} WHERE id=1"),
        ))
        .await
        .map_err(|_| database_error())?
        .ok_or_else(database_error)?;
    let revision: i64 = row.try_get("", "revision").map_err(|_| database_error())?;
    u64::try_from(revision).map_err(|_| database_error())
}
fn database_error() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "i18n_database_source_unavailable",
        "数据库翻译来源不可用",
    )
}

#[cfg(all(
    test,
    any(feature = "db-mysql", feature = "db-postgres", feature = "db-sqlite")
))]
#[path = "database_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "database_parse_tests.rs"]
mod parse_tests;

// 以唯一核心解析器保留嵌套重复键拒绝；数据库内容仅作为JSON输入，不是SQL模板。
fn parse_value(bytes: &[u8]) -> Result<Value> {
    let mut wrapped = b"{\"value\":".to_vec();
    wrapped.extend_from_slice(bytes);
    wrapped.push(b'}');
    let mut parsed = parse_translation_lines(&wrapped)?;
    // 原列必须恰好一个 JSON 值；逗号尾注入不能成为包装对象的额外成员。
    if parsed.len() != 1 {
        return Err(invalid_resource());
    }
    parsed.remove("value").ok_or_else(invalid_resource)
}
fn invalid_resource() -> Error {
    Error::custom("i18n_translation_resource_invalid", "翻译资源结构无效")
}
