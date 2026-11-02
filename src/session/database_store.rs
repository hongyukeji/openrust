//! 原生数据库 handler 接入唯一 Session 状态机；JSON 只编码 Rust 属性数组。

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "database_store/tests.rs"]
mod tests;

use std::sync::Arc;

use time::OffsetDateTime;
use tokio::sync::Mutex;

use super::store::{guard_json_depth, invalid_record, json_bytes, unavailable};
use super::{
    DatabaseSessionHandler, DatabaseSessionMetadata, SessionId, SessionRecord, SessionStore,
    SessionStoreGuarantees, SessionStoreSemantics, SessionWriteMode, SessionWriteOutcome,
};
use crate::{Result, core::BoxFuture, db::Db};

/// Laravel database driver 的属性适配器；每个受管 Session 独占 handler exists 状态。
///
/// 属性以有界 JSON 编码而非 PHP serialize；表内 Base64、ID、活动时间和写入行为由
/// `DatabaseSessionHandler` 决定。绝对截止与防复活能力由 `GuardedDatabaseSessionStore` 提供。
pub struct DatabaseSessionStore {
    db: Db,
    table: String,
    minutes: i64,
    metadata: DatabaseSessionMetadata,
    handler: Mutex<Option<DatabaseSessionHandler>>,
    #[cfg(feature = "auth")]
    session_user_metadata: bool,
}

impl std::fmt::Debug for DatabaseSessionStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseSessionStore([REDACTED])")
    }
}

impl DatabaseSessionStore {
    /// 复用应用 Db，默认 sessions 表与 120 分钟；构造不执行数据库 I/O。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            table: "sessions".into(),
            minutes: 120,
            metadata: DatabaseSessionMetadata::default(),
            handler: Mutex::new(None),
            #[cfg(feature = "auth")]
            session_user_metadata: false,
        }
    }

    /// 使用最终配置的逻辑表名和分钟数；不会创建表或复制数据库连接池。
    ///
    /// # Errors
    ///
    /// 表名无效、分钟非正或溢出时拒绝。
    pub fn configured(mut self, table: impl Into<String>, minutes: u64) -> Result<Self> {
        let table = table.into();
        self.db
            .table_names()
            .resolve(&table)
            .map_err(|_| unavailable())?;
        let minutes = i64::try_from(minutes).map_err(|_| unavailable())?;
        if minutes <= 0 || minutes.checked_mul(60).is_none() {
            return Err(unavailable());
        }
        self.table = table;
        self.minutes = minutes;
        Ok(self)
    }

    fn fresh(&self, metadata: DatabaseSessionMetadata) -> Self {
        Self {
            db: self.db.clone(),
            table: self.table.clone(),
            minutes: self.minutes,
            metadata,
            handler: Mutex::new(None),
            #[cfg(feature = "auth")]
            session_user_metadata: self.session_user_metadata,
        }
    }

    #[cfg(feature = "auth")]
    pub(crate) fn with_session_user_metadata(mut self, enabled: bool) -> Self {
        self.session_user_metadata = enabled;
        self
    }

    fn handler(&self, maximum: usize) -> Result<DatabaseSessionHandler> {
        DatabaseSessionHandler::new(self.db.clone(), Some(self.table.clone()), self.minutes)?
            .with_metadata(self.metadata.clone())
            .with_max_bytes(maximum)
    }
}

impl SessionStore for DatabaseSessionStore {
    fn guarantees(&self) -> SessionStoreGuarantees {
        SessionStoreGuarantees::NativeDatabase
    }
    fn semantics(&self) -> SessionStoreSemantics {
        SessionStoreSemantics::NativeDatabase
    }
    fn supports_session_authentication(&self, _cookie_name: &str) -> bool {
        true
    }
    fn activity_lifetime_seconds(&self) -> Option<i64> {
        self.minutes.checked_mul(60)
    }

    fn for_session(&self) -> Result<Option<Arc<dyn SessionStore>>> {
        Ok(Some(Arc::new(self.fresh(self.metadata.clone()))))
    }

    fn reset_exists(&self) -> Result<()> {
        let mut handler = self.handler.try_lock().map_err(|_| unavailable())?;
        if let Some(handler) = handler.as_mut() {
            handler.set_exists(false);
        }
        Ok(())
    }

    fn accepts_request(&self) -> bool {
        true
    }

    fn bind_request(
        &self,
        context: super::SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        Ok(Some(Arc::new(
            self.fresh(context.database_metadata().clone()),
        )))
    }

    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            self.handler(super::DEFAULT_MAX_BYTES)?
                .gc(lifetime_seconds)
                .await
        })
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        maximum: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let mut slot = self.handler.lock().await;
            if slot.is_none() {
                *slot = Some(self.handler(maximum)?);
            }
            let handler = slot.as_mut().ok_or_else(unavailable)?;
            handler.set_max_bytes(maximum)?;
            let bytes = handler.read(id.as_str()).await?;
            if !handler.exists() {
                return Ok(None);
            }
            guard_json_depth(&bytes)?;
            let data = match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(data) if data.is_array() || data.is_object() => data,
                _ => serde_json::Value::Array(Vec::new()),
            };
            let record = SessionRecord {
                id: id.clone(),
                data,
                expiry_date: empty_expiry,
            };
            record.validate_for(SessionStoreSemantics::NativeDatabase, maximum)?;
            Ok(Some(record))
        })
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        _mode: SessionWriteMode,
        maximum: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            record.validate()?;
            let bytes = json_bytes(&record.data, maximum)?;
            let mut slot = self.handler.lock().await;
            if slot.is_none() {
                *slot = Some(self.handler(maximum)?);
            }
            slot.as_mut()
                .ok_or_else(invalid_record)?
                .set_max_bytes(maximum)?;
            #[cfg(feature = "auth")]
            if self.session_user_metadata {
                // 只在真实认证服务绑定后投影当前 Session guard 的身份；不依据 feature 伪造 Guard。
                let user = crate::support::data::get(&record.data, crate::auth::AUTH_USER_ID_KEY)
                    .filter(|value| !value.is_null())
                    .map(|value| value.as_i64().ok_or_else(invalid_record))
                    .transpose()?;
                let mut metadata = self.metadata.clone();
                metadata.user_id = Some(sea_orm::Value::BigInt(user));
                slot.as_mut()
                    .ok_or_else(invalid_record)?
                    .set_metadata(metadata);
            }
            slot.as_mut()
                .ok_or_else(invalid_record)?
                .write(record.id.as_str(), &bytes)
                .await?;
            Ok(SessionWriteOutcome::Written)
        })
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.handler(super::DEFAULT_MAX_BYTES)?
                .destroy(id.as_str())
                .await?;
            Ok(())
        })
    }
}
