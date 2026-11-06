//! 请求级缓存隔离、并发构造、递归与取消恢复的行为回归。
use super::*;
use crate::auth::guard::contracts::AuthUser;
use crate::auth::guard::contracts::Authenticatable;
use crate::auth::guard::contracts::Credentials;
use crate::auth::guard::contracts::GenericUser;
use crate::config::ConfigSources;
use crate::config::repository::ConfigRepository;
use crate::container::Container;
use crate::container::ContainerBuilder;
use crate::foundation::resources::ResourceOwner;
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct TestGuard(Mutex<Option<AuthUser>>);
impl Guard for TestGuard {
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
        Box::pin(async move { Ok(self.0.lock().map_err(|_| state_error())?.clone()) })
    }
    fn validate<'a>(&'a self, _: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn has_user(&self) -> Result<bool> {
        Ok(self.0.lock().map_err(|_| state_error())?.is_some())
    }
    fn set_user(&self, user: AuthUser) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            *self.0.lock().map_err(|_| state_error())? = Some(user);
            Ok(())
        })
    }
}

struct TestProvider;
impl UserProvider for TestProvider {
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
        _: &'a dyn Authenticatable,
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
        _: &'a dyn Authenticatable,
        _: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn container(
    drivers: AuthDrivers,
    default_provider: Option<&str>,
) -> Result<(Container, ResourceOwner)> {
    let sources = ConfigSources::from_ordered_entries([(
        "auth",
        json!({
            "defaults": {"guard": "web", "passwords": "accounts", "provider": default_provider},
            "guards": {
                "web": {"driver": "test", "provider": null},
                "admin": {"driver": "test", "provider": null},
                "broken": {"driver": "missing", "provider": null},
                "recursive": {"driver": "recursive", "provider": null}
            },
            "providers": {"accounts": {"driver": "test"}, "broken": {"driver": "missing"}}
        }),
    )])
    .map_err(Error::internal)?;
    let owner = ResourceOwner::new();
    let mut builder = ContainerBuilder::new();
    builder.context(ConfigRepository::new(sources), owner.registrar());
    builder.instance(Arc::new(drivers));
    builder.scoped::<AuthManager, _>(|resolver| {
        Box::pin(async move {
            let drivers = resolver.resolve::<AuthDrivers>().await?;
            Ok(Arc::new(AuthManager::new(
                resolver,
                drivers.as_ref().clone(),
            )))
        })
    });
    Ok((builder.build(), owner))
}

fn drivers(counter: Arc<AtomicUsize>) -> Result<AuthDrivers> {
    let drivers = AuthDrivers::default();
    drivers.extend("test", move |_| {
        let counter = Arc::clone(&counter);
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
        })
    })?;
    drivers.provider("test", |_, _| {
        Box::pin(async { Ok(Arc::new(TestProvider) as Arc<dyn UserProvider>) })
    })?;
    Ok(drivers)
}

#[tokio::test]
async fn concurrent_resolution_is_once_and_names_and_scopes_isolate_users() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let (container, _owner) = container(drivers(Arc::clone(&calls))?, None)?;
    let first = container.scope()?;
    let second = container.scope()?;
    let manager = first.resolve::<AuthManager>().await?;
    assert!(Arc::ptr_eq(
        &manager,
        &first.resolve::<AuthManager>().await?
    ));
    let other = second.resolve::<AuthManager>().await?;
    assert!(!Arc::ptr_eq(&manager, &other));
    let (a, b, c) = tokio::join!(
        manager.guard(None),
        manager.guard(Some("web")),
        manager.guard(Some(""))
    );
    let (a, b, c) = (a?, b?, c?);
    assert!(Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&b, &c));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    a.set_user(Arc::new(GenericUser::new(Map::from_iter([(
        "id".into(),
        json!("first-user"),
    )]))))
    .await?;
    assert_eq!(b.id().await?, Some(json!("first-user")));
    let admin = manager.guard(Some("admin")).await?;
    let other_web = other.guard(None).await?;
    assert!(admin.guest().await?);
    assert!(other_web.guest().await?);
    assert!(!Arc::ptr_eq(&a, &admin));
    assert!(!Arc::ptr_eq(&a, &other_web));
    manager.should_use("admin")?;
    assert_eq!(manager.default_driver()?, "admin");
    assert_eq!(other.default_driver()?, "web");
    assert!(Arc::ptr_eq(&admin, &manager.guard(None).await?));
    manager.forget_guards()?;
    let rebuilt = manager.guard(Some("web")).await?;
    assert!(!Arc::ptr_eq(&a, &rebuilt));
    assert!(rebuilt.guest().await?);
    assert_eq!(a.id().await?, Some(json!("first-user")));
    assert!(Arc::ptr_eq(&other_web, &other.guard(None).await?));
    Ok(())
}

