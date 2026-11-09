use super::*;
use crate::session::SessionStore;
use serde_json::json;

fn receipt(identifier: Value) -> Result<FactorReceipt> {
    FactorReceipt::issue(
        identifier,
        "web".into(),
        None,
        "db/main/users".into(),
        "generation-1".into(),
        super::super::proof::FactorVerificationMethod::Totp,
        123,
    )
}
#[tokio::test]
async fn factor_proof_is_committed_with_login_and_cleared_by_ordinary_identity_changes()
-> Result<()> {
    let guard = regeneration_guard(
        Arc::new(RegenerationStore::default()),
        Arc::new(EmptyProvider(false)),
        None,
    )
    .await?;
    guard
        .login_with_factor_receipt(account(), false, receipt(json!(42))?)
        .await?;
    let proof = guard
        .session_factor_proof()
        .await?
        .ok_or_else(state_error)?;
    assert!(proof.matches(&json!(42), "web", None, "db/main/users", "generation-1"));
    for (id, name, provider, source, generation) in [
        (json!("42"), "web", None, "db/main/users", "generation-1"),
        (json!(42), "api", None, "db/main/users", "generation-1"),
        (
            json!(42),
            "web",
            Some("other"),
            "db/main/users",
            "generation-1",
        ),
        (json!(42), "web", None, "db/other/users", "generation-1"),
        (json!(42), "web", None, "db/main/users", "generation-2"),
    ] {
        assert!(!proof.matches(&id, name, provider, source, generation));
    }
    guard.login(account(), false).await?;
    assert!(guard.session_factor_proof().await?.is_none());
    guard
        .login_with_factor_receipt(account(), false, receipt(json!(42))?)
        .await?;
    guard.set_user(account()).await?;
    assert!(
        guard
            .session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_none()
    );
    guard
        .login_with_factor_receipt(account(), false, receipt(json!(42))?)
        .await?;
    guard.logout().await?;
    assert!(
        guard
            .session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_none()
    );
    Ok(())
}
#[tokio::test]
async fn factor_attestation_keeps_current_persistent_identity_and_session_id() -> Result<()> {
    let guard = regeneration_guard(
        Arc::new(RegenerationStore::default()),
        Arc::new(EmptyProvider(false)),
        None,
    )
    .await?;
    assert!(
        guard
            .attest_factor_receipt(receipt(json!(42))?)
            .await
            .is_err()
    );
    guard.login(account(), false).await?;
    let before = guard.session.id()?;
    let context = guard.factor_management_context().await?;
    assert_eq!(context.identifier(), &json!(42));
    assert!(context.factor_proof().is_none());
    guard.attest_factor_receipt(receipt(json!(42))?).await?;
    assert_eq!(guard.session.id()?, before);
    assert_eq!(guard.id().await?, Some(json!(42)));
    assert!(guard.session_factor_proof().await?.is_some());
    assert_eq!(
        guard.factor_management_context().await?.session_binding(),
        context.session_binding()
    );
    assert!(
        guard
            .factor_management_context()
            .await?
            .factor_proof()
            .is_some()
    );
    assert!(
        guard
            .attest_factor_receipt(receipt(json!(7))?)
            .await
            .is_err()
    );
    assert!(guard.session_factor_proof().await?.is_some());
    guard.set_user(account()).await?;
    assert!(
        guard
            .attest_factor_receipt(receipt(json!(42))?)
            .await
            .is_err()
    );
    assert!(guard.factor_management_context().await.is_err());
    Ok(())
}
#[tokio::test]
async fn login_and_authenticated_listeners_cannot_read_uncommitted_factor_proof() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
    let observed = Arc::new(AtomicUsize::new(0));
    let mut events = crate::events::EventsServiceProvider::new();
    macro_rules! observe_pending {
        ($event:ty) => {{
            let slot = slot.clone();
            let observed = observed.clone();
            events.listen::<$event, _>(move |_, _| {
                let guard = slot
                    .read()
                    .map_err(|_| state_error())
                    .and_then(|slot| slot.upgrade().ok_or_else(state_error));
                let observed = observed.clone();
                Box::pin(async move {
                    let guard = guard?;
                    assert!(guard.user().await?.is_some());
                    assert!(
                        guard
                            .session
                            .get_json(&session_factor_key(&guard.session_name()))?
                            .is_some()
                    );
                    assert!(guard.session_factor_proof().await?.is_none());
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            });
        }};
    }
    observe_pending!(Login);
    observe_pending!(Authenticated);
    let guard = Arc::new(
        regeneration_guard(
            Arc::new(RegenerationStore::default()),
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(events).await?),
        )
        .await?,
    );
    *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
    guard
        .login_with_factor_receipt(account(), false, receipt(json!(42))?)
        .await?;
    assert_eq!(observed.load(Ordering::SeqCst), 2);
    assert!(guard.session_factor_proof().await?.is_some());
    Ok(())
}
#[tokio::test]
async fn restored_factor_proof_requires_same_typed_identity_and_provider() -> Result<()> {
    let guard = regeneration_guard(
        Arc::new(RegenerationStore::default()),
        Arc::new(EmptyProvider(true)),
        None,
    )
    .await?;
    guard
        .login_with_factor_receipt(account(), false, receipt(json!(42))?)
        .await?;
    let restored = SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(true)),
        guard.session.clone(),
        guard.cookies.clone(),
        None,
        None,
        None,
    );
    assert!(restored.session_factor_proof().await?.is_some());
    let different = SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(true)),
        guard.session.clone(),
        guard.cookies.clone(),
        None,
        None,
        None,
    )
    .with_provider_name(Some("other".into()));
    assert!(different.session_factor_proof().await?.is_none());
    assert!(
        guard
            .session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_none()
    );
    assert!(
        guard
            .login_with_factor_receipt(account(), false, receipt(json!("42"))?)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn cancelled_old_factor_login_does_not_remove_successor_proof() -> Result<()> {
    let entered = Arc::new(tokio::sync::Notify::new());
    let notify = entered.clone();
    let mut events = crate::events::EventsServiceProvider::new();
    events.listen::<Login, _>(move |event, _| {
        let notify = notify.clone();
        let id = event.user.auth_identifier();
        Box::pin(async move {
            if id? == json!(42) {
                notify.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        regeneration_guard(
            Arc::new(RegenerationStore::default()),
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(events).await?),
        )
        .await?,
    );
    let first = guard.clone();
    let worker = tokio::spawn(async move {
        first
            .login_with_factor_receipt(account(), false, receipt(json!(42))?)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    guard
        .login_with_factor_receipt(identified_account(7), false, receipt(json!(7))?)
        .await?;
    worker.abort();
    assert!(worker.await.is_err());
    let proof = guard
        .session_factor_proof()
        .await?
        .ok_or_else(state_error)?;
    assert!(proof.matches(&json!(7), "web", None, "db/main/users", "generation-1"));
    assert_eq!(guard.id().await?, Some(json!(7)));
    Ok(())
}
#[tokio::test]
async fn failed_extra_regeneration_cleans_factor_proof_and_identity() -> Result<()> {
    let guard = regeneration_guard(
        Arc::new(RegenerationStore {
            fail_second: true,
            ..RegenerationStore::default()
        }),
        Arc::new(EmptyProvider(false)),
        None,
    )
    .await?;
    assert!(
        guard
            .login_with_factor_receipt(account(), true, receipt(json!(42))?)
            .await
            .is_err()
    );
    assert_failed_closed(&guard).await?;
    assert!(
        guard
            .session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_none()
    );
    Ok(())
}
#[tokio::test]
async fn cancelled_factor_login_fences_busy_session_then_retry_cleans_proof() -> Result<()> {
    let BusyCancellationFixture {
        backend,
        session,
        admin: _,
        migration,
        entered,
        guard,
    } = busy_cancellation_fixture().await?;
    let capture = guard.clone();
    let worker = tokio::spawn(async move {
        capture
            .login_with_factor_receipt(account(), false, receipt(json!(42))?)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    assert!(
        session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_some()
    );
    worker.abort();
    assert!(worker.await.is_err());
    assert!(session.save().await.is_err());
    assert_eq!(
        guard
            .session_factor_proof()
            .await
            .err()
            .ok_or_else(state_error)?
            .code(),
        "session_authentication_transition_failed"
    );
    backend
        .pause_delete
        .store(false, std::sync::atomic::Ordering::SeqCst);
    backend.delete_release.notify_one();
    let pending = migration
        .lock()
        .map_err(|_| state_error())?
        .take()
        .ok_or_else(state_error)?;
    tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)??;
    tokio::time::timeout(
        Duration::from_secs(2),
        guard.login(identified_account(7), false),
    )
    .await
    .map_err(Error::internal)??;
    assert!(
        guard
            .session
            .get_json(&session_factor_key(&guard.session_name()))?
            .is_none()
    );
    assert!(guard.session_factor_proof().await?.is_none());
    Ok(())
}
// 本例只初始化私有Rejected原因测试状态机；不声称真实authority拒绝流程已执行。
// 其后登录、事件后台migrate、store.delete屏障、取消与Session fence均走现有真实fixture。
#[tokio::test]
async fn rejected_reason_does_not_hide_cancelled_successor_authentication_fence() -> Result<()> {
    use std::sync::atomic::Ordering;
    let BusyCancellationFixture {
        backend,
        session,
        admin: _,
        migration,
        entered,
        guard,
    } = busy_cancellation_fixture().await?;
    {
        let mut state = guard.write()?;
        state.logged_out = true;
        state.recalled_identity = RecallStatus::Rejected;
    }
    assert!(guard.session_factor_proof().await?.is_none());
    let capture = guard.clone();
    let worker = tokio::spawn(async move {
        capture
            .login_with_factor_receipt(account(), false, receipt(json!(42))?)
            .await
    });
    let reached = tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal);
    worker.abort();
    assert!(worker.await.is_err());
    let outcome: Result<()> = async {
        reached?;
        assert_eq!(
            session
                .require_authenticated_storage()
                .err()
                .ok_or_else(state_error)?
                .code(),
            "session_authentication_transition_failed"
        );
        assert_eq!(
            guard
                .session_factor_proof()
                .await
                .err()
                .ok_or_else(state_error)?
                .code(),
            "session_authentication_transition_failed"
        );
        Ok(())
    }
    .await;
    backend.pause_delete.store(false, Ordering::SeqCst);
    backend.delete_release.notify_one();
    let pending = migration
        .lock()
        .map_err(|_| state_error())?
        .take()
        .ok_or_else(state_error)?;
    tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)??;
    outcome?;
    assert_eq!(
        guard
            .session_factor_proof()
            .await
            .err()
            .ok_or_else(state_error)?
            .code(),
        "session_authentication_transition_failed"
    );
    tokio::time::timeout(
        Duration::from_secs(2),
        guard.login(identified_account(7), false),
    )
    .await
    .map_err(Error::internal)??;
    assert!(guard.user().await?.is_some());
    assert!(guard.session_factor_proof().await?.is_none());
    Ok(())
}

// 只用私有Rejected种子隔离状态机边界，不声称真实authority策略拒绝已运行。
// 无Events监听器：屏障位于首次update_session.regenerate(true)的store.delete，早于login_inner后续成功重置。
#[tokio::test]
async fn rejected_reason_does_not_hide_first_regeneration_cancellation_fence() -> Result<()> {
    use std::sync::atomic::Ordering;
    let backend = Arc::new(BackgroundSaveStore::default());
    let session = Session::new(None, backend.clone(), None)?;
    session.start().await?;
    let guard = Arc::new(SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(false)),
        session.clone(),
        CookieJar::new(crate::http::cookie::CookieFactory::default()),
        None,
        None,
        None,
    ));
    {
        let mut state = guard.write()?;
        state.logged_out = true;
        state.recalled_identity = RecallStatus::Rejected;
    }
    assert!(guard.session_factor_proof().await?.is_none());
    backend.pause_delete.store(true, Ordering::SeqCst);
    let capture = guard.clone();
    let worker = tokio::spawn(async move {
        capture
            .login_with_factor_receipt(account(), false, receipt(json!(42))?)
            .await
    });
    let reached = tokio::time::timeout(Duration::from_secs(2), backend.delete_entered.notified())
        .await
        .map_err(Error::internal);
    worker.abort();
    assert!(worker.await.is_err());
    let outcome: Result<()> = async {
        reached?;
        assert_eq!(
            session
                .require_authenticated_storage()
                .err()
                .ok_or_else(state_error)?
                .code(),
            "session_authentication_transition_failed"
        );
        assert_eq!(
            guard
                .session_factor_proof()
                .await
                .err()
                .ok_or_else(state_error)?
                .code(),
            "session_authentication_transition_failed"
        );
        Ok(())
    }
    .await;
    backend.pause_delete.store(false, Ordering::SeqCst);
    backend.delete_release.notify_one();
    // 明确取得同一Session I/O门完成一次迁移，而非用sleep猜测取消任务已退出。
    tokio::time::timeout(Duration::from_secs(2), session.migrate(false))
        .await
        .map_err(Error::internal)??;
    outcome?;
    assert_eq!(
        guard
            .session_factor_proof()
            .await
            .err()
            .ok_or_else(state_error)?
            .code(),
        "session_authentication_transition_failed"
    );
    tokio::time::timeout(
        Duration::from_secs(2),
        guard.login(identified_account(7), false),
    )
    .await
    .map_err(Error::internal)??;
    assert_eq!(guard.id().await?, Some(json!(7)));
    assert!(guard.user().await?.is_some());
    assert!(guard.session_factor_proof().await?.is_none());
    Ok(())
}

#[test]
fn recaller_identifiers_reject_ambiguous_values_without_losing_numeric_precision() -> Result<()> {
    assert_eq!(recaller_identifier(json!(u64::MAX))?, u64::MAX.to_string());
    assert_eq!(
        recaller_identifier(json!("tenant:user-42"))?,
        "tenant:user-42"
    );
    for invalid in [Value::Null, json!({"id": 42}), json!([42]), json!(" ")] {
        assert!(recaller_identifier(invalid).is_err());
    }
    Ok(())
}

#[test]
fn failed_events_hide_all_credentials_and_user_details() {
    let event = Failed {
        guard: "web".into(),
        user: None,
        credentials: Credentials::new(serde_json::Map::from_iter([(
            "password".into(),
            json!("private-password"),
        )])),
    };
    let output = format!("{event:?}");
    assert!(!output.contains("private-password"));
    assert!(!output.contains("credentials"));
}
struct EmptyProvider(bool);
impl UserProvider for EmptyProvider {
    fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok(self.0.then(account)) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.0 {
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn login_and_authenticated_listeners_can_read_same_guard() -> Result<()> {
    use crate::events::EventsServiceProvider;
    use crate::foundation::ProviderBuilder;
    use crate::http::cookie::CookieFactory;
    use crate::session::MemorySessionStore;
    use std::sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
    };
    let slot = Arc::new(RwLock::new(Weak::<SessionGuard>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut events = EventsServiceProvider::new();
    let capture = slot.clone();
    let observed = calls.clone();
    events.listen::<Login, _>(move |event, _| {
        let capture = capture.clone();
        let observed = observed.clone();
        let identifier = event.user.auth_identifier();
        Box::pin(async move {
            let guard = capture
                .read()
                .map_err(|_| state_error())?
                .upgrade()
                .ok_or_else(state_error)?;
            assert_eq!(guard.id().await?, Some(identifier?));
            assert!(guard.user().await?.is_some());
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let capture = slot.clone();
    let observed = calls.clone();
    events.listen::<Authenticated, _>(move |_, _| {
        let capture = capture.clone();
        let observed = observed.clone();
        Box::pin(async move {
            let guard = capture
                .read()
                .map_err(|_| state_error())?
                .upgrade()
                .ok_or_else(state_error)?;
            assert!(guard.user().await?.is_some());
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(events);
    let runtime = builder.start(figment::Figment::new()).await?;
    let dispatcher = (*runtime.container().resolve::<Dispatcher>().await?).clone();
    let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
    session.start().await?;
    let guard = Arc::new(SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(false)),
        session,
        CookieJar::new(CookieFactory::default()),
        None,
        Some(dispatcher),
        None,
    ));
    *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
    let user: AuthUser = Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
        [("id".into(), json!(42))],
    )));
    tokio::time::timeout(Duration::from_secs(2), guard.login(user, false))
        .await
        .map_err(Error::internal)??;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
async fn failure_guard(
    provider: Arc<dyn UserProvider>,
    events: Option<Dispatcher>,
) -> Result<SessionGuard> {
    let session = Session::new(
        None,
        Arc::new(crate::session::MemorySessionStore::default()),
        None,
    )?;
    session.start().await?;
    session.put_typed("unrelated", "retained")?;
    Ok(SessionGuard::new(
        "web".into(),
        provider,
        session,
        CookieJar::new(crate::http::cookie::CookieFactory::default()),
        None,
        events,
        None,
    )
    .with_remember_cookie_protected(true))
}
fn account() -> AuthUser {
    Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
        [("id".into(), json!(42)), ("password".into(), json!("hash"))],
    )))
}
async fn assert_failed_closed(guard: &SessionGuard) -> Result<()> {
    assert!(guard.user().await?.is_none());
    assert!(guard.id().await?.is_none());
    assert!(!guard.has_user()?);
    assert!(guard.session.get_json(&guard.session_name())?.is_none());
    assert!(guard.session.get_json("password_hash_web")?.is_none());
    assert!(!guard.cookies.has_queued(&guard.recaller_name(), None)?);
    assert_eq!(
        guard.session.get_json("unrelated")?,
        Some(json!("retained"))
    );
    Ok(())
}
#[tokio::test]
async fn cancelled_token_persistence_removes_partial_login() -> Result<()> {
    let guard = failure_guard(Arc::new(EmptyProvider(true)), None).await?;
    let user = account();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), guard.login(user.clone(), true))
            .await
            .is_err()
    );
    assert!(user.remember_token()?.is_none());
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn cookie_creation_failure_removes_partial_login() -> Result<()> {
    let guard = failure_guard(Arc::new(EmptyProvider(false)), None)
        .await?
        .with_remember_duration(i64::MAX);
    assert!(guard.login(account(), true).await.is_err());
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn login_event_error_removes_identity_and_queued_recaller() -> Result<()> {
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Login, _>(|_, _| {
        Box::pin(async { Err(Error::custom("listener_failed", "测试监听器失败")) })
    });
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let events = (*runtime.container().resolve::<Dispatcher>().await?).clone();
    let guard = failure_guard(Arc::new(EmptyProvider(false)), Some(events)).await?;
    assert!(guard.login(account(), true).await.is_err());
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn logout_event_error_still_removes_identity_and_cookie() -> Result<()> {
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Logout, _>(|_, _| {
        Box::pin(async { Err(Error::custom("listener_failed", "测试监听器失败")) })
    });
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let events = (*runtime.container().resolve::<Dispatcher>().await?).clone();
    let guard = failure_guard(Arc::new(EmptyProvider(false)), Some(events)).await?;
    guard.login(account(), true).await?;
    assert!(guard.logout().await.is_err());
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn authenticated_listener_logout_is_observed_by_outer_user() -> Result<()> {
    let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
    let captured = slot.clone();
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Authenticated, _>(move |_, _| {
        let captured = captured.clone();
        Box::pin(async move {
            let guard = captured
                .read()
                .map_err(|_| state_error())?
                .upgrade()
                .ok_or_else(state_error)?;
            guard.logout().await
        })
    });
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let events = (*runtime.container().resolve::<Dispatcher>().await?).clone();
    let guard = Arc::new(failure_guard(Arc::new(EmptyProvider(true)), Some(events)).await?);
    guard.session.put_typed(&guard.session_name(), 42)?;
    *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
    let resolved = tokio::time::timeout(Duration::from_secs(2), guard.user())
        .await
        .map_err(Error::internal)??;
    assert!(resolved.is_none());
    assert!(guard.user().await?.is_none());
    Ok(())
}
struct ProofRememberProvider {
    user: AuthUser,
    database: Option<crate::database::Db>,
}
impl UserProvider for ProofRememberProvider {
    fn uses_database_accounts(&self, db: &crate::database::Db, table: &str) -> Result<bool> {
        Ok(table == "users"
            && self
                .database
                .as_ref()
                .is_some_and(|bound| bound.same_handle(db)))
    }
    fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok(Some(self.user.clone())) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        token: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            Ok((self.user.remember_token()?.as_deref() == Some(token)).then(|| self.user.clone()))
        })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
struct CacheValidationProvider {
    user: AuthUser,
    database: Option<crate::database::Db>,
    armed: std::sync::atomic::AtomicBool,
    reached: Arc<tokio::sync::Notify>,
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
impl UserProvider for CacheValidationProvider {
    fn uses_database_accounts(&self, db: &crate::database::Db, table: &str) -> Result<bool> {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.reached.notify_one();
        }
        Ok(table == "users"
            && self
                .database
                .as_ref()
                .is_some_and(|bound| bound.same_handle(db)))
    }
    fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok(Some(self.user.clone())) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        token: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            Ok((self.user.remember_token()?.as_deref() == Some(token)).then(|| self.user.clone()))
        })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
fn proof_value_for_test(guard: &SessionGuard) -> Result<Value> {
    guard
        .session
        .get_json(&session_factor_key(&guard.session_name()))?
        .ok_or_else(state_error)
}
fn queued_recaller(guard: &SessionGuard) -> Result<String> {
    let cookie = guard
        .cookies
        .queued(&guard.recaller_name(), None)?
        .ok_or_else(state_error)?;
    cookie.value().map(str::to_owned).ok_or_else(state_error)
}
async fn proof_remember_guard(
    provider: Arc<dyn UserProvider>,
    wire: Option<String>,
) -> Result<SessionGuard> {
    Ok(failure_guard(provider, None)
        .await?
        .with_decrypted_recaller(wire))
}
#[tokio::test]
async fn same_token_old_cookie_never_inherits_new_cookie_factor_proof() -> Result<()> {
    let user = account();
    let provider: Arc<dyn UserProvider> = Arc::new(ProofRememberProvider {
        user: user.clone(),
        database: None,
    });
    let ordinary = proof_remember_guard(provider.clone(), None).await?;
    ordinary.login(user.clone(), true).await?;
    let old_cookie = queued_recaller(&ordinary)?;
    assert_eq!(old_cookie.split('|').count(), 3);
    let verified = proof_remember_guard(provider.clone(), None).await?;
    verified
        .login_with_factor_receipt(user.clone(), true, receipt(json!(42))?)
        .await?;
    let cookie = queued_recaller(&verified)?;
    assert_eq!(cookie.split('|').count(), 4);
    assert_eq!(
        cookie.split('|').take(3).collect::<Vec<_>>().join("|"),
        old_cookie
    );
    ordinary.session.put_typed(
        &session_factor_key(&ordinary.session_name()),
        &proof_value_for_test(&verified)?,
    )?;
    ordinary.login(user.clone(), true).await?;
    assert_eq!(queued_recaller(&ordinary)?.split('|').count(), 3);
    let old = proof_remember_guard(provider.clone(), Some(old_cookie)).await?;
    assert_eq!(old.id().await?, Some(json!(42)));
    assert!(old.session_factor_proof().await?.is_none());
    let restored = proof_remember_guard(provider.clone(), Some(cookie.clone())).await?;
    assert!(restored.user().await?.is_none());
    assert!(restored.session_factor_proof().await?.is_none());
    for (guard_name, provider_name) in [("admin", None), ("web", Some("other"))] {
        let mut wrong = proof_remember_guard(provider.clone(), Some(cookie.clone())).await?;
        wrong.name = guard_name.into();
        wrong.provider_name = provider_name.map(str::to_owned);
        assert!(wrong.user().await?.is_none());
        assert!(wrong.session.get_json(&wrong.session_name())?.is_none());
    }
    let changed: AuthUser = Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
        [
            ("id".into(), json!("42")),
            ("password".into(), json!("hash")),
            ("remember_token".into(), json!(user.remember_token()?)),
        ],
    )));
    let wrong_id = proof_remember_guard(
        Arc::new(ProofRememberProvider {
            user: changed,
            database: None,
        }),
        Some(cookie),
    )
    .await?;
    assert!(wrong_id.user().await?.is_none());
    Ok(())
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
struct ProofRememberAccount {
    attributes: super::super::GenericUser,
    authority: Option<crate::auth::fortify::DatabaseFactorAuthority>,
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
impl super::super::Authenticatable for ProofRememberAccount {
    fn auth_identifier_name(&self) -> &str {
        self.attributes.auth_identifier_name()
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &str {
        self.attributes.auth_password_name()
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.attributes.auth_password()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(&self, token: Option<String>) -> BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(token)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.attributes
            .begin_remember_token_update(expected, replacement)
    }
    fn remember_token_name(&self) -> &str {
        self.attributes.remember_token_name()
    }
    fn as_two_factor(&self) -> Option<&dyn crate::auth::fortify::TwoFactorAuthenticatable> {
        Some(self)
    }
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
impl crate::auth::fortify::TwoFactorAuthenticatable for ProofRememberAccount {
    fn two_factor_account_name(&self, _: &str) -> Result<Option<String>> {
        Ok(Some("remember-proof@example.test".into()))
    }
    fn two_factor_authority(&self) -> Option<&crate::auth::fortify::DatabaseFactorAuthority> {
        self.authority.as_ref()
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<crate::auth::fortify::TwoFactorState>> {
        Box::pin(async move {
            match &self.authority {
                Some(authority) => Ok(authority.load().await?.state),
                None => Ok(crate::auth::fortify::TwoFactorState::default()),
            }
        })
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        _: &'a crate::auth::fortify::TwoFactorState,
        _: &'a crate::auth::fortify::TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Err(state_error()) })
    }
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
struct RememberEmptyMigrations;
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
impl crate::database::sea_orm_migration::MigratorTrait for RememberEmptyMigrations {
    fn migrations() -> Vec<Box<dyn crate::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
struct RememberAuthorityFixture {
    db: crate::database::Db,
    identifier: Value,
    encrypter: Arc<dyn crate::encryption::ApplicationEncrypter>,
    authority: crate::auth::fortify::DatabaseFactorAuthority,
    user: AuthUser,
    provider: Arc<dyn UserProvider>,
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
async fn remember_authority_case<F>(callback: F) -> Result<()>
where
    F: FnOnce(RememberAuthorityFixture) -> BoxFuture<'static, Result<()>>,
{
    use crate::auth::fortify::{DatabaseFactorAuthority, DatabaseTwoFactorState};
    use crate::database::sea_orm::ConnectionTrait;
    let database = crate::testing::TestDatabase::sqlite_file::<RememberEmptyMigrations>().await?;
    let db = crate::database::Db::default();
    let outcome: Result<()> = async {
        db.initialize(database.connection()?)?;
        db.connection()?.execute_unprepared("CREATE TABLE users (id TEXT PRIMARY KEY, two_factor_secret TEXT, two_factor_recovery_codes TEXT, two_factor_confirmed_at datetime, two_factor_authority_source TEXT, two_factor_authority_generation TEXT, two_factor_consumed_codes TEXT, two_factor_management_session_hash TEXT, two_factor_management_generation TEXT, two_factor_management_authorized_at datetime)").await.map_err(Error::internal)?;
        let identifier = json!("tenant-user-42");
        db.table("users")?.insert(vec![("id", "tenant-user-42".into())]).await?;
        let encrypter = crate::auth::fortify::test_field_encrypter("KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=")?;
        let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(db.clone(), "users", "id", identifier.clone())?);
        let user: AuthUser = Arc::new(ProofRememberAccount {
            attributes: super::super::GenericUser::new(serde_json::Map::from_iter([("id".into(), identifier.clone()), ("password".into(), json!("hash"))])), authority: Some(authority.clone()),
        });
        let provider: Arc<dyn UserProvider> = Arc::new(ProofRememberProvider { user: user.clone(), database: Some(db.clone()) });
        callback(RememberAuthorityFixture { db: db.clone(), identifier, encrypter, authority, user, provider }).await
    }.await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn selected_authority_recall_requires_current_source_generation_and_never_upgrades_old_cookie()
-> Result<()> {
    use crate::auth::fortify::{FactorAuthorityMutation, TwoFactorState};
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let RememberAuthorityFixture {
                db,
                identifier,
                encrypter,
                authority,
                user,
                provider,
            } = fixture;
            let ordinary = proof_remember_guard(provider.clone(), None).await?;
            ordinary.login(user.clone(), true).await?;
            let old_cookie = queued_recaller(&ordinary)?;
            let before = authority.load().await?;
            let enabled = authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(encrypter.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                        recovery_codes: Some(encrypter.encrypt_serialized_string(
                            br#"["one","two","three","four","five","six","seven","eight"]"#,
                        )?),
                        confirmed_at: Some(chrono::Utc::now()),
                    },
                    FactorAuthorityMutation::Enable,
                )
                .await?;
            let receipt = FactorReceipt::issue(
                identifier.clone(),
                "web".into(),
                None,
                enabled.source.clone(),
                enabled.generation.clone(),
                super::super::FactorVerificationMethod::Totp,
                123,
            )?;
            let verified = proof_remember_guard(provider.clone(), None).await?;
            verified
                .login_with_factor_receipt(user.clone(), true, receipt)
                .await?;
            let cookie = queued_recaller(&verified)?;
            assert_eq!(
                cookie.split('|').take(3).collect::<Vec<_>>().join("|"),
                old_cookie
            );
            for (name, provider_name) in [("admin", None), ("web", Some("other"))] {
                let mut wrong =
                    proof_remember_guard(provider.clone(), Some(cookie.clone())).await?;
                wrong.name = name.into();
                wrong.provider_name = provider_name.map(str::to_owned);
                assert!(wrong.user().await?.is_none());
            }
            let restored = proof_remember_guard(provider.clone(), Some(cookie.clone())).await?;
            assert!(restored.user().await?.is_some());
            assert!(
                restored
                    .session_factor_proof()
                    .await?
                    .ok_or_else(state_error)?
                    .matches(
                        &identifier,
                        "web",
                        None,
                        &enabled.source,
                        &enabled.generation
                    )
            );
            let old = proof_remember_guard(provider.clone(), Some(old_cookie.clone())).await?;
            assert!(old.user().await?.is_none());
            assert!(old.session_factor_proof().await?.is_none());
            reject_invalid_recall_adapters(&db, &identifier, &authority, &user, &provider, &cookie)
                .await?;
            authority
                .replace(
                    &enabled,
                    &TwoFactorState::default(),
                    FactorAuthorityMutation::Disable,
                )
                .await?;
            let expired = proof_remember_guard(provider.clone(), Some(cookie)).await?;
            assert!(expired.user().await?.is_none());
            let disabled = proof_remember_guard(provider, Some(old_cookie)).await?;
            assert!(disabled.user().await?.is_some());
            assert!(disabled.session_factor_proof().await?.is_none());
            Ok(())
        })
    })
    .await
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
async fn reject_invalid_recall_adapters(
    db: &crate::database::Db,
    identifier: &Value,
    authority: &crate::auth::fortify::DatabaseFactorAuthority,
    user: &AuthUser,
    provider: &Arc<dyn UserProvider>,
    cookie: &str,
) -> Result<()> {
    let wrong_identifier: AuthUser = Arc::new(ProofRememberAccount {
        attributes: super::super::GenericUser::new(serde_json::Map::from_iter([
            ("id".into(), json!(42)),
            ("password".into(), json!("hash")),
            ("remember_token".into(), json!(user.remember_token()?)),
        ])),
        authority: Some(authority.clone()),
    });
    let wrong = proof_remember_guard(
        Arc::new(ProofRememberProvider {
            user: wrong_identifier,
            database: Some(db.clone()),
        }),
        Some(cookie.to_owned()),
    )
    .await?;
    assert!(wrong.user().await?.is_none());
    for field in ["source", "generation"] {
        let mut parts = cookie.split('|').map(str::to_owned).collect::<Vec<_>>();
        let encoded = parts
            .get(3)
            .and_then(|part| part.strip_prefix(RECALLER_PROOF_PREFIX))
            .ok_or_else(state_error)?;
        let mut json: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).map_err(Error::internal)?)
                .map_err(Error::internal)?;
        json[field] = json!("outdated");
        parts[3] = format!(
            "{RECALLER_PROOF_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json).map_err(Error::internal)?)
        );
        let invalid = proof_remember_guard(provider.clone(), Some(parts.join("|"))).await?;
        assert!(invalid.user().await?.is_none());
    }
    let no_adapter: AuthUser = Arc::new(ProofRememberAccount {
        attributes: super::super::GenericUser::new(serde_json::Map::from_iter([
            ("id".into(), identifier.clone()),
            ("password".into(), json!("hash")),
            ("remember_token".into(), json!(user.remember_token()?)),
        ])),
        authority: None,
    });
    let missing = proof_remember_guard(
        Arc::new(ProofRememberProvider {
            user: no_adapter,
            database: Some(db.clone()),
        }),
        Some(cookie.to_owned()),
    )
    .await?;
    assert!(missing.user().await?.is_none());
    let wrong_db = proof_remember_guard(
        Arc::new(ProofRememberProvider {
            user: user.clone(),
            database: None,
        }),
        Some(cookie.to_owned()),
    )
    .await?;
    assert!(wrong_db.user().await.is_err());
    Ok(())
}
#[cfg(feature = "auth-fortify")]
#[tokio::test]
async fn required_authority_rejects_generic_three_segment_recaller() -> Result<()> {
    let user = account();
    let provider: Arc<dyn UserProvider> = Arc::new(ProofRememberProvider {
        user: user.clone(),
        database: None,
    });
    let original = proof_remember_guard(provider.clone(), None).await?;
    original.login(user, true).await?;
    let cookie = queued_recaller(&original)?;
    assert_eq!(cookie.split('|').count(), 3);
    let required = proof_remember_guard(provider.clone(), Some(cookie.clone()))
        .await?
        .with_factor_authority_policy(true, false);
    assert!(required.user().await?.is_none());
    assert!(
        required
            .session
            .get_json(&required.session_name())?
            .is_none()
    );
    let official = proof_remember_guard(provider, Some(cookie)).await?;
    assert!(official.user().await?.is_some());
    assert!(official.session_factor_proof().await?.is_none());
    Ok(())
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn required_recaller_checks_adapter_and_actual_pending_confirmation_policy() -> Result<()> {
    use crate::auth::fortify::{FactorAuthorityMutation, TwoFactorState};
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let RememberAuthorityFixture {
                db,
                identifier,
                encrypter,
                authority,
                user,
                provider,
            } = fixture;
            let original = proof_remember_guard(provider.clone(), None).await?;
            original.login(user.clone(), true).await?;
            let cookie = queued_recaller(&original)?;
            let disabled = proof_remember_guard(provider.clone(), Some(cookie.clone()))
                .await?
                .with_factor_authority_policy(true, false);
            assert!(disabled.user().await?.is_some());
            assert!(disabled.session_factor_proof().await?.is_none());
            let no_adapter: AuthUser = Arc::new(ProofRememberAccount {
                attributes: super::super::GenericUser::new(serde_json::Map::from_iter([
                    ("id".into(), identifier),
                    ("password".into(), json!("hash")),
                    ("remember_token".into(), json!(user.remember_token()?)),
                ])),
                authority: None,
            });
            let missing = proof_remember_guard(
                Arc::new(ProofRememberProvider {
                    user: no_adapter,
                    database: Some(db),
                }),
                Some(cookie.clone()),
            )
            .await?
            .with_factor_authority_policy(true, true);
            assert!(missing.user().await?.is_none());
            let before = authority.load().await?;
            authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(encrypter.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                        recovery_codes: Some(encrypter.encrypt_serialized_string(
                            br#"["one","two","three","four","five","six","seven","eight"]"#,
                        )?),
                        confirmed_at: None,
                    },
                    FactorAuthorityMutation::Enable,
                )
                .await?;
            for confirm in [false, true] {
                let pending = proof_remember_guard(provider.clone(), Some(cookie.clone()))
                    .await?
                    .with_factor_authority_policy(true, confirm);
                assert_eq!(pending.user().await?.is_some(), confirm);
                assert!(pending.session_factor_proof().await?.is_none());
                assert_eq!(
                    pending.session.get_json(&pending.session_name())?.is_some(),
                    confirm
                );
            }
            Ok(())
        })
    })
    .await
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn same_guard_pending_without_proof_rechecks_current_factor_confirmation() -> Result<()> {
    use crate::auth::fortify::{FactorAuthorityMutation, TwoFactorState};
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let original = proof_remember_guard(fixture.provider.clone(), None).await?;
            original.login(fixture.user.clone(), true).await?;
            let cookie = queued_recaller(&original)?;
            let before = fixture.authority.load().await?;
            fixture
                .authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(
                            fixture
                                .encrypter
                                .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
                        ),
                        recovery_codes: Some(fixture.encrypter.encrypt_serialized_string(
                            br#"["one","two","three","four","five","six","seven","eight"]"#,
                        )?),
                        confirmed_at: None,
                    },
                    FactorAuthorityMutation::Enable,
                )
                .await?;
            for disable_feature in [false, true] {
                let config =
                    crate::config::repository::ConfigRepository::new(figment::Figment::new());
                config.set("fortify.features", json!(["two-factor-authentication"]))?;
                config.set(
                    "fortify-options.two-factor-authentication.confirm",
                    json!(true),
                )?;
                let guard = proof_remember_guard(fixture.provider.clone(), Some(cookie.clone()))
                    .await?
                    .with_factor_authority_policy(true, false)
                    .with_factor_config(config.clone());
                assert!(guard.user().await?.is_some());
                assert!(guard.session_factor_proof().await?.is_none());
                assert!(guard.user().await?.is_some());
                if disable_feature {
                    config.set("fortify.features", json!([]))?;
                } else {
                    config.set(
                        "fortify-options.two-factor-authentication.confirm",
                        json!(false),
                    )?;
                }
                // 同一个实例已经缓存真实身份；不得通过新Scope/新Cookie重取掩盖旧配置。
                assert!(guard.user().await?.is_none());
                assert!(guard.session_factor_proof().await?.is_none());
                guard.login(fixture.user.clone(), false).await?;
                assert!(guard.via_remember()?);
                assert!(guard.user().await?.is_some());
                assert!(guard.session_factor_proof().await?.is_none());
            }
            Ok(())
        })
    })
    .await
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn recalled_login_listener_policy_change_rejects_first_user_resolution() -> Result<()> {
    use crate::auth::fortify::{FactorAuthorityMutation, TwoFactorState};
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let original = proof_remember_guard(fixture.provider.clone(), None).await?;
            original.login(fixture.user.clone(), true).await?;
            let cookie = queued_recaller(&original)?;
            let before = fixture.authority.load().await?;
            fixture
                .authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(
                            fixture
                                .encrypter
                                .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
                        ),
                        recovery_codes: Some(fixture.encrypter.encrypt_serialized_string(
                            br#"["one","two","three","four","five","six","seven","eight"]"#,
                        )?),
                        confirmed_at: None,
                    },
                    FactorAuthorityMutation::Enable,
                )
                .await?;
            for disable_feature in [false, true] {
                let config =
                    crate::config::repository::ConfigRepository::new(figment::Figment::new());
                config.set("fortify.features", json!(["two-factor-authentication"]))?;
                config.set(
                    "fortify-options.two-factor-authentication.confirm",
                    json!(true),
                )?;
                let captured = config.clone();
                let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let observed = calls.clone();
                let mut events = crate::events::EventsServiceProvider::new();
                events.listen::<Login, _>(move |event, _| {
                    let captured = captured.clone();
                    let observed = observed.clone();
                    assert!(event.remember);
                    Box::pin(async move {
                        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if disable_feature {
                            captured.set("fortify.features", json!([]))?;
                        } else {
                            captured.set(
                                "fortify-options.two-factor-authentication.confirm",
                                json!(false),
                            )?;
                        }
                        Ok(())
                    })
                });
                let guard = failure_guard(
                    fixture.provider.clone(),
                    Some(test_dispatcher(events).await?),
                )
                .await?
                .with_decrypted_recaller(Some(cookie.clone()))
                .with_factor_authority_policy(true, false)
                .with_factor_config(config);
                assert!(guard.user().await?.is_none());
                assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
                assert!(guard.session_factor_proof().await?.is_none());
                assert!(guard.session.get_json(&guard.session_name())?.is_none());
                assert!(guard.user().await?.is_none());
                assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            }
            Ok(())
        })
    })
    .await
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
async fn cached_pending_race_guard(
    fixture: &RememberAuthorityFixture,
) -> Result<(SessionGuard, Arc<CacheValidationProvider>)> {
    use crate::auth::fortify::{FactorAuthorityMutation, TwoFactorState};
    let original = proof_remember_guard(fixture.provider.clone(), None).await?;
    original.login(fixture.user.clone(), true).await?;
    let cookie = queued_recaller(&original)?;
    let before = fixture.authority.load().await?;
    fixture
        .authority
        .replace(
            &before,
            &TwoFactorState {
                secret: Some(
                    fixture
                        .encrypter
                        .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
                ),
                recovery_codes: Some(fixture.encrypter.encrypt_serialized_string(
                    br#"["one","two","three","four","five","six","seven","eight"]"#,
                )?),
                confirmed_at: None,
            },
            FactorAuthorityMutation::Enable,
        )
        .await?;
    let provider = Arc::new(CacheValidationProvider {
        user: fixture.user.clone(),
        database: Some(fixture.db.clone()),
        armed: std::sync::atomic::AtomicBool::new(false),
        reached: Arc::new(tokio::sync::Notify::new()),
    });
    let config = crate::config::repository::ConfigRepository::new(figment::Figment::new());
    config.set("fortify.features", json!(["two-factor-authentication"]))?;
    config.set(
        "fortify-options.two-factor-authentication.confirm",
        json!(true),
    )?;
    let guard = proof_remember_guard(provider.clone(), Some(cookie))
        .await?
        .with_factor_authority_policy(true, false)
        .with_factor_config(config.clone());
    assert!(guard.user().await?.is_some());
    config.set(
        "fortify-options.two-factor-authentication.confirm",
        json!(false),
    )?;
    Ok((guard, provider))
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
async fn different_successor_with_test_receipt(
    fixture: &RememberAuthorityFixture,
    session: Session,
) -> Result<(SessionGuard, AuthUser, FactorReceipt)> {
    use crate::auth::fortify::{
        DatabaseFactorAuthority, DatabaseTwoFactorState, FactorAuthorityMutation, TwoFactorState,
    };
    let identifier = json!("successor-user-99");
    fixture
        .db
        .table("users")?
        .insert(vec![("id", "successor-user-99".into())])
        .await?;
    let authority = DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
        fixture.db.clone(),
        "users",
        "id",
        identifier.clone(),
    )?);
    let before = authority.load().await?;
    let current = authority
        .replace(
            &before,
            &TwoFactorState {
                secret: Some(
                    fixture
                        .encrypter
                        .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
                ),
                recovery_codes: Some(fixture.encrypter.encrypt_serialized_string(
                    br#"["one","two","three","four","five","six","seven","eight"]"#,
                )?),
                confirmed_at: Some(chrono::Utc::now()),
            },
            FactorAuthorityMutation::Enable,
        )
        .await?;
    let user: AuthUser = Arc::new(ProofRememberAccount {
        attributes: super::super::GenericUser::new(serde_json::Map::from_iter([
            ("id".into(), identifier.clone()),
            ("password".into(), json!("hash")),
        ])),
        authority: Some(authority),
    });
    let provider: Arc<dyn UserProvider> = Arc::new(ProofRememberProvider {
        user: user.clone(),
        database: Some(fixture.db.clone()),
    });
    let guard = SessionGuard::new(
        "web".into(),
        provider,
        session,
        CookieJar::new(crate::http::cookie::CookieFactory::default()),
        None,
        None,
        None,
    );
    // 只测试sealed证明的转换/租约隔离；此处不声称执行过真实Provider因子校验。
    let receipt = FactorReceipt::issue(
        identifier,
        "web".into(),
        None,
        current.source,
        current.generation,
        super::super::FactorVerificationMethod::Totp,
        123,
    )?;
    Ok((guard, user, receipt))
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
async fn assert_successor_identity_and_proof(
    guard: &SessionGuard,
    expected: &Value,
    proof: &Value,
) -> Result<()> {
    assert_eq!(
        guard.session.get_json(&guard.session_name())?,
        Some(expected.clone())
    );
    assert_eq!(&proof_value_for_test(guard)?, proof);
    assert_eq!(
        &guard
            .user()
            .await?
            .ok_or_else(state_error)?
            .auth_identifier()?,
        expected
    );
    assert!(guard.session_factor_proof().await?.is_some());
    assert_eq!(&proof_value_for_test(guard)?, proof);
    Ok(())
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn stale_cached_recall_validation_preserves_other_guard_successor_lease() -> Result<()> {
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let (old, provider) = cached_pending_race_guard(&fixture).await?;
            let (successor, user, receipt) =
                different_successor_with_test_receipt(&fixture, old.session.clone()).await?;
            let expected = user.auth_identifier()?;
            assert_ne!(expected, fixture.identifier);
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let hold_entered = entered.clone();
            let hold_release = release.clone();
            let db = fixture.db.clone();
            let holder = tokio::spawn(async move {
                db.transaction_for_update(move |_| {
                    Box::pin(async move {
                        hold_entered.notify_one();
                        hold_release.notified().await;
                        Ok(())
                    })
                })
                .await
            });
            let outcome: Result<()> = async {
                tokio::time::timeout(Duration::from_secs(2), entered.notified())
                    .await
                    .map_err(Error::internal)?;
                provider
                    .armed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                let validation = old.user();
                tokio::pin!(validation);
                tokio::time::timeout(Duration::from_secs(2), async {
                    tokio::select! {
                        () = provider.reached.notified() => Ok(()),
                        result = &mut validation => { result?; Err(state_error()) },
                    }
                })
                .await
                .map_err(Error::internal)??;
                successor
                    .login_with_factor_receipt(user, false, receipt)
                    .await?;
                let proof = proof_value_for_test(&successor)?;
                assert!(!proof.is_null());
                release.notify_one();
                assert!(
                    tokio::time::timeout(Duration::from_secs(2), &mut validation)
                        .await
                        .map_err(Error::internal)??
                        .is_none()
                );
                assert_successor_identity_and_proof(&successor, &expected, &proof).await
            }
            .await;
            release.notify_one();
            let held = tokio::time::timeout(Duration::from_secs(2), holder)
                .await
                .map_err(Error::internal)?
                .map_err(Error::internal)?;
            outcome?;
            held
        })
    })
    .await
}

