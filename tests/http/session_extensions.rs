//! 公开 Session 扩展的身份归属、转移与请求隔离；受控 driver 不替代真实服务证据。
#![cfg(feature = "session")]

use openrust::http::{CookieFactory, CookieJar, CookieOptions};
use openrust::session::{
    DEFAULT_MAX_BYTES, MemorySessionStore, Session, SessionId, SessionRecord,
    SessionRequestContext, SessionStore, SessionWriteMode, SessionWriteOutcome,
};
use openrust::{Error, Result, core::BoxFuture};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use time::OffsetDateTime;

#[derive(Default)]
struct Probe {
    memory: MemorySessionStore,
    writes: Mutex<Vec<SessionWriteMode>>,
    binds: AtomicUsize,
    deletes: AtomicUsize,
}
impl SessionStore for Probe {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        self.memory.load(id, max, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            self.writes.lock().map_err(|_| error())?.push(mode);
            self.memory.write(record, mode, max).await
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.memory.delete(id)
    }
    fn bind_request(&self, _: SessionRequestContext) -> Result<Option<Arc<dyn SessionStore>>> {
        self.binds.fetch_add(1, Ordering::SeqCst);
        Err(error())
    }
}
fn error() -> Error {
    Error::custom("session_extension_test_failed", "扩展测试受控失败")
}
fn context(cookies: Value) -> Result<SessionRequestContext> {
    SessionRequestContext::new(cookies, CookieJar::new(CookieFactory::default()))
}
fn session(store: Arc<dyn SessionStore>) -> Result<Session> {
    Session::new(None, store, None)
}
fn code<T>(result: Result<T>) -> Option<String> {
    result.err().map(|error| error.code().to_owned())
}
fn conflict<T>(result: Result<T>) {
    assert_eq!(
        code(result).as_deref(),
        Some("session_store_write_conflict")
    );
}

#[test]
fn set_id_retains_bag_and_started_and_invalid_inputs_generate_valid_ids() -> Result<()> {
    let session = session(Arc::new(Probe::default()))?;
    session.put_typed("owned", "unchanged")?;
    for value in [
        None,
        Some("short"),
        Some("éééééééééééééééééééé"),
        Some("000000000000000000000000000000000000000-"),
    ] {
        let previous = session.id()?;
        session.set_id(value)?;
        let id = session.id()?;
        assert_ne!(id, previous);
        assert_eq!(id.as_str().len(), 40);
        assert!(id.as_str().bytes().all(|byte| byte.is_ascii_alphanumeric()));
        assert_eq!(session.get_json("owned")?, Some(json!("unchanged")));
        assert!(!session.is_started()?);
    }
    let explicit = "A".repeat(40);
    session.set_id(Some(&explicit))?;
    assert_eq!(session.id()?, SessionId::parse(&explicit)?);
    assert_eq!(session.get_json("owned")?, Some(json!("unchanged")));
    Ok(())
}

#[tokio::test]
async fn same_binding_noop_and_switching_back_restore_update_proof() -> Result<()> {
    let origin = Arc::new(Probe::default());
    let erased: Arc<dyn SessionStore> = origin.clone();
    let session = session(erased.clone())?;
    session.start().await?;
    session.put_typed("owned", "source")?.save().await?;
    session.start().await?;
    let id = session.id()?;
    let other: Arc<dyn SessionStore> = Arc::new(Probe::default());
    assert!(Arc::ptr_eq(&other, &session.set_handler(other.clone())?));
    assert!(Arc::ptr_eq(&other, &session.handler()?));
    assert!(session.is_started()?);
    assert_eq!(session.get_json("owned")?, Some(json!("source")));
    assert_eq!(
        code(session.start().await).as_deref(),
        Some("session_rebind_pending")
    );
    session.set_id(Some(&"B".repeat(40)))?;
    assert!(session.is_started()?);
    session.set_handler(erased.clone())?;
    session.set_id(Some(id.as_str()))?;
    session.set_handler(erased.clone())?;
    session.set_id(Some(id.as_str()))?;
    session.save().await?;
    assert_eq!(
        *origin.writes.lock().map_err(|_| error())?,
        vec![SessionWriteMode::Insert, SessionWriteMode::Update]
    );
    origin.delete(&id).await?;
    conflict(session.save().await);
    conflict(session.set_id(None));
    conflict(session.set_handler(other));
    Ok(())
}

