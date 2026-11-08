//! 外部包消费内置 Cookie driver，并在包装扩展中保留真实持久保证分类。
use openrust::http::{CookieFactory, CookieJar};
use openrust::session::{
    CookieSessionStore, Session, SessionId, SessionRecord, SessionRequestContext, SessionStore,
    SessionStoreGuarantees, SessionWriteMode, SessionWriteOutcome,
};
use openrust::{Result, support::BoxFuture};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;

struct Delegating {
    inner: Arc<dyn SessionStore>,
}
impl SessionStore for Delegating {
    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        self.inner.gc(lifetime_seconds)
    }

    fn supports_session_authentication(&self, cookie_name: &str) -> bool {
        self.inner.supports_session_authentication(cookie_name)
    }
    fn guarantees(&self) -> SessionStoreGuarantees {
        self.inner.guarantees()
    }
    fn needs_request(&self) -> bool {
        self.inner.needs_request()
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        Ok(self
            .inner
            .bind_request(context)?
            .map(|inner| Arc::new(Self { inner }) as Arc<dyn SessionStore>))
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.inner.load(id, max, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        self.inner.write(record, mode, max)
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.inner.delete(id)
    }
}
#[tokio::test]
async fn builtin_cookie_driver_is_consumable_and_wrappers_preserve_snapshot_guarantees()
-> Result<()> {
    assert!(CookieSessionStore::new(0, false).is_err());
    let driver = Arc::new(Delegating {
        inner: Arc::new(CookieSessionStore::new(2, true)?),
    });
    assert_eq!(
        driver.guarantees(),
        SessionStoreGuarantees::ClientSideSnapshot
    );
    assert!(!driver.supports_session_authentication("consumer_session"));
    assert_eq!(driver.gc(1).await?, 0);
    let queue = CookieJar::new(CookieFactory::default());
    let session = Session::new(None, driver, None)?;
    session
        .set_request_on_handler(SessionRequestContext::new(json!({}), queue.clone())?)
        .await?;
    assert_eq!(
        session.handler()?.guarantees(),
        SessionStoreGuarantees::ClientSideSnapshot
    );
    assert!(
        !session
            .handler()?
            .supports_session_authentication("consumer_session")
    );
    session.start().await?;
    session.put_typed("consumer.value", "owned")?.save().await?;
    let id = session.id()?;
    let data = queue
        .queued(id.as_str(), None)?
        .ok_or_else(|| openrust::Error::custom("cookie_consumer_missing", "缺少数据Cookie"))?;
    assert!(data.http_only() && data.expires().is_none());
    let mut replay = json!({});
    replay[id.as_str()] = json!(data.value());
    session.invalidate().await?;
    session.save().await?;
    let replayed = Session::new(Some(id), Arc::new(CookieSessionStore::new(2, true)?), None)?;
    replayed
        .set_request_on_handler(SessionRequestContext::new(
            replay,
            CookieJar::new(CookieFactory::default()),
        )?)
        .await?;
    replayed.start().await?;
    assert_eq!(replayed.get_json("consumer.value")?, Some(json!("owned")));
    Ok(())
}