#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn old_cached_guard_read_preserves_already_committed_other_identity_proof() -> Result<()> {
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let (old, _) = cached_pending_race_guard(&fixture).await?;
            let (successor, user, receipt) =
                different_successor_with_test_receipt(&fixture, old.session.clone()).await?;
            let expected = user.auth_identifier()?;
            assert_ne!(expected, fixture.identifier);
            successor
                .login_with_factor_receipt(user, false, receipt)
                .await?;
            let proof = proof_value_for_test(&successor)?;
            assert!(!proof.is_null());
            assert!(old.user().await?.is_none());
            assert_successor_identity_and_proof(&successor, &expected, &proof).await
        })
    })
    .await
}

#[test]
fn proof_recaller_reserved_versions_and_bad_payloads_never_downgrade() -> Result<()> {
    for extension in [
        "~or-factor-v1~!!!",
        "~or-factor-v2~e30",
        "~or-factor-v1~e30",
        "~or-factor-v1~bnVsbA",
        "~or-factor-v1~",
    ] {
        assert!(Recaller::parse(&format!("42|token|hash|{extension}")).is_none());
    }
    let over = "A".repeat(RECALLER_PROOF_LIMIT * 2);
    assert!(Recaller::parse(&format!("42|token|hash|{RECALLER_PROOF_PREFIX}{over}")).is_none());
    assert!(
        Recaller::parse("42|token|hash|ignored|also-ignored")
            .ok_or_else(state_error)?
            .factor_proof
            .is_none()
    );
    Ok(())
}
#[cfg(all(feature = "auth-fortify", feature = "db-sqlite"))]
#[tokio::test]
async fn recalled_proof_is_not_visible_to_event_until_commit_and_cancel_cleans_it() -> Result<()> {
    remember_authority_case(|fixture| {
        Box::pin(async move {
            let RememberAuthorityFixture {
                authority,
                encrypter,
                identifier,
                user,
                provider,
                ..
            } = fixture;
            let before = authority.load().await?;
            let enabled = authority
                .replace(
                    &before,
                    &crate::auth::fortify::TwoFactorState {
                        secret: Some(encrypter.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                        recovery_codes: Some(encrypter.encrypt_serialized_string(
                            br#"["one","two","three","four","five","six","seven","eight"]"#,
                        )?),
                        confirmed_at: Some(chrono::Utc::now()),
                    },
                    crate::auth::fortify::FactorAuthorityMutation::Enable,
                )
                .await?;
            let original = proof_remember_guard(provider.clone(), None).await?;
            let receipt = FactorReceipt::issue(
                identifier,
                "web".into(),
                None,
                enabled.source,
                enabled.generation,
                super::super::FactorVerificationMethod::Totp,
                123,
            )?;
            original
                .login_with_factor_receipt(user, true, receipt)
                .await?;
            let cookie = queued_recaller(&original)?;
            let entered = Arc::new(tokio::sync::Notify::new());
            let notify = entered.clone();
            let mut events = crate::events::EventsServiceProvider::new();
            events.listen::<Login, _>(move |_, _| {
                let notify = notify.clone();
                Box::pin(async move {
                    notify.notify_one();
                    std::future::pending::<()>().await;
                    Ok(())
                })
            });
            let restored = Arc::new(
                failure_guard(provider, Some(test_dispatcher(events).await?))
                    .await?
                    .with_decrypted_recaller(Some(cookie)),
            );
            let capture = restored.clone();
            let worker = tokio::spawn(async move { capture.user().await });
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .map_err(Error::internal)?;
            assert!(
                restored
                    .session
                    .get_json(&session_factor_key(&restored.session_name()))?
                    .is_some()
            );
            assert!(restored.session_factor_proof().await?.is_none());
            worker.abort();
            assert!(worker.await.is_err());
            assert!(restored.user().await?.is_none());
            assert!(
                restored
                    .session
                    .get_json(&session_factor_key(&restored.session_name()))?
                    .is_none()
            );
            assert!(
                restored
                    .session
                    .get_json(&restored.session_name())?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
}
#[test]
fn recallers_support_legacy_serialized_strings_extra_segments_and_encoded_identifiers() -> Result<()>
{
    for identifier in ["tenant|user", "~or1~reserved", "用户|42", "normal-id"] {
        let encoded = recaller_identifier(json!(identifier))?;
        let raw = format!("{encoded}|token|hash|ignored");
        let parsed = Recaller::parse(&raw).ok_or_else(state_error)?;
        assert_eq!(parsed.identifier, json!(identifier));
        assert_eq!(parsed.token, "token");
        assert_eq!(parsed.hash, "hash");
        let serialized = format!("s:{}:\"{}\";", raw.len(), raw);
        assert_eq!(
            Recaller::parse(&serialized)
                .ok_or_else(state_error)?
                .identifier,
            json!(identifier)
        );
    }
    assert!(php_serialized_string("s:2:\"long\";").is_none());
    assert!(php_serialized_string("O:8:\"stdClass\":0:{}").is_none());
    assert!(Recaller::parse("~or1~s!!!|token|hash").is_none());
    assert!(Recaller::parse(&"x".repeat(RECALLER_LIMIT + 1)).is_none());
    Ok(())
}
#[tokio::test]
async fn all_direct_user_entry_points_reject_missing_id_without_email_requirement() -> Result<()> {
    for identifier in [Value::Null, json!({"id":42}), json!([42])] {
        let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
        let user: AuthUser = Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
            [("id".into(), identifier)],
        )));
        assert!(guard.set_user(user.clone()).await.is_err());
        assert!(guard.login(user, false).await.is_err());
        assert_failed_closed(&guard).await?;
    }
    for identifier in [
        json!(42),
        json!("f4733e92-bc37-4196-80a8-92b0dd5736d3"),
        json!("tenant|user"),
    ] {
        let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
        let user: AuthUser = Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
            [("id".into(), identifier.clone())],
        )));
        guard.login(user, true).await?;
        assert_eq!(guard.id().await?, Some(identifier));
    }
    Ok(())
}
#[tokio::test]
async fn remember_requires_trusted_cookie_protection_but_plain_session_does_not() -> Result<()> {
    let guard = failure_guard(Arc::new(EmptyProvider(false)), None)
        .await?
        .with_remember_cookie_protected(false);
    assert!(guard.login(account(), true).await.is_err());
    assert_failed_closed(&guard).await?;
    guard.login(account(), false).await?;
    assert_eq!(guard.id().await?, Some(json!(42)));
    let mut recall = failure_guard(Arc::new(EmptyProvider(false)), None)
        .await?
        .with_remember_cookie_protected(false);
    recall.recaller = Some("42|token|hash".into());
    assert!(recall.user().await.is_err());
    Ok(())
}
fn identified_account(identifier: i64) -> AuthUser {
    Arc::new(super::super::GenericUser::new(serde_json::Map::from_iter(
        [("id".into(), json!(identifier))],
    )))
}
async fn test_dispatcher(provider: crate::events::EventsServiceProvider) -> Result<Dispatcher> {
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    Ok((*runtime.container().resolve::<Dispatcher>().await?).clone())
}
#[tokio::test]
async fn cancelled_old_login_does_not_clear_newer_successful_login() -> Result<()> {
    let entered = Arc::new(tokio::sync::Notify::new());
    let capture = entered.clone();
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Login, _>(move |event, _| {
        let capture = capture.clone();
        let identifier = event.user.auth_identifier();
        Box::pin(async move {
            if identifier? == json!(42) {
                capture.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        failure_guard(
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(provider).await?),
        )
        .await?,
    );
    let original = guard.clone();
    let worker = tokio::spawn(async move { original.login(identified_account(42), false).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    guard.login(identified_account(7), false).await?;
    worker.abort();
    assert!(worker.await.is_err());
    assert_eq!(guard.id().await?, Some(json!(7)));
    assert_eq!(
        guard.session.get_json(&guard.session_name())?,
        Some(json!(7))
    );
    Ok(())
}
#[tokio::test]
async fn listener_replacement_login_invalidates_outer_login_without_overwriting_new_user()
-> Result<()> {
    let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
    let capture = slot.clone();
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Login, _>(move |event, _| {
        let capture = capture.clone();
        let identifier = event.user.auth_identifier();
        Box::pin(async move {
            if identifier? == json!(42) {
                let guard = capture
                    .read()
                    .map_err(|_| state_error())?
                    .upgrade()
                    .ok_or_else(state_error)?;
                guard.login(identified_account(7), false).await?;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        failure_guard(
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(provider).await?),
        )
        .await?,
    );
    *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
    let outer = tokio::time::timeout(
        Duration::from_secs(2),
        guard.login(identified_account(42), false),
    )
    .await
    .map_err(Error::internal)?;
    assert!(outer.is_err());
    assert_eq!(guard.id().await?, Some(json!(7)));
    assert_eq!(
        guard.session.get_json(&guard.session_name())?,
        Some(json!(7))
    );
    Ok(())
}
#[tokio::test]
async fn login_listener_logout_invalidates_outer_login_without_resurrection() -> Result<()> {
    let slot = Arc::new(RwLock::new(std::sync::Weak::<SessionGuard>::new()));
    let capture = slot.clone();
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen::<Login, _>(move |_, _| {
        let capture = capture.clone();
        Box::pin(async move {
            let guard = capture
                .read()
                .map_err(|_| state_error())?
                .upgrade()
                .ok_or_else(state_error)?;
            guard.logout().await
        })
    });
    let guard = Arc::new(
        failure_guard(
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(provider).await?),
        )
        .await?,
    );
    *slot.write().map_err(|_| state_error())? = Arc::downgrade(&guard);
    let outer = tokio::time::timeout(
        Duration::from_secs(2),
        guard.login(identified_account(42), false),
    )
    .await
    .map_err(Error::internal)?;
    assert!(outer.is_err());
    assert!(guard.user().await?.is_none());
    assert!(guard.session.get_json(&guard.session_name())?.is_none());
    Ok(())
}
#[tokio::test]
async fn forged_plain_or_except_cookie_session_cannot_authenticate() -> Result<()> {
    use crate::session::{CookieSessionStore, SessionId, SessionRequestContext};
    let id = SessionId::parse(&"A".repeat(40))?;
    for (encrypted, except) in [
        (false, vec![]),
        (true, vec!["openrust_session".to_owned()]),
        (true, vec![id.as_str().to_owned()]),
    ] {
        let cookies = CookieJar::new(crate::http::CookieFactory::default());
        let session = Session::new(
            Some(id.clone()),
            Arc::new(CookieSessionStore::new(5, false)?),
            None,
        )?;
        let guard = SessionGuard::new(
            "web".into(),
            Arc::new(EmptyProvider(true)),
            session,
            cookies.clone(),
            None,
            None,
            None,
        )
        .with_remember_cookie_protected(true);
        let data =
            serde_json::to_string(&json!({guard.session_name(): 42})).map_err(Error::internal)?;
        let payload = serde_json::to_string(&json!({"data": data, "expires": time::OffsetDateTime::now_utc().unix_timestamp() + 300})).map_err(Error::internal)?;
        let context = SessionRequestContext::from_request(
            json!({id.as_str(): payload}),
            cookies,
            Some(crate::http::cookie::CookieProtection { encrypted, except }),
        )?;
        guard.session.set_request_on_handler(context).await?;
        guard.session.start().await?;
        // 确认恶意账户键确实已被载入，不能仅靠空 Session 得到负例。
        assert_eq!(
            guard.session.get_json(&guard.session_name())?,
            Some(json!(42))
        );
        assert!(guard.user().await.is_err());
        assert!(guard.login(account(), false).await.is_err());
        // 可信调用方单次认证不依赖持久 Session 的安全证明。
        guard.set_user(account()).await?;
        assert_eq!(guard.id().await?, Some(json!(42)));
    }
    Ok(())
}
#[tokio::test]
async fn cached_persistent_user_rechecks_handler_and_cookie_name() -> Result<()> {
    let guard = failure_guard(Arc::new(EmptyProvider(false)), None).await?;
    guard.login(account(), false).await?;
    guard
        .session
        .set_handler(Arc::new(crate::session::CookieSessionStore::new(5, false)?))?;
    assert!(guard.user().await.is_err());
    // 即使应用不再读取 Guard，自动保存也必须拒绝降级后的持久化。
    assert!(guard.session.save().await.is_err());

    let cookies = CookieJar::new(crate::http::CookieFactory::default());
    let session = Session::new(
        None,
        Arc::new(crate::session::CookieSessionStore::new(5, false)?),
        None,
    )?;
    let context = crate::session::SessionRequestContext::from_request(
        json!({}),
        cookies.clone(),
        Some(crate::http::cookie::CookieProtection {
            encrypted: true,
            except: vec!["later_plain_name".into()],
        }),
    )?;
    session.set_request_on_handler(context).await?;
    session.start().await?;
    let guard = SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(false)),
        session,
        cookies,
        None,
        None,
        None,
    );
    guard.login(account(), false).await?;
    guard.session.set_name("later_plain_name")?;
    assert!(guard.user().await.is_err());
    assert!(guard.session.save().await.is_err());
    Ok(())
}
#[derive(Default)]
struct TokenProvider {
    calls: std::sync::atomic::AtomicUsize,
    pending: std::sync::atomic::AtomicBool,
    fail: std::sync::atomic::AtomicBool,
    stored: std::sync::Mutex<Option<String>>,
    entered: tokio::sync::Notify,
    controlled: std::sync::atomic::AtomicBool,
    release: tokio::sync::Notify,
}
impl UserProvider for TokenProvider {
    fn retrieve_by_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        user: &'a dyn super::super::Authenticatable,
        token: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            assert_eq!(user.remember_token()?.as_deref(), Some(token));
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            if self.controlled.load(Ordering::SeqCst) {
                self.release.notified().await;
            }
            if self.pending.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::custom("token_write_failed", "测试 token 持久化失败"));
            }
            *self.stored.lock().map_err(|_| state_error())? = Some(token.to_owned());
            Ok(())
        })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn pending_provider_token_cannot_be_used_by_another_guards_remember_cookie() -> Result<()> {
    use std::sync::atomic::Ordering;
    let provider_a = Arc::new(TokenProvider::default());
    provider_a.controlled.store(true, Ordering::SeqCst);
    provider_a.fail.store(true, Ordering::SeqCst);
    let guard_a = Arc::new(failure_guard(provider_a.clone(), None).await?);
    let provider_b = Arc::new(TokenProvider::default());
    let guard_b = failure_guard(provider_b.clone(), None).await?;
    let user = account();
    let task_guard = guard_a.clone();
    let task_user = user.clone();
    let pending = tokio::spawn(async move { task_guard.login(task_user, true).await });
    tokio::time::timeout(Duration::from_secs(2), provider_a.entered.notified())
        .await
        .map_err(Error::internal)?;
    assert!(
        user.remember_token()?
            .is_some_and(|token| !token.is_empty())
    );
    assert!(
        provider_a
            .stored
            .lock()
            .map_err(|_| state_error())?
            .is_none()
    );
    let result = tokio::time::timeout(Duration::from_secs(2), guard_b.login(user.clone(), true))
        .await
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.status().as_u16() == 409));
    assert_eq!(provider_b.calls.load(Ordering::SeqCst), 0);
    assert!(
        provider_b
            .stored
            .lock()
            .map_err(|_| state_error())?
            .is_none()
    );
    assert_failed_closed(&guard_b).await?;
    // A仅在B已明确拒绝后返回已知持久化错误，排除未知数据库完成的解释。
    provider_a.release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "token_write_failed"));
    assert_eq!(provider_a.calls.load(Ordering::SeqCst), 1);
    assert!(
        provider_a
            .stored
            .lock()
            .map_err(|_| state_error())?
            .is_none()
    );
    assert!(user.remember_token()?.is_none());
    assert_failed_closed(&guard_a).await?;
    assert_failed_closed(&guard_b).await?;
    Ok(())
}