#[tokio::test]
async fn explicit_target_collision_preserves_id_and_target_and_allows_later_transfer() -> Result<()>
{
    let store = Arc::new(Probe::default());
    let source = session(store.clone())?;
    source.start().await?;
    source.put_typed("owner", "source")?.save().await?;
    let target_id = "T".repeat(40);
    let target = session(store.clone())?;
    target.set_id(Some(&target_id))?;
    target.start().await?;
    target.put_typed("owner", "target")?.save().await?;
    source.set_id(Some(&target_id))?;
    conflict(source.save().await);
    assert_eq!(source.id()?, SessionId::parse(&target_id)?);
    assert_eq!(source.get_json("owner")?, Some(json!("source")));
    let persisted = store
        .load(&target.id()?, DEFAULT_MAX_BYTES, target.expiry_date()?)
        .await?
        .ok_or_else(error)?;
    assert_eq!(persisted.data["owner"], "target");
    let free = "F".repeat(40);
    source.set_id(Some(&free))?;
    source.save().await?;
    source.put_typed("after", true)?.save().await?;
    let modes = store.writes.lock().map_err(|_| error())?;
    assert_eq!(
        &modes[modes.len() - 2..],
        &[SessionWriteMode::Insert, SessionWriteMode::Update]
    );
    Ok(())
}

#[tokio::test]
async fn revoked_source_cannot_be_copied_to_explicit_other_backend() -> Result<()> {
    let origin = Arc::new(Probe::default());
    let target = Arc::new(Probe::default());
    let session = session(origin.clone())?;
    session.start().await?;
    session.put_typed("authenticated", true)?.save().await?;
    origin.delete(&session.id()?).await?;
    session.set_handler(target.clone())?;
    session.set_id(Some(&"N".repeat(40)))?;
    conflict(session.save().await);
    assert!(target.writes.lock().map_err(|_| error())?.is_empty());
    conflict(session.set_id(None));
    assert_eq!(
        code(session.set_request_on_handler(context(json!({}))?).await).as_deref(),
        Some("session_store_write_conflict")
    );
    Ok(())
}

#[tokio::test]
async fn destroy_and_invalidate_tombstones_forbid_old_ids_even_on_another_store() -> Result<()> {
    for invalidate in [false, true] {
        let store = Arc::new(Probe::default());
        let session = session(store.clone())?;
        session.start().await?;
        session.put_typed("owned", "secret")?.save().await?;
        let old = session.id()?;
        if invalidate {
            session.invalidate().await?;
        } else {
            session.migrate(true).await?;
        }
        assert_ne!(session.id()?, old);
        conflict(session.set_id(Some(old.as_str())));
        session.set_handler(Arc::new(Probe::default()))?;
        conflict(session.set_id(Some(old.as_str())));
        assert_eq!(
            session.get_json("owned")?,
            if invalidate {
                None
            } else {
                Some(json!("secret"))
            }
        );
        if invalidate {
            session.start().await?;
        } // flush 后允许重建 token；非 flush pending 转移仍须先保存。
        session.save().await?;
        assert!(
            store
                .load(&old, DEFAULT_MAX_BYTES, session.expiry_date()?)
                .await?
                .is_none()
        );
    }
    Ok(())
}

struct Pending;
impl SessionStore for Pending {
    fn load<'a>(
        &'a self,
        _: &'a SessionId,
        _: usize,
        _: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(std::future::pending())
    }
    fn write<'a>(
        &'a self,
        _: &'a SessionRecord,
        _: SessionWriteMode,
        _: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(std::future::pending())
    }
    fn delete<'a>(&'a self, _: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test]
