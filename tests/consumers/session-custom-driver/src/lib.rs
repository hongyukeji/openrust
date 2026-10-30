//! 外部应用只用公开接口实现 Session store，并验证共享持久记录与撤销。
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use openrust::Result;
use openrust::core::BoxFuture;
use openrust::session::{
    MemorySessionStore, SessionId, SessionRecord, SessionStore, SessionWriteMode,
    SessionWriteOutcome,
};
use time::OffsetDateTime;

/// 外部 crate 的 store 实现；委托已有有界内存 driver，不复制编码逻辑。
#[derive(Clone, Default)]
pub struct ExternalSessionStore {
    inner: MemorySessionStore,
}

impl SessionStore for ExternalSessionStore {
    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        self.inner.gc(lifetime_seconds)
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.inner.load(id, max_bytes, empty_expiry)
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        self.inner.write(record, mode, max_bytes)
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.inner.delete(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use openrust::Result;
    use openrust::session::{Expiry, Session, SessionStore};
    use serde_json::json;
    use time::Duration;

    use super::ExternalSessionStore;

    #[tokio::test]
    async fn external_store_preserves_session_and_rejects_revoked_update() -> Result<()> {
        let store = Arc::new(ExternalSessionStore::default());
        let expiry = Some(Expiry::OnInactivity(Duration::hours(1)));
        let first = Session::new(None, store.clone(), expiry)?;
        let erased: Arc<dyn SessionStore> = store.clone();
        assert!(Arc::ptr_eq(&erased, &first.handler()?));
        first.handler()?.delete(&first.id()?).await?;
        let id = first.id()?;
        assert_eq!(id.as_str().len(), 40);
        assert!(id.as_str().bytes().all(|byte| byte.is_ascii_alphanumeric()));
        first.start().await?;
        first
            .put("profile.mode", "compact")?
            .flash("notice", "next")?;
        assert_eq!(
            first.get_typed::<String>("profile.mode")?,
            Some("compact".into())
        );
        assert_eq!(first.remember("helpers.cached", || Ok(7))?, json!(7));
        first.push("helpers.list", "consumer")?;
        assert_eq!(first.increment("helpers.counter")?, json!(1));
        assert_eq!(first.decrement_by("helpers.counter", 2)?, json!(-1));
        first.set_name("consumer_session")?;
        assert_eq!(first.name()?, "consumer_session");
        first.save().await?;

        let second = Session::new(Some(id.clone()), store.clone(), expiry)?;
        second.start().await?;
        assert_eq!(second.get("helpers.list")?, Some(json!(["consumer"])));
        assert_eq!(second.remove("helpers.cached")?, Some(json!(7)));
        assert_eq!(
            second.get_typed::<String>("profile.mode")?,
            Some("compact".into())
        );
        assert_eq!(second.get("notice")?, Some(json!("next")));
        second.save().await?;

        store.delete(&id).await?;
        let error = second.save().await.err().ok_or_else(|| {
            openrust::Error::custom("session_consumer_expected_conflict", "应拒绝撤销后的更新")
        })?;
        assert_eq!(error.code(), "session_store_write_conflict");
        assert!(
            store
                .load(
                    &id,
                    openrust::session::DEFAULT_MAX_BYTES,
                    second.expiry_date()?
                )
                .await?
                .is_none()
        );
        Ok(())
    }
}

#[cfg(test)]
mod garbage_collection {
    use super::*;
    use openrust::session::{DEFAULT_MAX_BYTES, SessionId};
    use serde_json::json;
    use time::Duration;

    #[tokio::test]
    async fn external_wrapper_forwards_gc_and_retains_explicit_live_expiry() -> Result<()> {
        let store = ExternalSessionStore::default();
        let live = SessionRecord {
            id: SessionId::parse(&"L".repeat(40))?,
            data: json!({"business":"preserved"}),
            expiry_date: OffsetDateTime::now_utc() + Duration::hours(2),
        };
        store
            .write(&live, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
            .await?;
        let short = SessionRecord {
            id: SessionId::parse(&"E".repeat(40))?,
            data: json!({"business":"expired"}),
            expiry_date: OffsetDateTime::now_utc() + Duration::milliseconds(20),
        };
        store
            .write(&short, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(store.gc(1).await?, 1);
        assert_eq!(store.gc(1).await?, 0);
        let retained = store
            .load(&live.id, DEFAULT_MAX_BYTES, live.expiry_date)
            .await?
            .ok_or_else(|| openrust::Error::custom("consumer_gc_live_missing", "活会话丢失"))?;
        assert_eq!(retained.data, json!({"business":"preserved"}));
        assert_eq!(
            store
                .write(&short, SessionWriteMode::Update, DEFAULT_MAX_BYTES)
                .await?,
            SessionWriteOutcome::Missing
        );
        Ok(())
    }
}
