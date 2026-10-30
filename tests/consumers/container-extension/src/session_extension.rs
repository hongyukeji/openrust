//! 第三方 SessionStore 沿原 Provider factory 的公开装饰与受管理 HTTP 回归。

use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request as Wire, StatusCode},
};
use figment::{Figment, providers::Serialized};
use openrust::{
    App, Error, Result, ServiceExt,
    config::{FrameworkConfig, SessionConfig},
    core::{BootContext, BoxFuture, ProviderBuilder, ProviderId, RegisterContext, ServiceProvider},
    http::{CookieServiceProvider, HttpServiceProvider, Json, build_router},
    session::{
        MemorySessionStore, Session, SessionId, SessionRecord, SessionRequestContext,
        SessionServiceProvider, SessionStore, SessionStoreGuarantees, SessionStoreHandle,
        SessionWriteMode, SessionWriteOutcome,
    },
};
use serde_json::{Value, json};
use time::OffsetDateTime;

#[derive(Default)]
struct Probe {
    created: AtomicUsize,
    original_loads: AtomicUsize,
    original_writes: AtomicUsize,
    dropped: AtomicUsize,
    decorated: AtomicUsize,
    wrapped_loads: AtomicUsize,
    wrapped_writes: AtomicUsize,
    original: Mutex<Option<Weak<dyn SessionStore>>>,
}

struct OriginalStore {
    storage: MemorySessionStore,
    probe: Arc<Probe>,
}

impl OriginalStore {
    fn new(probe: Arc<Probe>) -> Self {
        probe.created.fetch_add(1, Ordering::SeqCst);
        Self {
            storage: MemorySessionStore::default(),
            probe,
        }
    }
}

impl Drop for OriginalStore {
    fn drop(&mut self) {
        self.probe.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl SessionStore for OriginalStore {
    fn guarantees(&self) -> SessionStoreGuarantees {
        self.storage.guarantees()
    }

    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        self.storage.gc(lifetime_seconds)
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.probe.original_loads.fetch_add(1, Ordering::SeqCst);
        self.storage.load(id, max_bytes, empty_expiry)
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        self.probe.original_writes.fetch_add(1, Ordering::SeqCst);
        self.storage.write(record, mode, max_bytes)
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.storage.delete(id)
    }
}

struct ObservedStore {
    inner: Arc<dyn SessionStore>,
    probe: Arc<Probe>,
}

impl SessionStore for ObservedStore {
    fn guarantees(&self) -> SessionStoreGuarantees {
        self.inner.guarantees()
    }

    fn supports_session_authentication(&self, cookie_name: &str) -> bool {
        self.inner.supports_session_authentication(cookie_name)
    }

    fn needs_request(&self) -> bool {
        self.inner.needs_request()
    }

    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        self.inner.bind_request(context).map(|bound| {
            bound.map(|inner| {
                Arc::new(Self {
                    inner,
                    probe: Arc::clone(&self.probe),
                }) as Arc<dyn SessionStore>
            })
        })
    }

    fn gc(&self, lifetime_seconds: i64) -> BoxFuture<'_, Result<u64>> {
        self.inner.gc(lifetime_seconds)
    }

    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max_bytes: usize,
        empty_expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.probe.wrapped_loads.fetch_add(1, Ordering::SeqCst);
        self.inner.load(id, max_bytes, empty_expiry)
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        self.probe.wrapped_writes.fetch_add(1, Ordering::SeqCst);
        self.inner.write(record, mode, max_bytes)
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.inner.delete(id)
    }
}

struct ExternalSessionExtension(Arc<Probe>);