async fn pending_io_rejects_setters_and_cancellation_restores_shared_clone_mutability() -> Result<()>
{
    let session = session(Arc::new(Pending))?;
    let clone = session.clone();
    let before = session.id()?;
    let mut start = Box::pin(session.start());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), start.as_mut())
            .await
            .is_err()
    );
    assert_eq!(
        code(clone.set_id(None)).as_deref(),
        Some("session_operation_in_progress")
    );
    assert_eq!(
        code(clone.set_handler(Arc::new(Probe::default()))).as_deref(),
        Some("session_operation_in_progress")
    );
    assert_eq!(clone.id()?, before);
    let mut hook = Box::pin(clone.set_request_on_handler(context(json!({}))?));
    let mut task_context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(
        matches!(std::future::Future::poll(hook.as_mut(), &mut task_context), std::task::Poll::Ready(Err(error)) if error.code() == "session_operation_in_progress")
    );
    drop(hook);
    drop(start);
    let next = "C".repeat(40);
    clone.set_id(Some(&next))?;
    assert_eq!(session.id()?, SessionId::parse(&next)?);
    let store: Arc<dyn SessionStore> = Arc::new(Probe::default());
    clone.set_handler(store.clone())?;
    assert!(Arc::ptr_eq(&session.handler()?, &store));
    Ok(())
}

#[tokio::test]
async fn explicitly_reaffirming_random_id_disables_collision_retry() -> Result<()> {
    let store = Arc::new(Probe::default());
    let session = session(store.clone())?;
    let id = session.id()?;
    session.put_typed("mine", true)?;
    let occupant = SessionRecord {
        id: id.clone(),
        data: json!({"occupant":true}),
        expiry_date: session.expiry_date()?,
    };
    store
        .write(&occupant, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
        .await?;
    session.set_id(Some(id.as_str()))?;
    conflict(session.save().await);
    assert_eq!(session.id()?, id);
    assert_eq!(session.get_json("mine")?, Some(json!(true)));
    assert!(session.set_id(None).is_ok()); // 显式碰撞不永久撤销。
    Ok(())
}

#[tokio::test]
async fn pending_transfer_cannot_destroy_either_binding_or_change_attributes() -> Result<()> {
    for change_store in [false, true] {
        for action in 0..3 {
            let origin = Arc::new(Probe::default());
            let target = Arc::new(Probe::default());
            let erased: Arc<dyn SessionStore> = origin.clone();
            let session = session(erased.clone())?;
            session.start().await?;
            session.put_typed("owned", "kept")?.save().await?;
            let old = session.id()?;
            if change_store {
                session.set_handler(target.clone())?;
            } else {
                session.set_id(Some(&"D".repeat(40)))?;
            }
            let selected = session.id()?;
            let before = session.all_json()?;
            let result = match action {
                0 => session.invalidate().await,
                1 => session.migrate(true).await,
                _ => session.regenerate(true).await,
            };
            assert_eq!(code(result).as_deref(), Some("session_rebind_pending"));
            assert_eq!(session.id()?, selected);
            assert_eq!(session.all_json()?, before);
            assert_eq!(origin.deletes.load(Ordering::SeqCst), 0);
            assert_eq!(target.deletes.load(Ordering::SeqCst), 0);
            assert!(
                origin
                    .load(&old, DEFAULT_MAX_BYTES, session.expiry_date()?)
                    .await?
                    .is_some()
            );
            assert!(target.writes.lock().map_err(|_| error())?.is_empty());
            session.set_handler(erased)?;
            session.set_id(Some(old.as_str()))?;
            session.migrate(true).await?;
            assert!(
                origin
                    .load(&old, DEFAULT_MAX_BYTES, session.expiry_date()?)
                    .await?
                    .is_none()
            );
        }
    }
    Ok(())
}

struct RequestDriver {
    backend: Arc<Probe>,
    binds: AtomicUsize,
    returns_instance: bool,
}
struct BoundDriver {
    backend: Arc<Probe>,
    context: SessionRequestContext,
}
impl SessionStore for RequestDriver {
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        self.binds.fetch_add(1, Ordering::SeqCst);
        Ok(self.returns_instance.then(|| {
            Arc::new(BoundDriver {
                backend: self.backend.clone(),
                context,
            }) as Arc<dyn SessionStore>
        }))
    }
    fn load<'a>(
        &'a self,
        _: &'a SessionId,
        _: usize,
        _: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async { Err(error()) })
    }
    fn write<'a>(
        &'a self,
        _: &'a SessionRecord,
        _: SessionWriteMode,
        _: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async { Err(error()) })
    }
    fn delete<'a>(&'a self, _: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(error()) })
    }
}
impl SessionStore for BoundDriver {
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
            let value = self
                .context
                .cookie("scope")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or_else(error)?;
            let jar = self.context.cookie_jar();
            jar.queue(
                jar.factory()
                    .make("bound_probe", value, 0, CookieOptions::default())?,
            )?;
            self.backend.write(record, mode, max).await
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.backend.delete(id)
    }
}