async fn guard_error(manager: &AuthManager, name: &str) -> Result<String> {
    match manager.guard(Some(name)).await {
        Ok(_) => Err(Error::custom(
            "expected_guard_error",
            "无效Guard意外解析成功",
        )),
        Err(error) => {
            if matches!(
                error.code(),
                "auth_guard_undefined"
                    | "auth_guard_driver_undefined"
                    | "auth_user_provider_driver_undefined"
            ) {
                assert_eq!(
                    error.status(),
                    crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
                );
            }
            Ok(error.code().to_owned())
        }
    }
}

#[tokio::test]
async fn undefined_guard_and_driver_fail_while_missing_provider_returns_none() -> Result<()> {
    let (container, _owner) = container(drivers(Arc::new(AtomicUsize::new(0)))?, None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    assert_eq!(
        guard_error(&manager, "unknown").await?,
        "auth_guard_undefined"
    );
    assert_eq!(
        guard_error(&manager, "broken").await?,
        "auth_guard_driver_undefined"
    );
    assert!(manager.create_user_provider(None).await?.is_none());
    assert!(manager.create_user_provider(Some("")).await?.is_none());
    assert!(
        manager
            .create_user_provider(Some("unknown"))
            .await?
            .is_none()
    );
    assert!(
        manager
            .create_user_provider(Some("accounts"))
            .await?
            .is_some()
    );
    match manager.create_user_provider(Some("broken")).await {
        Ok(_) => {
            return Err(Error::custom(
                "expected_provider_error",
                "未注册Provider driver意外解析成功",
            ));
        }
        Err(error) => {
            assert_eq!(error.code(), "auth_user_provider_driver_undefined");
            assert_eq!(
                error.status(),
                crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
        }
    }
    let (with_default, _owner_default) = container_with_provider()?;
    let scope = with_default.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    assert!(manager.create_user_provider(None).await?.is_some());
    assert!(manager.create_user_provider(Some("")).await?.is_some());
    Ok(())
}

fn container_with_provider() -> Result<(Container, ResourceOwner)> {
    container(drivers(Arc::new(AtomicUsize::new(0)))?, Some("accounts"))
}

#[tokio::test]
async fn recursive_creator_returns_a_bounded_error_and_does_not_poison_later_resolution()
-> Result<()> {
    let drivers = drivers(Arc::new(AtomicUsize::new(0)))?;
    drivers.extend("recursive", |context| {
        Box::pin(async move {
            context
                .resolver
                .resolve::<AuthManager>()
                .await?
                .guard(Some("recursive"))
                .await
        })
    })?;
    let (container, _owner) = container(drivers.clone(), None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let code = tokio::time::timeout(Duration::from_secs(1), guard_error(&manager, "recursive"))
        .await
        .map_err(Error::internal)??;
    assert_eq!(code, "auth_guard_resolution_cycle");
    drivers.extend("recursive", |_| {
        Box::pin(async { Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>) })
    })?;
    assert!(manager.guard(Some("recursive")).await?.guest().await?);
    Ok(())
}

#[tokio::test]
async fn cancelling_first_construction_releases_the_cell_for_retry() -> Result<()> {
    let drivers = AuthDrivers::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    drivers.extend("test", {
        let calls = Arc::clone(&calls);
        let entered = Arc::clone(&entered);
        move |_| {
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
            })
        }
    })?;
    let (container, _owner) = container(drivers, None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    {
        let construction = manager.guard(None);
        tokio::pin!(construction);
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                () = entered.notified() => Ok(()),
                result = &mut construction => { result?; Err(Error::custom("expected_pending_guard", "首个构造意外完成")) }
            }
        }).await.map_err(Error::internal)??;
    }
    let rebuilt = tokio::time::timeout(Duration::from_secs(1), manager.guard(None))
        .await
        .map_err(Error::internal)??;
    assert!(rebuilt.guest().await?);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(Arc::ptr_eq(&rebuilt, &manager.guard(None).await?));
    Ok(())
}

