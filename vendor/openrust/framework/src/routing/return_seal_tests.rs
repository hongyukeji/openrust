//! 真实 Store I/O 与 Session 下游闸的分离见证。
use super::{HandlerFn, Invocation, ManagedNext};
use crate::foundation::{
    Application, ApplicationState, BootContext, ProviderBuilder, http::build_router,
};
use crate::http::HttpServiceProvider;
use crate::session::{
    ErrorBagCodecs, MemorySessionStore, SessionId, SessionLayer, SessionRecord,
    SessionServiceProvider, SessionSettings, SessionStore, SessionWriteMode, SessionWriteOutcome,
};
use crate::support::{BoxFuture, ServiceProvider};
use crate::{Error, Result};
use axum::{
    body::Body,
    extract::Request,
    response::{IntoResponse, Response},
};
use http::{StatusCode, header::SET_COOKIE};
use std::convert::Infallible;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::sync::Notify;
use tower::{Service, ServiceExt};

#[derive(Default)]
pub(in crate::routing) struct Writes {
    pub(in crate::routing) count: AtomicUsize,
    entered: Notify,
    release: Notify,
}

pub(in crate::routing) struct ObservedStore {
    data: MemorySessionStore,
    writes: Arc<Writes>,
    pause: bool,
}

impl ObservedStore {
    pub(in crate::routing) fn new(writes: Arc<Writes>, pause: bool) -> Self {
        Self {
            data: MemorySessionStore::default(),
            writes,
            pause,
        }
    }
}

impl SessionStore for ObservedStore {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        limit: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.data.load(id, limit, expiry)
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        limit: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            self.writes.count.fetch_add(1, Ordering::SeqCst);
            if self.pause {
                self.writes.entered.notify_one();
                self.writes.release.notified().await;
            }
            self.data.write(record, mode, limit).await
        })
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.data.delete(id)
    }
}

#[derive(Clone)]
struct SealRoute {
    writes: Arc<Writes>,
    calls: Arc<AtomicUsize>,
}

impl ServiceProvider for SealRoute {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let fixture = self.clone();
            app.routes(move |routes| {
                let fixture = fixture.clone();
                routes.get("/seal-store-boundary", move |request: Request| {
                    let fixture = fixture.clone();
                    async move { exercise(request, fixture).await }
                })
            })?;
            Ok(())
        })
    }
}

async fn exercise(request: Request, fixture: SealRoute) -> Result<Response> {
    let state = request
        .extensions()
        .get::<ApplicationState>()
        .cloned()
        .ok_or_else(|| Error::custom("fixture_missing_app_state", "缺少实际入口状态"))?;
    let (root, root_retire) = Invocation::new(None);
    assert!(root.owns_scope(&request));
    let calls = Arc::clone(&fixture.calls);
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = Mutex::new(Some(send));
    let handler: HandlerFn = Arc::new(move |request, _, invocation| {
        assert!(invocation.owns_scope(&request));
        calls.fetch_add(1, Ordering::SeqCst);
        if let Some(sender) = send
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            assert!(sender.send(request).is_ok());
        }
        Box::pin(async { StatusCode::ACCEPTED.into_response() })
    });
    let (next, session_retire) = ManagedNext::new(handler, state, &root);
    let late_next = next.clone();
    let mut late_service = tower::service_fn(move |request| {
        let response = late_next.run(request);
        async move { Ok::<_, Infallible>(response.await) }
    });
    let session = SessionLayer {
        settings: SessionSettings {
            config: crate::config::SessionConfig {
                driver: Some("array".into()),
                lottery: [0, 1],
                ..Default::default()
            },
            cookie_name: "seal_session".into(),
        },
        store: Arc::new(ObservedStore::new(Arc::clone(&fixture.writes), true)),
        error_bag_codecs: Arc::new(ErrorBagCodecs::default()),
    };
    let late = async {
        let request = receive.await.map_err(Error::internal)?;
        fixture.writes.entered.notified().await;
        // 已有一次同闸准入。实际 Store 暂停，Session 对应 Retire 仍由本函数保活。
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        let response = late_service.call(request).await;
        fixture.writes.release.notify_one();
        response.map_err(Error::internal)
    };
    let (response, late) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(crate::session::handle_session(session, request, next), late)
    })
    .await
    .map_err(Error::internal)?;
    assert_eq!(late?.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    drop(session_retire);
    drop(root_retire);
    Ok(response)
}

#[tokio::test]
async fn session_store_write_is_already_sealed_with_its_own_retire_still_live() -> Result<()> {
    let writes = Arc::new(Writes::default());
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(SessionServiceProvider::with_store(
            MemorySessionStore::default(),
        ))
        .provider(SealRoute {
            writes: Arc::clone(&writes),
            calls: Arc::new(AtomicUsize::new(0)),
        });
    let config = figment::Figment::from(figment::providers::Serialized::defaults(
        crate::config::FrameworkConfig::default(),
    ));
    let app = Application::from_provider_runtime(builder.start(config).await?)?;
    let mut request = Request::new(Body::empty());
    *request.uri_mut() = http::Uri::from_static("/seal-store-boundary");
    request
        .headers_mut()
        .insert("host", http::HeaderValue::from_static("localhost"));
    let response = build_router(&app)?
        .oneshot(request)
        .await
        .map_err(Error::internal)?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(writes.count.load(Ordering::SeqCst), 1);
    assert!(response.headers().contains_key(SET_COOKIE));
    Ok(())
}