#[tokio::test]
async fn request_hooks_require_real_instances_preserve_driver_identity_and_isolate_queues()
-> Result<()> {
    let ordinary = Arc::new(Probe::default());
    let plain = session(ordinary.clone())?;
    assert!(!plain.handler_needs_request()?);
    plain.set_request_on_handler(context(json!({}))?).await?;
    assert_eq!(ordinary.binds.load(Ordering::SeqCst), 0);
    let invalid = session(Arc::new(RequestDriver {
        backend: ordinary.clone(),
        binds: AtomicUsize::new(0),
        returns_instance: false,
    }))?;
    assert_eq!(
        code(invalid.start().await).as_deref(),
        Some("session_request_unbound")
    );
    assert_eq!(
        code(invalid.set_request_on_handler(context(json!({}))?).await).as_deref(),
        Some("session_request_binding_invalid")
    );
    assert_eq!(
        code(invalid.save().await).as_deref(),
        Some("session_request_unbound")
    );
    let driver = Arc::new(RequestDriver {
        backend: Arc::new(Probe::default()),
        binds: AtomicUsize::new(0),
        returns_instance: true,
    });
    let identity: Arc<dyn SessionStore> = driver.clone();
    let first = session(identity.clone())?;
    let second = session(identity.clone())?;
    assert!(first.handler_needs_request()?);
    let first_jar = CookieJar::new(CookieFactory::default());
    let second_jar = CookieJar::new(CookieFactory::default());
    first
        .set_request_on_handler(SessionRequestContext::new(
            json!({"scope":"one"}),
            first_jar.clone(),
        )?)
        .await?;
    second
        .set_request_on_handler(SessionRequestContext::new(
            json!({"scope":"two"}),
            second_jar.clone(),
        )?)
        .await?;
    assert!(!Arc::ptr_eq(&first.handler()?, &second.handler()?));
    assert!(Arc::ptr_eq(&first.handler()?, &first.clone().handler()?));
    first.start().await?;
    second.start().await?;
    first.put_typed("owner", "one")?.save().await?;
    second.put_typed("owner", "two")?.save().await?;
    assert_eq!(
        first_jar
            .queued("bound_probe", None)?
            .and_then(|cookie| cookie.value().map(str::to_owned)),
        Some("one".into())
    );
    assert_eq!(
        second_jar
            .queued("bound_probe", None)?
            .and_then(|cookie| cookie.value().map(str::to_owned)),
        Some("two".into())
    );
    first.set_handler(identity)?; // 同逻辑驱动不得丢失本请求实例或原 Update 归属。
    first.save().await?;
    assert_eq!(driver.binds.load(Ordering::SeqCst), 2);
    assert_eq!(
        driver.backend.writes.lock().map_err(|_| error())?.last(),
        Some(&SessionWriteMode::Update)
    );
    Ok(())
}