#[tokio::test]
async fn custom_guard_resolves_provider_only_when_its_creator_explicitly_requests_it() -> Result<()>
{
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let drivers = drivers(Arc::new(AtomicUsize::new(0)))?;
    drivers.provider("test", {
        let calls = Arc::clone(&provider_calls);
        move |_, _| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(TestProvider) as Arc<dyn UserProvider>)
            })
        }
    })?;
    // broken是已配置的Provider名，但其missing driver尚未注册。
    let (container, _owner) = container(drivers.clone(), Some("broken"))?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    assert!(manager.guard(None).await?.guest().await?);
    assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        guard_error(&manager, "unknown").await?,
        "auth_guard_undefined"
    );
    assert_eq!(provider_calls.load(Ordering::SeqCst), 0);

    drivers.extend("test", |context| {
        Box::pin(async move {
            let provider = context.user_provider().await?;
            assert!(provider.is_some());
            Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
        })
    })?;
    manager.forget_guards()?;
    assert_eq!(
        guard_error(&manager, "web").await?,
        "auth_user_provider_driver_undefined"
    );
    assert_eq!(provider_calls.load(Ordering::SeqCst), 0);

    drivers.provider("missing", {
        let calls = Arc::clone(&provider_calls);
        move |_, _| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(TestProvider) as Arc<dyn UserProvider>)
            })
        }
    })?;
    // 失败没有缓存半成品；同名重试才执行现在可用的Provider工厂。
    assert!(manager.guard(Some("web")).await?.guest().await?);
    assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
    assert!(manager.guard(Some("web")).await?.guest().await?);
    assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn separately_spawned_creators_detect_wait_cycle_without_reentering_repeated_barrier()
-> Result<()> {
    let drivers = AuthDrivers::default();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let calls = Arc::new(AtomicUsize::new(0));
    drivers.extend("test", {
        let barrier = barrier.clone();
        let calls = calls.clone();
        move |context| {
            let barrier = barrier.clone();
            let calls = calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                // 每次调用均等待；检测环后不得让 waiter 接管并重进下一轮屏障。
                barrier.wait().await;
                let manager = context.resolver.resolve::<AuthManager>().await?;
                let other = if context.name == "web" {
                    "admin"
                } else {
                    "web"
                };
                // 同一 creator 内的两个 waiter 都必须保留环标记，直到最后一个退出。
                let (first, second) =
                    tokio::join!(manager.guard(Some(other)), manager.guard(Some(other)));
                let first = first?;
                second?;
                Ok(first)
            })
        }
    })?;
    let (container, _owner) = container(drivers.clone(), None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.clone();
    let second = manager.clone();
    let web = tokio::spawn(async move { guard_error(&first, "web").await });
    let admin = tokio::spawn(async move { guard_error(&second, "admin").await });
    let (web, admin) =
        tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(web, admin) })
            .await
            .map_err(Error::internal)?;
    assert_eq!(
        web.map_err(Error::internal)??,
        "auth_guard_resolution_cycle"
    );
    assert_eq!(
        admin.map_err(Error::internal)??,
        "auth_guard_resolution_cycle"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        manager
            .waits
            .lock()
            .map_err(|_| state_error())?
            .edges
            .is_empty()
    );
    drivers.extend("test", |_| {
        Box::pin(async { Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>) })
    })?;
    let (web, admin) = tokio::join!(manager.guard(Some("web")), manager.guard(Some("admin")));
    assert!(web?.guest().await?);
    assert!(admin?.guest().await?);
    Ok(())
}