struct CookieClearingIdentity {
    user: super::super::GenericUser,
    replacement: Option<String>,
    cookie_reads: std::sync::atomic::AtomicUsize,
    cleared: std::sync::atomic::AtomicBool,
}
impl super::super::Authenticatable for CookieClearingIdentity {
    fn auth_identifier_name(&self) -> &str {
        self.user.auth_identifier_name()
    }
    fn auth_identifier(&self) -> Result<Value> {
        use std::sync::atomic::Ordering;
        // 第一次Cookie快照之后，合法身份getter同步修改同一账户；不依赖调度或sleep。
        if self.cookie_reads.load(Ordering::SeqCst) > 0
            && !self.cleared.swap(true, Ordering::SeqCst)
        {
            self.user.set_attribute(
                "remember_token",
                self.replacement.clone().map_or(Value::Null, Value::String),
            )?;
        }
        self.user.auth_identifier()
    }
    fn auth_password_name(&self) -> &str {
        self.user.auth_password_name()
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.user.auth_password()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.user.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        let token = self.user.remember_token_for_cookie()?;
        self.cookie_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(token)
    }
    fn set_remember_token(&self, value: Option<String>) -> BoxFuture<'_, Result<()>> {
        self.user.set_remember_token(value)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.user.begin_remember_token_update(expected, replacement)
    }
    fn remember_token_name(&self) -> &str {
        self.user.remember_token_name()
    }
}
#[tokio::test]
async fn final_cookie_token_rejects_invalid_values_set_by_the_identity_getter_after_first_read()
-> Result<()> {
    use super::super::Authenticatable;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    for replacement in [
        None,
        Some(String::new()),
        Some(" \t\n".into()),
        Some("0".into()),
        Some("changed|token".into()),
    ] {
        let provider = Arc::new(TokenProvider::default());
        let guard = failure_guard(provider.clone(), None).await?;
        let user = Arc::new(CookieClearingIdentity {
            user: super::super::GenericUser::new(serde_json::Map::from_iter([
                ("id".into(), json!(42)),
                ("password".into(), json!("hash")),
                ("remember_token".into(), json!("initial-committed-token")),
            ])),
            replacement: replacement.clone(),
            cookie_reads: AtomicUsize::new(0),
            cleared: AtomicBool::new(false),
        });
        assert_eq!(
            user.user.remember_token()?.as_deref(),
            Some("initial-committed-token")
        );
        assert!(matches!(guard.login(user.clone(), true).await, Err(error)
            if error.code() == "auth_remember_token_conflict" && error.status().as_u16() == 409));
        assert_eq!(user.cookie_reads.load(Ordering::SeqCst), 2);
        assert!(user.cleared.load(Ordering::SeqCst));
        assert_eq!(user.user.remember_token()?, replacement);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_failed_closed(&guard).await?;
    }
    Ok(())
}