#[test]
fn request_context_has_exact_cookie_names_bounded_ownership_and_redacted_debug() -> Result<()> {
    let jar = CookieJar::new(CookieFactory::default());
    let context = SessionRequestContext::new(
        json!({"secret.cookie":"credential-private", "secret":{"cookie":"different"}}),
        jar.clone(),
    )?;
    assert_eq!(
        context.cookie("secret.cookie"),
        Some(json!("credential-private"))
    );
    assert_eq!(context.cookie("missing"), None);
    let indexed = SessionRequestContext::new(json!(["v"]), jar.clone())?;
    assert_eq!(indexed.cookie("0"), Some(json!("v")));
    assert_eq!(indexed.cookie("00"), None);
    let mut copy = context.cookie("secret.cookie").ok_or_else(error)?;
    if let Value::String(value) = &mut copy {
        value.clear();
    }
    assert_eq!(copy, json!(""));
    assert_eq!(
        context.cookie("secret.cookie"),
        Some(json!("credential-private"))
    );
    assert!(!format!("{context:?}").contains("credential-private"));
    assert!(!format!("{context:?}").contains("secret.cookie"));
    context.cookie_jar().queue(jar.factory().make(
        "shared",
        "visible",
        0,
        CookieOptions::default(),
    )?)?;
    assert!(jar.has_queued("shared", None)?);
    assert!(SessionRequestContext::new(json!("scalar"), jar.clone()).is_err());
    assert_eq!(
        code(SessionRequestContext::new(
            json!({"oversized":"x".repeat(64*1024)}),
            jar.clone()
        ))
        .as_deref(),
        Some("session_limits_exceeded")
    );
    let mut deep = json!(true);
    for _ in 0..256 {
        deep = json!({"nested":deep});
    }
    assert!(SessionRequestContext::new(deep, jar).is_err());
    Ok(())
}

struct CaptureSession(Arc<Mutex<Option<Session>>>);
impl openrust::core::ServiceProvider for CaptureSession {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(self.0.clone()).await?;
            Ok(())
        })
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| routes.get("/extension/close", capture_session))?;
            Ok(())
        })
    }
}
async fn capture_session(
    session: Session,
    openrust::http::Inject(capture): openrust::http::Inject<Mutex<Option<Session>>>,
) -> Result<openrust::http::Json<Value>> {
    *capture.lock().map_err(|_| error())? = Some(session);
    Ok(openrust::http::Json(json!({"done":true})))
}
#[tokio::test]
async fn completed_response_closes_setters_and_request_binding_on_retained_clone() -> Result<()> {
    use openrust::ServiceExt;
    let capture = Arc::new(Mutex::new(None));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(openrust::http::HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(openrust::session::SessionServiceProvider::with_store(
            Probe::default(),
        ))
        .provider(CaptureSession(capture.clone()));
    let app = openrust::App::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(
                    openrust::config::FrameworkConfig::default(),
                ),
            ))
            .await?,
    )?;
    let outcome: Result<()> = async {
        let response = openrust::http::build_router(&app)?
            .oneshot(
                axum::http::Request::get("/extension/close")
                    .header("host", "localhost")
                    .body(axum::body::Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let session = capture
            .lock()
            .map_err(|_| error())?
            .take()
            .ok_or_else(error)?;
        assert_eq!(
            code(session.set_id(None)).as_deref(),
            Some("session_closed")
        );
        assert_eq!(
            code(session.set_handler(Arc::new(Probe::default()))).as_deref(),
            Some("session_closed")
        );
        assert_eq!(
            code(session.set_request_on_handler(context(json!({}))?).await).as_deref(),
            Some("session_closed")
        );
        assert_eq!(
            code(session.set_previous_url("/late")).as_deref(),
            Some("session_closed")
        );
        assert_eq!(
            code(session.set_previous_route(None)).as_deref(),
            Some("session_closed")
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    outcome
}

struct FailingRequestDriver {
    fail_bind: bool,
    context: Option<SessionRequestContext>,
}
impl SessionStore for FailingRequestDriver {
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        let jar = context.cookie_jar();
        jar.queue(
            jar.factory()
                .make("must_not_leave", "private", 0, CookieOptions::default())?,
        )?;
        if self.fail_bind {
            return Err(Error::custom_with_status(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "extension_bind_failed",
                "受控绑定失败",
            ));
        }
        Ok(Some(Arc::new(Self {
            fail_bind: false,
            context: Some(context),
        })))
    }
    fn load<'a>(
        &'a self,
        _: &'a SessionId,
        _: usize,
        _: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async {
            assert!(self.context.is_some());
            Err(Error::custom_with_status(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "extension_start_failed",
                "受控加载失败",
            ))
        })
    }
    fn write<'a>(
        &'a self,
        _: &'a SessionRecord,
        _: SessionWriteMode,
        _: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async { Err(error()) })
    }
    fn delete<'a>(&'a self, _: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn binding_or_start_failure_discards_queued_cookies_and_preserves_original_error()
