//! 外部 crate 实现请求绑定 hook；只用公开构造与同一外部持久 driver。
use openrust::http::{CookieFactory, CookieJar, CookieOptions};
use openrust::session::{
    Session, SessionId, SessionRecord, SessionRequestContext, SessionStore, SessionWriteMode,
    SessionWriteOutcome,
};
use openrust::{Result, support::BoxFuture};
use serde_json::json;
use session_custom_driver_consumer::ExternalSessionStore;
use std::sync::Arc;
use time::OffsetDateTime;

struct ExternalRequestDriver {
    backend: Arc<ExternalSessionStore>,
    context: Option<SessionRequestContext>,
}
impl SessionStore for ExternalRequestDriver {
    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        self.backend.gc(lifetime_seconds)
    }
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        Ok(Some(Arc::new(Self {
            backend: self.backend.clone(),
            context: Some(context),
        })))
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.backend.load(id, max, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let context = self
                .context
                .as_ref()
                .ok_or_else(|| openrust::Error::custom("consumer_unbound", "外部驱动未绑定请求"))?;
            let label = context
                .cookie("consumer.scope")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(|| {
                    openrust::Error::custom("consumer_scope_missing", "缺少消费端请求标识")
                })?;
            let jar = context.cookie_jar();
            jar.queue(
                jar.factory()
                    .make("consumer_reply", label, 0, CookieOptions::default())?,
            )?;
            self.backend.write(record, mode, max).await
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.backend.delete(id)
    }
}

#[tokio::test]
async fn external_hook_instances_are_request_local_and_explicit_transfer_stays_public() -> Result<()>
{
    let backend = Arc::new(ExternalSessionStore::default());
    let driver: Arc<dyn SessionStore> = Arc::new(ExternalRequestDriver {
        backend: backend.clone(),
        context: None,
    });
    let first = Session::new(None, driver.clone(), None)?;
    let second = Session::new(None, driver.clone(), None)?;
    assert!(first.handler_needs_request()?);
    assert_eq!(
        first
            .start()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("session_request_unbound".into())
    );
    let first_jar = CookieJar::new(CookieFactory::default());
    let second_jar = CookieJar::new(CookieFactory::default());
    first
        .set_request_on_handler(SessionRequestContext::new(
            json!({"consumer.scope":"first"}),
            first_jar.clone(),
        )?)
        .await?;
    second
        .set_request_on_handler(SessionRequestContext::new(
            json!({"consumer.scope":"second"}),
            second_jar.clone(),
        )?)
        .await?;
    assert!(!Arc::ptr_eq(&first.handler()?, &second.handler()?));
    first.start().await?;
    second.start().await?;
    first.put_typed("consumer.value", "owned")?.save().await?;
    second.put_typed("consumer.value", "other")?.save().await?;
    assert_eq!(
        first_jar
            .queued("consumer_reply", None)?
            .and_then(|cookie| cookie.value().map(str::to_owned)),
        Some("first".into())
    );
    assert_eq!(
        second_jar
            .queued("consumer_reply", None)?
            .and_then(|cookie| cookie.value().map(str::to_owned)),
        Some("second".into())
    );
    let first_id = first.id()?;
    let transferred: Arc<dyn SessionStore> = Arc::new(ExternalSessionStore::default());
    assert!(Arc::ptr_eq(
        &first.set_handler(transferred.clone())?,
        &transferred
    ));
    first.set_id(Some(&"E".repeat(40)))?;
    first.save().await?;
    let restored = Session::new(Some(first.id()?), transferred, None)?;
    restored.start().await?;
    assert_eq!(restored.get_json("consumer.value")?, Some(json!("owned")));
    assert!(
        backend
            .load(
                &first_id,
                openrust::session::DEFAULT_MAX_BYTES,
                first.expiry_date()?
            )
            .await?
            .is_some()
    );
    Ok(())
}