#[tokio::test]
async fn joined_sibling_creators_have_independent_paths_and_share_one_target() -> Result<()> {
    let drivers = AuthDrivers::default();
    let calls = Arc::new(AtomicUsize::new(0));
    drivers.extend("test", {
        let calls = calls.clone();
        move |context| {
            let calls = calls.clone();
            Box::pin(async move {
                if context.name == "web" {
                    let manager = context.resolver.resolve::<AuthManager>().await?;
                    let (first, second) =
                        tokio::join!(manager.guard(Some("admin")), manager.guard(Some("admin")));
                    let (first, second) = (first?, second?);
                    assert!(Arc::ptr_eq(&first, &second));
                    return Ok(first);
                }
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
            })
        }
    })?;
    let (container, _owner) = container(drivers, None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let web = tokio::time::timeout(Duration::from_secs(2), manager.guard(Some("web")))
        .await
        .map_err(Error::internal)??;
    assert!(Arc::ptr_eq(&web, &manager.guard(Some("admin")).await?));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        manager
            .waits
            .lock()
            .map_err(|_| state_error())?
            .edges
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_dependency_wait_removes_edge_before_reverse_dependency_retry() -> Result<()> {
    let drivers = AuthDrivers::default();
    let entered = Arc::new(Notify::new());
    drivers.extend("test", {
        let entered = entered.clone();
        move |context| {
            let entered = entered.clone();
            Box::pin(async move {
                if context.name == "web" {
                    return context
                        .resolver
                        .resolve::<AuthManager>()
                        .await?
                        .guard(Some("admin"))
                        .await;
                }
                entered.notify_one();
                std::future::pending::<Result<Arc<dyn Guard>>>().await
            })
        }
    })?;
    let (container, _owner) = container(drivers.clone(), None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.clone();
    let worker = tokio::spawn(async move { first.guard(Some("web")).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    assert_eq!(
        manager.waits.lock().map_err(|_| state_error())?.edges.len(),
        1
    );
    worker.abort();
    assert!(worker.await.is_err());
    assert!(
        manager
            .waits
            .lock()
            .map_err(|_| state_error())?
            .edges
            .is_empty()
    );
    drivers.extend("test", |context| {
        Box::pin(async move {
            if context.name == "admin" {
                return context
                    .resolver
                    .resolve::<AuthManager>()
                    .await?
                    .guard(Some("web"))
                    .await;
            }
            Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
        })
    })?;
    let admin = tokio::time::timeout(Duration::from_secs(2), manager.guard(Some("admin")))
        .await
        .map_err(Error::internal)??;
    assert!(Arc::ptr_eq(&admin, &manager.guard(Some("web")).await?));
    Ok(())
}

#[tokio::test]
async fn cancelling_one_joined_waiter_keeps_other_reference_and_single_construction() -> Result<()>
{
    let drivers = AuthDrivers::default();
    let entered = Arc::new(Notify::new());
    let cancel = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    drivers.extend("test", {
        let (entered, cancel, cancelled, release, calls) = (entered.clone(), cancel.clone(), cancelled.clone(), release.clone(), calls.clone());
        move |context| {
            let (entered, cancel, cancelled, release, calls) = (entered.clone(), cancel.clone(), cancelled.clone(), release.clone(), calls.clone());
            Box::pin(async move {
                if context.name == "web" {
                    let manager = context.resolver.resolve::<AuthManager>().await?;
                    let (abandoned, remaining) = tokio::join!(async {
                        tokio::select! {
                            result = manager.guard(Some("admin")) => { result?; return Err(state_error()); },
                            () = cancel.notified() => {}
                        }
                        cancelled.notify_one();
                        Ok(())
                    }, manager.guard(Some("admin")));
                    abandoned?;
                    return remaining;
                }
                calls.fetch_add(1, Ordering::SeqCst);
                entered.notify_one();
                release.notified().await;
                Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
            })
        }
    })?;
    let (container, _owner) = container(drivers, None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.clone();
    let worker = tokio::spawn(async move { first.guard(Some("web")).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    let count = || -> Result<usize> {
        Ok(manager
            .waits
            .lock()
            .map_err(|_| state_error())?
            .edges
            .values()
            .flat_map(BTreeMap::values)
            .sum())
    };
    assert_eq!(count()?, 2);
    cancel.notify_one();
    tokio::time::timeout(Duration::from_secs(2), cancelled.notified())
        .await
        .map_err(Error::internal)?;
    assert_eq!(count()?, 1);
    // 被取消者原来拥有 initializer；存活 waiter 接管时会再调用工厂，绝不共享半成品。
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    release.notify_one();
    let guard = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)??;
    assert!(Arc::ptr_eq(&guard, &manager.guard(Some("admin")).await?));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        manager
            .waits
            .lock()
            .map_err(|_| state_error())?
            .edges
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn forgetting_guards_during_construction_distinguishes_same_name_cell_generations()
-> Result<()> {
    let drivers = AuthDrivers::default();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    drivers.extend("test", {
        let (entered, release, calls) = (entered.clone(), release.clone(), calls.clone());
        move |context| {
            let (entered, release, calls) = (entered.clone(), release.clone(), calls.clone());
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.notify_one();
                    release.notified().await;
                    return context
                        .resolver
                        .resolve::<AuthManager>()
                        .await?
                        .guard(Some("web"))
                        .await;
                }
                Ok(Arc::new(TestGuard::default()) as Arc<dyn Guard>)
            })
        }
    })?;
    let (container, _owner) = container(drivers, None)?;
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    let first = manager.clone();
    let worker = tokio::spawn(async move { first.guard(Some("web")).await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    manager.forget_guards()?;
    let current = manager.guard(Some("web")).await?;
    release.notify_one();
    let previous = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)??;
    assert!(Arc::ptr_eq(&previous, &current));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