#[tokio::test]
async fn recaller_queue_and_parser_share_the_exact_complete_payload_byte_limit() -> Result<()> {
    for with_proof in [false, true] {
        for extra in [0, 1] {
            let provider = Arc::new(TokenProvider::default());
            let guard = failure_guard(provider.clone(), None).await?;
            let identifier = recaller_identifier(json!(42))?;
            let hash = guard.hash_password_for_cookie(Some("hash"))?;
            let suffix = if with_proof {
                let proof = receipt(json!(42))?.into_proof();
                let json = serde_json::to_vec(&proof).map_err(Error::internal)?;
                format!("|{RECALLER_PROOF_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
            } else {
                String::new()
            };
            let overhead = format!("{identifier}||{hash}{suffix}");
            let token_len = RECALLER_LIMIT
                .checked_sub(overhead.len())
                .ok_or_else(state_error)?
                + extra;
            let token = "x".repeat(token_len);
            let expected = format!("{identifier}|{token}|{hash}{suffix}");
            assert_eq!(expected.len(), RECALLER_LIMIT + extra);
            if extra == 0 {
                let parsed = Recaller::parse(&expected).ok_or_else(state_error)?;
                assert_eq!(parsed.identifier, json!("42"));
                assert_eq!(parsed.token, token);
                assert_eq!(parsed.hash, hash);
                assert_eq!(parsed.factor_proof.is_some(), with_proof);
            } else {
                assert!(Recaller::parse(&expected).is_none());
            }
            let user = account();
            user.set_remember_token(Some(token)).await?;
            let result = if with_proof {
                guard
                    .login_with_factor_receipt(user, true, receipt(json!(42))?)
                    .await
            } else {
                guard.login(user, true).await
            };
            if extra == 0 {
                result?;
                let cookie = guard
                    .cookies
                    .queued(&guard.recaller_name(), None)?
                    .ok_or_else(state_error)?;
                assert_eq!(cookie.value(), Some(expected.as_str()));
                assert!(Recaller::parse(cookie.value().ok_or_else(state_error)?).is_some());
            } else {
                assert!(matches!(result, Err(error) if error.code() == "auth_recaller_too_large"));
                assert_failed_closed(&guard).await?;
            }
            assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        }
    }
    Ok(())
}

#[tokio::test]
async fn failed_token_update_restores_user_before_retrying_same_account() -> Result<()> {
    use std::sync::atomic::Ordering;
    let provider = Arc::new(TokenProvider::default());
    provider.fail.store(true, Ordering::SeqCst);
    let guard = failure_guard(provider.clone(), None).await?;
    let user = account();
    assert!(guard.login(user.clone(), true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(user.remember_token()?.is_none());
    assert_failed_closed(&guard).await?;
    provider.fail.store(false, Ordering::SeqCst);
    guard.login(user.clone(), true).await?;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        user.remember_token()?,
        *provider.stored.lock().map_err(|_| state_error())?
    );
    Ok(())
}
#[tokio::test]
async fn cancelled_token_update_restores_user_before_retrying_same_account() -> Result<()> {
    use std::sync::atomic::Ordering;
    let provider = Arc::new(TokenProvider::default());
    provider.pending.store(true, Ordering::SeqCst);
    let guard = failure_guard(provider.clone(), None).await?;
    let user = account();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), guard.login(user.clone(), true))
            .await
            .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(user.remember_token()?.is_none());
    assert_failed_closed(&guard).await?;
    provider.pending.store(false, Ordering::SeqCst);
    guard.login(user.clone(), true).await?;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        user.remember_token()?,
        *provider.stored.lock().map_err(|_| state_error())?
    );
    Ok(())
}
#[tokio::test]
async fn failed_logout_token_rotation_restores_previous_user_token() -> Result<()> {
    use std::sync::atomic::Ordering;
    let provider = Arc::new(TokenProvider::default());
    let guard = failure_guard(provider.clone(), None).await?;
    let user = account();
    guard.login(user.clone(), true).await?;
    let previous = user.remember_token()?;
    provider.fail.store(true, Ordering::SeqCst);
    assert!(guard.logout().await.is_err());
    assert_eq!(user.remember_token()?, previous);
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn stale_token_cleanup_cannot_replace_newer_committed_token() -> Result<()> {
    let provider = Arc::new(TokenProvider::default());
    let guard = failure_guard(provider.clone(), None).await?;
    let user = account();
    user.set_remember_token(Some("old".into())).await?;
    let mut first = guard.operation().await;
    guard.start_mutation(&mut first)?;
    let stale = user
        .begin_remember_token_update(Some("old"), Some("pending".into()))
        .await?
        .ok_or_else(state_changed)?;
    drop(first);
    guard.set_user(user.clone()).await?;
    guard.logout().await?;
    let committed = user.remember_token()?;
    assert_ne!(committed.as_deref(), Some("pending"));
    assert_eq!(
        committed,
        *provider.stored.lock().map_err(|_| state_error())?
    );
    drop(stale);
    assert_eq!(user.remember_token()?, committed);
    guard.login(user.clone(), true).await?;
    assert_eq!(guard.id().await?, Some(json!(42)));
    Ok(())
}
#[tokio::test]
async fn token_receipt_cleanup_restores_raw_even_after_guard_epoch_changes() -> Result<()> {
    let provider = Arc::new(TokenProvider::default());
    let guard = failure_guard(provider.clone(), None).await?;
    let user = account();
    user.set_remember_token(Some("old".into())).await?;
    let mut first = guard.operation().await;
    guard.start_mutation(&mut first)?;
    let stale = user
        .begin_remember_token_update(Some("old"), Some("pending".into()))
        .await?
        .ok_or_else(state_changed)?;
    drop(first);
    guard.set_user(user.clone()).await?;
    assert_eq!(user.remember_token()?.as_deref(), Some("pending"));
    drop(stale);
    assert_eq!(user.remember_token()?.as_deref(), Some("old"));
    assert_eq!(guard.id().await?, Some(json!(42)));
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}
#[tokio::test]
async fn overlapping_token_receipt_cancellation_cannot_resurrect_an_aborted_candidate() -> Result<()>
{
    let user = account();
    let first = user
        .begin_remember_token_update(None, Some("first".into()))
        .await?
        .ok_or_else(state_changed)?;
    let second = user
        .begin_remember_token_update(Some("first"), Some("second".into()))
        .await?
        .ok_or_else(state_changed)?;
    drop(first);
    assert_eq!(user.remember_token()?.as_deref(), Some("second"));
    drop(second);
    assert!(user.remember_token()?.is_none());
    // 同值的普通后继赋值也已经提交，不能因值相等被旧收据撤销。
    let first = user
        .begin_remember_token_update(None, Some("first".into()))
        .await?
        .ok_or_else(state_changed)?;
    let second = user
        .begin_remember_token_update(Some("first"), Some("second".into()))
        .await?
        .ok_or_else(state_changed)?;
    user.set_remember_token(Some("second".into())).await?;
    drop(first);
    drop(second);
    assert_eq!(user.remember_token()?.as_deref(), Some("second"));
    Ok(())
}
struct RestoreBarrierUser {
    user: super::super::GenericUser,
    install_conflict: bool,
    restore_entered: Arc<tokio::sync::Notify>,
    restore_gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}
impl super::super::Authenticatable for RestoreBarrierUser {
    fn auth_identifier_name(&self) -> &str {
        self.user.auth_identifier_name()
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.user.auth_identifier()
    }
    fn auth_password_name(&self) -> &str {
        self.user.auth_password_name()
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.user.auth_password()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.user.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.user.remember_token_for_cookie()
    }
    fn set_remember_token(&self, value: Option<String>) -> BoxFuture<'_, Result<()>> {
        self.user.set_remember_token(value)
    }
    fn remember_token_name(&self) -> &str {
        self.user.remember_token_name()
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        Box::pin(async move {
            if self.install_conflict && replacement.is_some() {
                return Ok(None);
            }
            Ok(self
                .user
                .begin_remember_token_update(expected, replacement)
                .await?
                .map(|inner| {
                    crate::auth::RememberTokenUpdate::new(RestoreBarrierMutation {
                        inner: Some(inner),
                        entered: Some(self.restore_entered.clone()),
                        gate: self.restore_gate.clone(),
                    })
                }))
        })
    }
}
struct RestoreBarrierMutation {
    inner: Option<crate::auth::RememberTokenUpdate>,
    entered: Option<Arc<tokio::sync::Notify>>,
    gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}