-> Result<()> {
    use openrust::ServiceExt;
    for fail_bind in [true, false] {
        let mut config = openrust::config::FrameworkConfig::default();
        config.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(openrust::http::HttpServiceProvider)
            .provider(openrust::http::CookieServiceProvider::plain())
            .provider(openrust::session::SessionServiceProvider::with_store(
                FailingRequestDriver {
                    fail_bind,
                    context: None,
                },
            ));
        let app = openrust::App::from_provider_runtime(
            builder
                .start(figment::Figment::from(
                    figment::providers::Serialized::defaults(config),
                ))
                .await?,
        )?;
        let outcome: Result<()> = async {
            let response = openrust::http::build_router(&app)?
                .oneshot(
                    axum::http::Request::get("/failure")
                        .header("host", "localhost")
                        .body(axum::body::Body::empty())
                        .map_err(Error::internal)?,
                )
                .await
                .map_err(Error::internal)?;
            assert_eq!(
                response.status(),
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert!(
                response
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .next()
                    .is_none()
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .map_err(Error::internal)?;
            let body: Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
            assert_eq!(body["success"], false);
            assert_eq!(
                body["code"],
                if fail_bind {
                    "extension_bind_failed"
                } else {
                    "extension_start_failed"
                }
            );
            assert_eq!(body["message"], "internal server error");
            Ok(())
        }
        .await;
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        outcome?;
    }
    Ok(())
}

#[derive(Default)]
struct GcTrace {
    events: Mutex<Vec<String>>,
    jar: Mutex<Option<CookieJar>>,
    entered: tokio::sync::Notify,
    cancelled: AtomicUsize,
}
struct GcDriver {
    trace: Arc<GcTrace>,
    backend: Arc<MemorySessionStore>,
    context: Option<SessionRequestContext>,
    mode: u8,
}
impl SessionStore for GcDriver {
    fn needs_request(&self) -> bool {
        true
    }
    fn bind_request(
        &self,
        context: SessionRequestContext,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        *self.trace.jar.lock().map_err(|_| error())? = Some(context.cookie_jar().clone());
        Ok(Some(Arc::new(Self {
            trace: self.trace.clone(),
            backend: self.backend.clone(),
            context: Some(context),
            mode: self.mode,
        })))
    }
    fn gc(&self, lifetime: i64) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move {
            let context = self.context.as_ref().ok_or_else(error)?;
            assert_eq!(context.cookie("gc_scope"), Some(json!("request")));
            self.trace
                .events
                .lock()
                .map_err(|_| error())?
                .push(format!("gc:{lifetime}"));
            let jar = context.cookie_jar();
            jar.queue(
                jar.factory()
                    .make("gc_receipt", "private", 0, CookieOptions::default())?,
            )?;
            if self.mode == 1 {
                return Err(Error::custom_with_status(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "session_gc_probe_failed",
                    "不得泄露",
                ));
            }
            if self.mode == 2 {
                struct Cancelled<'a>(&'a AtomicUsize);
                impl Drop for Cancelled<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_add(1, Ordering::SeqCst);
                    }
                }
                let _cancelled = Cancelled(&self.trace.cancelled);
                self.trace.entered.notify_one();
                std::future::pending::<()>().await;
            }
            self.backend.gc(lifetime).await
        })
    }
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        max: usize,
        expiry: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            self.trace
                .events
                .lock()
                .map_err(|_| error())?
                .push("load".into());
            self.backend.load(id, max, expiry).await
        })
    }
    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        max: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            self.trace
                .events
                .lock()
                .map_err(|_| error())?
                .push("write".into());
            self.backend.write(record, mode, max).await
        })
    }
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        self.backend.delete(id)
    }
}
struct GcRoutes(Arc<GcTrace>);
impl openrust::core::ServiceProvider for GcRoutes {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(self.0.clone()).await?;
            Ok(())
        })
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| routes.get("/gc", gc_business))?;
            Ok(())
        })
    }
}
async fn gc_business(
    session: Session,
    openrust::http::Inject(trace): openrust::http::Inject<GcTrace>,
) -> Result<openrust::http::Json<Value>> {
    assert!(session.is_started()?);
    trace
        .events
        .lock()
        .map_err(|_| error())?
        .push("business".into());
    session.put_typed("business", "nonempty")?;
    Ok(openrust::http::Json(
        json!({"value":session.get_json("business")?}),
    ))
}
async fn gc_app(lottery: [u32; 2], mode: u8) -> Result<(openrust::App, Arc<GcTrace>)> {
    let trace = Arc::new(GcTrace::default());
    let mut config = openrust::config::FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(openrust::http::HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(
            openrust::session::SessionServiceProvider::with_store(GcDriver {
                trace: trace.clone(),
                backend: Arc::new(MemorySessionStore::default()),
                context: None,
                mode,
            })
            .with_cookie_config(openrust::config::SessionConfig {
                lottery,
                lifetime: 7,
                secure: Some(false),
                ..Default::default()
            }),
        )
        .provider(GcRoutes(trace.clone()));
    let runtime = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await?;
    Ok((openrust::App::from_provider_runtime(runtime)?, trace))
}
fn gc_request() -> Result<axum::http::Request<axum::body::Body>> {
    axum::http::Request::get("/gc")
        .header("host", "localhost")
        .header("cookie", "gc_scope=request")
        .body(axum::body::Body::empty())
        .map_err(Error::internal)
}
#[tokio::test]
async fn gc_lottery_zero_and_full_use_bound_driver_after_start_before_business() -> Result<()> {
    use openrust::ServiceExt;
    assert_eq!(Probe::default().gc(420).await?, 0);
    for (lottery, expected) in [
        ([0, 1], vec!["load", "business", "write"]),
        ([1, 1], vec!["load", "gc:420", "business", "write"]),
    ] {
        let (app, trace) = gc_app(lottery, 0).await?;
        let outcome: Result<()> = async {
            let response = openrust::http::build_router(&app)?
                .oneshot(gc_request()?)
                .await
                .map_err(Error::internal)?;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .map_err(Error::internal)?;
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).map_err(Error::internal)?["data"]["value"],
                "nonempty"
            );
            assert_eq!(*trace.events.lock().map_err(|_| error())?, expected);
            Ok(())
        }
        .await;
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        outcome?;
    }
    Ok(())
}
#[tokio::test]
async fn gc_error_discards_queued_cookie_and_never_enters_business_or_save() -> Result<()> {
    use openrust::ServiceExt;
    let (app, trace) = gc_app([1, 1], 1).await?;
    let outcome: Result<()> = async {
        let response = openrust::http::build_router(&app)?
            .oneshot(gc_request()?)
            .await
            .map_err(Error::internal)?;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(response.headers().get("set-cookie").is_none());
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        assert_eq!(
            response
                .headers()
                .get("pragma")
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .map_err(Error::internal)?;
        let body: Value = serde_json::from_slice(&bytes).map_err(Error::internal)?;
        assert_eq!(body["code"], "session_gc_probe_failed");
        assert_eq!(body["message"], "internal server error");
        assert_eq!(
            *trace.events.lock().map_err(|_| error())?,
            vec!["load", "gc:420"]
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    outcome
}
#[tokio::test]
async fn cancelled_gc_drops_request_future_without_business_save_or_background_work() -> Result<()>
{
    use openrust::ServiceExt;
    let (app, trace) = gc_app([1, 1], 2).await?;
    let outcome: Result<()> = async {
        let mut request = Box::pin(openrust::http::build_router(&app)?.oneshot(gc_request()?));
        tokio::select! { ()=trace.entered.notified()=>{}, _=&mut request=>return Err(error()), ()=tokio::time::sleep(std::time::Duration::from_secs(5))=>return Err(error()) }
        drop(request);
        assert_eq!(trace.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(
            *trace.events.lock().map_err(|_| error())?,
            vec!["load", "gc:420"]
        );
        let queue = trace
            .jar
            .lock()
            .map_err(|_| error())?
            .clone()
            .ok_or_else(error)?;
        assert!(queue.queued("gc_receipt", None)?.is_none());
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    outcome
}