impl ServiceProvider for ExternalSessionExtension {
    fn dependencies(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<SessionServiceProvider>()]
    }

    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        let probe = Arc::clone(&self.0);
        app.extend::<SessionStoreHandle, _>(move |old, _| {
            let probe = Arc::clone(&probe);
            Box::pin(async move {
                let original = old.store();
                *probe.original.lock().map_err(|_| failure())? = Some(Arc::downgrade(&original));
                probe.decorated.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(SessionStoreHandle::new(ObservedStore {
                    inner: original,
                    probe,
                })))
            })
        });
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/session-extension/write", write)
                    .get("/session-extension/read", read)
            })?;
            Ok(())
        })
    }
}

async fn write(session: Session) -> Result<Json<Value>> {
    session.put("extension.value", "persisted")?;
    Ok(Json(json!({ "written": true })))
}

async fn read(session: Session) -> Result<Json<Value>> {
    Ok(Json(json!({ "value": session.get("extension.value")? })))
}

fn failure() -> Error {
    Error::custom(
        "session_extension_test_failed",
        "Session 扩展调用链断言失败",
    )
}

fn ensure(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(failure()) }
}

async fn request(
    router: axum::Router,
    path: &str,
    cookie: Option<&str>,
) -> Result<(HeaderMap, Value)> {
    let mut wire = Wire::get(path).header("host", "localhost");
    if let Some(cookie) = cookie {
        wire = wire.header("cookie", cookie);
    }
    let response = tokio::time::timeout(Duration::from_secs(5), async {
        router
            .oneshot(wire.body(Body::empty()).map_err(Error::internal)?)
            .await
            .map_err(Error::internal)
    })
    .await
    .map_err(|_| failure())??;
    ensure(response.status() == StatusCode::OK)?;
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 8192)
        .await
        .map_err(Error::internal)?;
    Ok((
        headers,
        serde_json::from_slice(&bytes).map_err(Error::internal)?,
    ))
}

#[tokio::test]
async fn original_custom_session_store_is_extended_through_managed_cookie_roundtrip() -> Result<()>
{
    let probe = Arc::new(Probe::default());
    let mut providers = ProviderBuilder::new();
    providers
        .provider(HttpServiceProvider)
        .provider(CookieServiceProvider::plain())
        .provider(
            SessionServiceProvider::with_store(OriginalStore::new(Arc::clone(&probe)))
                .with_cookie_config(SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some("extended_session".into()),
                    secure: Some(false),
                    ..SessionConfig::default()
                }),
        )
        .provider(ExternalSessionExtension(Arc::clone(&probe)));
    let runtime = providers
        .start(Figment::from(Serialized::defaults(
            FrameworkConfig::default(),
        )))
        .await?;
    let app = App::from_provider_runtime(runtime)?;
    let result = async {
        let router = build_router(&app)?;
        let (headers, first) = request(router.clone(), "/session-extension/write", None).await?;
        ensure(first["data"] == json!({ "written": true }))?;
        let cookies = headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .filter(|value| value.starts_with("extended_session="))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        ensure(cookies.len() == 1)?;
        let (_, second) = request(router, "/session-extension/read", Some(&cookies[0])).await?;
        ensure(second["data"] == json!({ "value": "persisted" }))?;
        ensure(probe.created.load(Ordering::SeqCst) == 1)?;
        ensure(probe.decorated.load(Ordering::SeqCst) == 1)?;
        let original = probe
            .original
            .lock()
            .map_err(|_| failure())?
            .as_ref()
            .and_then(Weak::upgrade);
        ensure(original.is_some())?;
        drop(original);
        let loads = probe.original_loads.load(Ordering::SeqCst);
        let writes = probe.original_writes.load(Ordering::SeqCst);
        ensure(loads >= 1 && writes >= 2)?;
        ensure(probe.wrapped_loads.load(Ordering::SeqCst) == loads)?;
        ensure(probe.wrapped_writes.load(Ordering::SeqCst) == writes)
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    drop(app);
    ensure(probe.dropped.load(Ordering::SeqCst) == 1)?;
    ensure(
        probe
            .original
            .lock()
            .map_err(|_| failure())?
            .as_ref()
            .is_some_and(|original| original.upgrade().is_none()),
    )?;
    result
}