impl crate::auth::RememberTokenMutation for RestoreBarrierMutation {
    fn commit(&mut self) -> Result<()> {
        self.inner
            .take()
            .map_or(Ok(()), crate::auth::RememberTokenUpdate::commit)
    }
    fn rollback(&mut self) -> Result<bool> {
        // 在真实raw恢复之前暂停，让另一Guard先通过Provider提交后继token。
        if let Some(entered) = self.entered.take() {
            entered.notify_one();
            let (gate, signal) = self.gate.as_ref();
            let released = gate.lock().map_err(|_| state_error())?;
            let (released, timeout) = signal
                .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                .map_err(|_| state_error())?;
            if timeout.timed_out() && !*released {
                return Err(state_error());
            }
        }
        self.inner
            .as_mut()
            .map_or(Ok(true), crate::auth::RememberTokenUpdate::rollback)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_guard_restore_cannot_overwrite_other_guards_committed_token() -> Result<()> {
    use std::sync::atomic::Ordering;
    let shared = Arc::new(RestoreBarrierUser {
        install_conflict: false,
        user: super::super::GenericUser::new(serde_json::Map::from_iter([
            ("id".into(), json!(42)),
            ("password".into(), json!("hash")),
        ])),
        restore_entered: Arc::new(tokio::sync::Notify::new()),
        restore_gate: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
    });
    let user: AuthUser = shared.clone();
    let provider_a = Arc::new(TokenProvider::default());
    provider_a.pending.store(true, Ordering::SeqCst);
    let guard_a = Arc::new(failure_guard(provider_a.clone(), None).await?);
    let provider_b = Arc::new(TokenProvider::default());
    let guard_b = failure_guard(provider_b.clone(), None).await?;
    let worker_guard = guard_a.clone();
    let worker_user = user.clone();
    let worker = tokio::spawn(async move { worker_guard.login(worker_user, true).await });
    tokio::time::timeout(Duration::from_secs(2), provider_a.entered.notified())
        .await
        .map_err(Error::internal)?;
    worker.abort();
    tokio::time::timeout(Duration::from_secs(2), shared.restore_entered.notified())
        .await
        .map_err(Error::internal)?;
    guard_b.set_user(user.clone()).await?;
    guard_b.logout().await?;
    let committed = user.remember_token()?;
    assert_eq!(
        committed,
        *provider_b.stored.lock().map_err(|_| state_error())?
    );
    guard_b.login(user.clone(), true).await?;
    {
        let (gate, signal) = shared.restore_gate.as_ref();
        *gate.lock().map_err(|_| state_error())? = true;
        signal.notify_all();
    }
    assert!(worker.await.is_err());
    assert_eq!(user.remember_token()?, committed);
    assert_eq!(guard_b.id().await?, Some(json!(42)));
    assert_eq!(
        guard_b.session.get_json(&guard_b.session_name())?,
        Some(json!(42))
    );
    assert_failed_closed(&guard_a).await?;
    Ok(())
}
#[tokio::test]
async fn token_install_conflict_fails_before_provider_write() -> Result<()> {
    use std::sync::atomic::Ordering;
    let provider = Arc::new(TokenProvider::default());
    let guard = failure_guard(provider.clone(), None).await?;
    let user: AuthUser = Arc::new(RestoreBarrierUser {
        user: super::super::GenericUser::new(serde_json::Map::from_iter([(
            "id".into(),
            json!(42),
        )])),
        install_conflict: true,
        restore_entered: Arc::new(tokio::sync::Notify::new()),
        restore_gate: Arc::new((std::sync::Mutex::new(true), std::sync::Condvar::new())),
    });
    assert!(matches!(guard.login(user.clone(), true).await,
        Err(error) if error.code() == "auth_remember_token_conflict" && error.status().as_u16() == 409));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(user.remember_token()?.is_none());
    assert_failed_closed(&guard).await
}
#[derive(Default)]
struct RegenerationStore {
    memory: crate::session::MemorySessionStore,
    resets: std::sync::atomic::AtomicUsize,
    fail_second: bool,
}
impl SessionStore for RegenerationStore {
    fn reset_exists(&self) -> Result<()> {
        let count = self
            .resets
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if self.fail_second && count == 2 {
            return Err(Error::custom(
                "extra_regeneration_failed",
                "额外 Session 轮换失败",
            ));
        }
        Ok(())
    }
    fn load<'a>(
        &'a self,
        id: &'a crate::session::SessionId,
        max_bytes: usize,
        expiry: time::OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<crate::session::SessionRecord>>> {
        self.memory.load(id, max_bytes, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a crate::session::SessionRecord,
        mode: crate::session::SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<crate::session::SessionWriteOutcome>> {
        self.memory.write(record, mode, max_bytes)
    }
    fn delete<'a>(&'a self, id: &'a crate::session::SessionId) -> BoxFuture<'a, Result<()>> {
        self.memory.delete(id)
    }
}
async fn regeneration_guard(
    store: Arc<RegenerationStore>,
    provider: Arc<dyn UserProvider>,
    events: Option<Dispatcher>,
) -> Result<SessionGuard> {
    let session = Session::new(None, store, None)?;
    session.start().await?;
    session.put_typed("unrelated", "retained")?;
    Ok(SessionGuard::new(
        "web".into(),
        provider,
        session,
        CookieJar::new(crate::http::cookie::CookieFactory::default()),
        None,
        events,
        None,
    )
    .with_remember_cookie_protected(true))
}
#[tokio::test]
async fn combined_login_revokes_identity_if_only_the_extra_regeneration_fails() -> Result<()> {
    use std::sync::atomic::Ordering;
    let store = Arc::new(RegenerationStore {
        fail_second: true,
        ..RegenerationStore::default()
    });
    let provider = Arc::new(TokenProvider::default());
    let guard = regeneration_guard(store.clone(), provider.clone(), None).await?;
    let error = guard
        .login_with_session_regeneration(account(), true)
        .await
        .err()
        .ok_or_else(state_error)?;
    assert_eq!(error.code(), "extra_regeneration_failed");
    assert_eq!(store.resets.load(Ordering::SeqCst), 2);
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "已发出的记住我持久化不冒充已回滚"
    );
    assert_failed_closed(&guard).await
}
struct CredentialProvider {
    account: AuthUser,
    tokens: TokenProvider,
    retrieved: std::sync::atomic::AtomicUsize,
    validated: std::sync::atomic::AtomicUsize,
    rehashed: std::sync::atomic::AtomicUsize,
}
impl CredentialProvider {
    fn new() -> Self {
        Self {
            account: account(),
            tokens: TokenProvider::default(),
            retrieved: std::sync::atomic::AtomicUsize::new(0),
            validated: std::sync::atomic::AtomicUsize::new(0),
            rehashed: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}
impl UserProvider for CredentialProvider {
    fn retrieve_by_id<'a>(&'a self, id: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok((id == &json!(42)).then(|| self.account.clone())) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        user: &'a dyn super::super::Authenticatable,
        token: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        self.tokens.update_remember_token(user, token)
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async {
            self.retrieved
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Some(self.account.clone()))
        })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.validated
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(credentials.get("password") == Some(&json!("correct")))
        })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn super::super::Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            self.rehashed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    }
}
#[tokio::test]
async fn combined_attempt_preserves_real_provider_rehash_events_and_ordinary_attempt_behavior()
-> Result<()> {
    use std::sync::atomic::Ordering;
    for extra in [false, true] {
        let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut events = crate::events::EventsServiceProvider::new();
        macro_rules! observe {
            ($event:ty, $label:literal) => {{
                let trace = trace.clone();
                events.listen::<$event, _>(move |_, _| {
                    let trace = trace.clone();
                    Box::pin(async move {
                        trace.lock().map_err(|_| state_error())?.push($label);
                        Ok(())
                    })
                });
            }};
        }
        observe!(Attempting, "attempting");
        observe!(Validated, "validated");
        observe!(Login, "login");
        observe!(Authenticated, "authenticated");
        let store = Arc::new(RegenerationStore {
            fail_second: true,
            ..RegenerationStore::default()
        });
        let provider = Arc::new(CredentialProvider::new());
        let guard = regeneration_guard(
            store.clone(),
            provider.clone(),
            Some(test_dispatcher(events).await?),
        )
        .await?;
        let credentials = Credentials::new(serde_json::Map::from_iter([(
            "password".into(),
            json!("correct"),
        )]));
        if extra {
            assert!(
                guard
                    .attempt_with_session_regeneration(&credentials, true)
                    .await
                    .is_err()
            );
            assert_eq!(store.resets.load(Ordering::SeqCst), 2);
            assert_failed_closed(&guard).await?;
        } else {
            assert!(guard.attempt(&credentials, true).await?);
            assert_eq!(store.resets.load(Ordering::SeqCst), 1);
            assert_eq!(guard.id().await?, Some(json!(42)));
        }
        assert_eq!(provider.retrieved.load(Ordering::SeqCst), 1);
        assert_eq!(provider.validated.load(Ordering::SeqCst), 1);
        assert_eq!(provider.rehashed.load(Ordering::SeqCst), 1);
        assert_eq!(
            *trace.lock().map_err(|_| state_error())?,
            ["attempting", "validated", "login", "authenticated"]
        );
    }
    Ok(())
}
#[tokio::test]
async fn combined_login_success_finishes_two_regenerations_before_returning() -> Result<()> {
    use std::sync::atomic::Ordering;
    let store = Arc::new(RegenerationStore::default());
    let guard = regeneration_guard(store.clone(), Arc::new(EmptyProvider(false)), None).await?;
    let before = guard.session.id()?;
    guard
        .login_with_session_regeneration(account(), true)
        .await?;
    assert_eq!(store.resets.load(Ordering::SeqCst), 2);
    assert_ne!(before, guard.session.id()?);
    assert_eq!(guard.id().await?, Some(json!(42)));
    assert!(guard.cookies.has_queued(&guard.recaller_name(), None)?);
    Ok(())
}
#[tokio::test]
async fn cancelling_combined_login_before_extra_regeneration_never_commits_identity() -> Result<()>
{
    use std::sync::atomic::Ordering;
    let entered = Arc::new(tokio::sync::Notify::new());
    let capture = entered.clone();
    let mut events = crate::events::EventsServiceProvider::new();
    events.listen::<Authenticated, _>(move |_, _| {
        let capture = capture.clone();
        Box::pin(async move {
            capture.notify_one();
            std::future::pending::<Result<()>>().await
        })
    });
    let store = Arc::new(RegenerationStore::default());
    let guard = Arc::new(
        regeneration_guard(
            store.clone(),
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(events).await?),
        )
        .await?,
    );
    let worker_guard = guard.clone();
    let worker = tokio::spawn(async move {
        worker_guard
            .login_with_session_regeneration(account(), true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    assert!(guard.has_user()?);
    assert!(guard.cookies.has_queued(&guard.recaller_name(), None)?);
    worker.abort();
    assert!(worker.await.is_err());
    assert_eq!(
        store.resets.load(Ordering::SeqCst),
        1,
        "取消发生在额外轮换前最后一个异步事件边界"
    );
    assert_failed_closed(&guard).await
}
#[tokio::test]
async fn cancelling_stale_combined_login_keeps_newer_combined_login_and_cookie() -> Result<()> {
    let entered = Arc::new(tokio::sync::Notify::new());
    let capture = entered.clone();
    let mut events = crate::events::EventsServiceProvider::new();
    events.listen::<Authenticated, _>(move |event, _| {
        let capture = capture.clone();
        let id = event.user.auth_identifier();
        Box::pin(async move {
            if id? == json!(42) {
                capture.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        failure_guard(
            Arc::new(EmptyProvider(false)),
            Some(test_dispatcher(events).await?),
        )
        .await?,
    );
    let first = guard.clone();
    let worker =
        tokio::spawn(async move { first.login_with_session_regeneration(account(), true).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    guard
        .login_with_session_regeneration(identified_account(7), true)
        .await?;
    worker.abort();
    assert!(worker.await.is_err());
    assert_eq!(guard.id().await?, Some(json!(7)));
    assert_eq!(
        guard.session.get_json(&guard.session_name())?,
        Some(json!(7))
    );
    let cookie = guard
        .cookies
        .queued(&guard.recaller_name(), None)?
        .ok_or_else(state_error)?;
    assert!(cookie.value().is_some_and(|value| value.starts_with("7|")));
    Ok(())
}
struct UnimplementedCombinedGuard(Arc<SessionGuard>);
impl Guard for UnimplementedCombinedGuard {
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
        self.0.user()
    }
    fn validate<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        self.0.validate(credentials)
    }
    fn has_user(&self) -> Result<bool> {
        self.0.has_user()
    }
    fn set_user(&self, user: AuthUser) -> BoxFuture<'_, Result<()>> {
        self.0.set_user(user)
    }
}
impl StatefulGuard for UnimplementedCombinedGuard {
    fn provider(&self) -> Arc<dyn UserProvider> {
        self.0.provider()
    }
    fn attempt<'a>(
        &'a self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> BoxFuture<'a, Result<bool>> {
        self.0.attempt(credentials, remember)
    }
    fn once<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        self.0.once(credentials)
    }
    fn login(&self, user: AuthUser, remember: bool) -> BoxFuture<'_, Result<()>> {
        self.0.login(user, remember)
    }
    fn login_using_id<'a>(
        &'a self,
        identifier: &'a Value,
        remember: bool,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        self.0.login_using_id(identifier, remember)
    }
    fn once_using_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        self.0.once_using_id(identifier)
    }
    fn via_remember(&self) -> Result<bool> {
        self.0.via_remember()
    }
    fn logout(&self) -> BoxFuture<'_, Result<()>> {
        self.0.logout()
    }
}
#[tokio::test]
async fn custom_guard_without_combined_capability_fails_before_any_login_or_provider_mutation()
-> Result<()> {
    use std::sync::atomic::Ordering;
    let provider = Arc::new(CredentialProvider::new());
    let inner = Arc::new(failure_guard(provider.clone(), None).await?);
    let guard = UnimplementedCombinedGuard(inner.clone());
    assert!(!guard.supports_session_regeneration_login());
    assert!(inner.supports_session_regeneration_login());
    let credentials = Credentials::new(serde_json::Map::from_iter([(
        "password".into(),
        json!("correct"),
    )]));
    let first = guard
        .attempt_with_session_regeneration(&credentials, true)
        .await
        .err()
        .ok_or_else(state_error)?;
    let second = guard
        .login_with_session_regeneration(account(), true)
        .await
        .err()
        .ok_or_else(state_error)?;
    for error in [first, second] {
        assert_eq!(error.code(), "auth_guard_session_regeneration_unsupported");
        assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    }
    assert_eq!(provider.retrieved.load(Ordering::SeqCst), 0);
    assert_eq!(provider.tokens.calls.load(Ordering::SeqCst), 0);
    assert_failed_closed(&inner).await
}
#[tokio::test]
async fn combined_attempt_rejection_preserves_timebox_and_never_regenerates() -> Result<()> {
    use std::sync::atomic::Ordering;
    for extra in [false, true] {
        let store = Arc::new(RegenerationStore::default());
        let provider = Arc::new(CredentialProvider::new());
        let guard = regeneration_guard(store.clone(), provider.clone(), None).await?;
        let credentials = Credentials::new(serde_json::Map::from_iter([(
            "password".into(),
            json!("wrong"),
        )]));
        let start = tokio::time::Instant::now();
        let result = if extra {
            guard
                .attempt_with_session_regeneration(&credentials, true)
                .await?
        } else {
            guard.attempt(&credentials, true).await?
        };
        assert!(!result);
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert_eq!(store.resets.load(Ordering::SeqCst), 0);
        assert_eq!(provider.rehashed.load(Ordering::SeqCst), 0);
        assert_failed_closed(&guard).await?;
    }
    Ok(())
}
#[derive(Default)]
struct BackgroundSaveStore {
    memory: crate::session::MemorySessionStore,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    writes: std::sync::atomic::AtomicUsize,
    pause_delete: std::sync::atomic::AtomicBool,
    delete_entered: tokio::sync::Notify,
    delete_release: tokio::sync::Notify,
}
impl SessionStore for BackgroundSaveStore {
    fn load<'a>(
        &'a self,
        id: &'a crate::session::SessionId,
        max_bytes: usize,
        expiry: time::OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<crate::session::SessionRecord>>> {
        self.memory.load(id, max_bytes, expiry)
    }
    fn write<'a>(
        &'a self,
        record: &'a crate::session::SessionRecord,
        mode: crate::session::SessionWriteMode,
        max_bytes: usize,
    ) -> BoxFuture<'a, Result<crate::session::SessionWriteOutcome>> {
        Box::pin(async move {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            self.memory.write(record, mode, max_bytes).await
        })
    }
    fn delete<'a>(&'a self, id: &'a crate::session::SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.pause_delete.load(std::sync::atomic::Ordering::SeqCst) {
                self.delete_entered.notify_one();
                self.delete_release.notified().await;
            }
            self.memory.delete(id).await
        })
    }
}
#[tokio::test]
async fn authenticated_listener_background_save_cannot_snapshot_uncommitted_login() -> Result<()> {
    use std::sync::atomic::Ordering;
    let store = Arc::new(BackgroundSaveStore::default());
    let session = Session::new(None, store.clone(), None)?;
    session.start().await?;
    let mut events = crate::events::EventsServiceProvider::new();
    let capture = session.clone();
    let backend = store.clone();
    events.listen::<Authenticated, _>(move |_, _| {
        let session = capture.clone(); let backend = backend.clone();
        Box::pin(async move {
            let mut save = tokio::spawn(async move { session.save().await });
            tokio::select! {
                () = backend.entered.notified() => {
                    // 旧实现会到达真实持久写屏障；释放资源后让测试明确失败。
                    backend.release.notify_one(); let _finished = save.await;
                    Err(Error::custom("candidate_identity_leaked", "后台保存已取得未提交身份快照"))
                },
                result = &mut save => {
                    let error = result.map_err(Error::internal)?.err().ok_or_else(state_error)?;
                    assert_eq!(error.code(), "session_authentication_transition_in_progress"); Ok(())
                }
            }
        })
    });
    let guard = SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(false)),
        session,
        CookieJar::new(crate::http::cookie::CookieFactory::default()),
        None,
        Some(test_dispatcher(events).await?),
        None,
    )
    .with_remember_cookie_protected(true);
    guard
        .login_with_session_regeneration(account(), true)
        .await?;
    assert_eq!(store.writes.load(Ordering::SeqCst), 0);
    assert_eq!(guard.id().await?, Some(json!(42)));
    Ok(())
}
type PendingMigration = Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<Result<bool>>>>>;
struct BusyCancellationFixture {
    backend: Arc<BackgroundSaveStore>,
    session: Session,
    admin: SessionGuard,
    migration: PendingMigration,
    entered: Arc<tokio::sync::Notify>,
    guard: Arc<SessionGuard>,
}
async fn busy_cancellation_fixture() -> Result<BusyCancellationFixture> {
    use std::sync::atomic::Ordering;
    let backend = Arc::new(BackgroundSaveStore::default());
    let session = Session::new(None, backend.clone(), None)?;
    session.start().await?;
    session.put_typed("unrelated", "retained")?;
    let cookies = CookieJar::new(crate::http::cookie::CookieFactory::default());
    let admin = SessionGuard::new(
        "admin".into(),
        Arc::new(EmptyProvider(false)),
        session.clone(),
        cookies.clone(),
        None,
        None,
        None,
    );
    admin.login(identified_account(99), false).await?;
    let migration = Arc::new(std::sync::Mutex::new(None));
    let entered = Arc::new(tokio::sync::Notify::new());
    let mut events = crate::events::EventsServiceProvider::new();
    let (capture, store, handle, notified) = (
        session.clone(),
        backend.clone(),
        migration.clone(),
        entered.clone(),
    );
    events.listen::<Authenticated, _>(move |event, _| {
        let (session, store, handle, notified) = (
            capture.clone(),
            store.clone(),
            handle.clone(),
            notified.clone(),
        );
        let identifier = event.user.auth_identifier();
        Box::pin(async move {
            if identifier? == json!(42) {
                store.pause_delete.store(true, Ordering::SeqCst);
                *handle.lock().map_err(|_| state_error())? =
                    Some(tokio::spawn(async move { session.migrate(true).await }));
                store.delete_entered.notified().await;
                notified.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        SessionGuard::new(
            "web".into(),
            Arc::new(EmptyProvider(false)),
            session.clone(),
            cookies.clone(),
            None,
            Some(test_dispatcher(events).await?),
            None,
        )
        .with_remember_cookie_protected(true),
    );
    Ok(BusyCancellationFixture {
        backend,
        session,
        admin,
        migration,
        entered,
        guard,
    })
}
async fn assert_authentication_fence(session: &Session, cookies: &CookieJar) -> Result<()> {
    assert_eq!(
        session.save().await.err().ok_or_else(state_error)?.code(),
        "session_authentication_transition_failed"
    );
    assert_eq!(
        session
            .require_authenticated_storage()
            .err()
            .ok_or_else(state_error)?
            .code(),
        "session_authentication_transition_failed"
    );
    let fresh = SessionGuard::new(
        "web".into(),
        Arc::new(EmptyProvider(true)),
        session.clone(),
        cookies.clone(),
        None,
        None,
        None,
    );
    assert_eq!(
        fresh.user().await.err().ok_or_else(state_error)?.code(),
        "session_authentication_transition_failed"
    );
    Ok(())
}
#[tokio::test]
async fn cancelled_login_with_busy_session_fences_save_reads_and_explicit_retry_cleans_owned_keys()
-> Result<()> {
    use std::sync::atomic::Ordering;
    let BusyCancellationFixture {
        backend,
        session,
        admin,
        migration,
        entered,
        guard,
    } = busy_cancellation_fixture().await?;
    let worker_guard = guard.clone();
    let worker = tokio::spawn(async move {
        worker_guard
            .login_with_session_regeneration(account(), true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    worker.abort();
    assert!(worker.await.is_err());
    // I/O 门仍由真实 store.delete 屏障占用；普通属性清理此刻无法执行。
    assert_eq!(session.get_json(&guard.session_name())?, Some(json!(42)));
    assert_authentication_fence(&session, &guard.cookies).await?;
    assert!(
        guard
            .login_with_session_regeneration(identified_account(7), false)
            .await
            .is_err(),
        "I/O仍忙时显式retry不能清fence"
    );
    backend.pause_delete.store(false, Ordering::SeqCst);
    backend.delete_release.notify_one();
    let handle = migration
        .lock()
        .map_err(|_| state_error())?
        .take()
        .ok_or_else(state_error)?;
    handle.await.map_err(Error::internal)??;
    assert_eq!(
        session.save().await.err().ok_or_else(state_error)?.code(),
        "session_authentication_transition_failed"
    );
    guard
        .login_with_session_regeneration(identified_account(7), false)
        .await?;
    assert_eq!(guard.id().await?, Some(json!(7)));
    assert_eq!(admin.id().await?, Some(json!(99)));
    assert_eq!(session.get_json(&admin.session_name())?, Some(json!(99)));
    assert_eq!(session.get_json("unrelated")?, Some(json!("retained")));
    assert!(
        session.get_json("password_hash_web")?.is_none(),
        "新账户无密码时不得残留旧候选哈希"
    );
    assert!(!session.all_json()?.to_string().contains("lease_owner"));
    backend.release.notify_one();
    session.save().await?;
    let record = backend
        .memory
        .load(
            &session.id()?,
            crate::session::DEFAULT_MAX_BYTES,
            session.expiry_date()?,
        )
        .await?
        .ok_or_else(state_error)?;
    assert_eq!(
        crate::support::data::get(&record.data, &guard.session_name()),
        Some(json!(7))
    );
    assert_eq!(
        crate::support::data::get(&record.data, &admin.session_name()),
        Some(json!(99))
    );
    Ok(())
}
#[tokio::test]
async fn failed_later_attempt_does_not_strand_cancelled_candidate_lease() -> Result<()> {
    let entered = Arc::new(tokio::sync::Notify::new());
    let capture = entered.clone();
    let mut events = crate::events::EventsServiceProvider::new();
    events.listen::<Authenticated, _>(move |event, _| {
        let capture = capture.clone();
        let id = event.user.auth_identifier();
        Box::pin(async move {
            if id? == json!(42) {
                capture.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    });
    let guard = Arc::new(
        failure_guard(
            Arc::new(CredentialProvider::new()),
            Some(test_dispatcher(events).await?),
        )
        .await?,
    );
    let first = guard.clone();
    let worker = tokio::spawn(async move {
        first
            .login_with_session_regeneration(account(), false)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    let credentials = Credentials::new(serde_json::Map::from_iter([(
        "password".into(),
        json!("wrong"),
    )]));
    assert!(!guard.attempt(&credentials, false).await?);
    worker.abort();
    assert!(worker.await.is_err());
    assert!(!guard.has_user()?);
    assert_eq!(
        guard
            .session
            .save()
            .await
            .err()
            .ok_or_else(state_error)?
            .code(),
        "session_authentication_transition_failed"
    );
    guard
        .login_with_session_regeneration(identified_account(7), false)
        .await?;
    guard.session.save().await?;
    assert_eq!(guard.id().await?, Some(json!(7)));
    Ok(())
}
