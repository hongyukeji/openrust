//! 用真实类型投影验证条件绑定、重入和并发发现。

use crate::config::Environment;
use crate::container::{
    ClassDescriptor, Container, ContainerBuilder, Resolver, ServiceKey,
    attributes::{Bind, BindWhen},
};
use crate::{Error, Result, class_projection};
use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

const LIMIT: Duration = Duration::from_secs(2);

trait View: Send + Sync {
    fn selected(&self) -> u8;
}

struct Concrete<const ID: u8>;

impl<const ID: u8> View for Concrete<ID> {
    fn selected(&self) -> u8 {
        ID
    }
}

struct Condition;

fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}

fn concrete<const ID: u8>(symbol: &'static str) -> ClassDescriptor {
    ClassDescriptor::concrete::<Concrete<ID>>(symbol, vec![], |_| Ok(Arc::new(Concrete)))
        .with_projection(class_projection!(Concrete<ID> => dyn View))
}

fn view() -> ClassDescriptor {
    ClassDescriptor::interface::<dyn View>("Test/View")
}

fn fixture(abstract_view: ClassDescriptor) -> Result<Container> {
    Ok(ContainerBuilder::with_class_descriptors(vec![
        abstract_view,
        concrete::<1>("Test/First"),
        concrete::<2>("Test/Second"),
        concrete::<3>("Test/Third"),
    ])?
    .build())
}

async fn resolve(container: &Container) -> Result<Arc<dyn View>> {
    tokio::time::timeout(LIMIT, container.resolve::<dyn View>())
        .await
        .map_err(|_| Error::custom("test_resolution_timeout", "条件绑定解析超时"))?
}

async fn start_waiting(
    resolution: impl Future<Output = Result<Arc<dyn View>>> + Send + 'static,
) -> Result<JoinHandle<Result<Arc<dyn View>>>> {
    let (started, ready) = oneshot::channel();
    let waiter = tokio::spawn(async move {
        let mut resolution = Box::pin(resolution);
        assert!(futures_util::poll!(resolution.as_mut()).is_pending());
        let _ = started.send(());
        resolution.await
    });
    tokio::time::timeout(LIMIT, ready)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    Ok(waiter)
}

async fn finish_waiting(waiter: JoinHandle<Result<Arc<dyn View>>>) -> Result<Arc<dyn View>> {
    tokio::time::timeout(LIMIT, waiter)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?
}

fn assert_not_instantiable(result: Result<Arc<dyn View>>) {
    assert!(matches!(result, Err(error) if error.code() == "container_class_not_instantiable"));
}

#[tokio::test]
async fn flush_restarts_pending_discovery_and_discards_its_old_selection() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let container = fixture(
        view()
            .with_bind(Bind::new(key::<Concrete<2>>()))
            .with_bind_when(BindWhen::new(key::<Concrete<1>>(), move |_| {
                let observed = Arc::clone(&observed);
                async move {
                    if observed.fetch_add(1, Ordering::Relaxed) == 0 {
                        std::future::pending::<()>().await;
                        return Ok(true);
                    }
                    Ok(false)
                }
            })),
    )?;
    container.set_binding_environment(Environment::Production)?;
    let mut old = Box::pin(resolve(&container));
    assert!(futures_util::poll!(old.as_mut()).is_pending());
    let mut waiter = Box::pin(resolve(&container));
    assert!(futures_util::poll!(waiter.as_mut()).is_pending());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    container.flush()?;
    assert_eq!(waiter.await?.selected(), 2);
    assert_eq!(old.await?.selected(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(resolve(&container).await?.selected(), 2);
    Ok(())
}

#[tokio::test]
async fn first_matching_condition_works_without_environment_and_precedes_wildcard() -> Result<()> {
    for environment in [None, Some(Environment::Production)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let first = Arc::clone(&calls);
        let second = Arc::clone(&calls);
        let container = fixture(
            view()
                .with_bind(Bind::new(key::<Concrete<1>>()))
                .with_bind_when(BindWhen::new(key::<Concrete<1>>(), move |_| {
                    let calls = Arc::clone(&first);
                    async move {
                        assert_eq!(calls.fetch_add(1, Ordering::Relaxed), 0);
                        Ok(false)
                    }
                }))
                .with_bind_when(BindWhen::new(key::<Concrete<2>>(), move |_| {
                    let calls = Arc::clone(&second);
                    async move {
                        assert_eq!(calls.fetch_add(1, Ordering::Relaxed), 1);
                        Ok(true)
                    }
                }))
                .with_bind_when(BindWhen::new(key::<Concrete<3>>(), |_| async {
                    Err(Error::custom("test_later_condition", "已选定实现"))
                })),
        )?;
        if let Some(environment) = environment {
            container.set_binding_environment(environment)?;
        }
        assert_eq!(resolve(&container).await?.selected(), 2);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
    Ok(())
}

#[tokio::test]
async fn regular_environment_binding_and_condition_share_declaration_order() -> Result<()> {
    let environment = Bind::for_environments(key::<Concrete<1>>(), ["testing"]);
    let conditional = || BindWhen::new(key::<Concrete<2>>(), |_| async { Ok(true) });
    for (descriptor, selected) in [
        (
            view()
                .with_bind(environment.clone())
                .with_bind_when(conditional()),
            1,
        ),
        (
            view().with_bind_when(conditional()).with_bind(environment),
            2,
        ),
    ] {
        let container = fixture(descriptor)?;
        container.set_binding_environment(Environment::Testing)?;
        assert_eq!(resolve(&container).await?.selected(), selected);
    }
    Ok(())
}

#[tokio::test]
async fn miss_retries_after_dependency_registration_and_match_stays_bound() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let evaluations = Arc::clone(&calls);
    let container = fixture(view().with_bind_when(BindWhen::new(
        key::<Concrete<1>>(),
        move |resolver: Resolver| {
            let calls = Arc::clone(&evaluations);
            async move {
                calls.fetch_add(1, Ordering::Relaxed);
                resolver.is_bound::<Condition>()
            }
        },
    )))?;

    assert_not_instantiable(resolve(&container).await);
    assert!(!container.contains::<dyn View>());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    container.instance(Arc::new(Condition)).await?;
    assert_eq!(resolve(&container).await?.selected(), 1);
    assert!(container.contains::<dyn View>());
    assert_eq!(resolve(&container).await?.selected(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    Ok(())
}

#[tokio::test]
async fn condition_error_stops_scan_and_preserves_checked_state() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let evaluations = Arc::clone(&calls);
    let container = fixture(
        view()
            .with_bind_when(BindWhen::new(key::<Concrete<1>>(), move |_| {
                let calls = Arc::clone(&evaluations);
                async move {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Err(Error::custom("test_condition_failed", "条件执行失败"))
                }
            }))
            .with_bind(Bind::new(key::<Concrete<2>>())),
    )?;
    container.set_binding_environment(Environment::Testing)?;

    assert!(
        matches!(resolve(&container).await, Err(error) if error.code() == "test_condition_failed")
    );
    assert_not_instantiable(resolve(&container).await);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!container.contains::<dyn View>());
    Ok(())
}

#[tokio::test]
async fn condition_reentry_through_same_resolver_uses_ordinary_resolution() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let evaluations = Arc::clone(&calls);
    let container = fixture(
        view()
            .with_bind(Bind::new(key::<Concrete<2>>()))
            .with_bind_when(BindWhen::new(
                key::<Concrete<1>>(),
                move |resolver: Resolver| {
                    let calls = Arc::clone(&evaluations);
                    async move {
                        assert_eq!(calls.fetch_add(1, Ordering::Relaxed), 0);
                        assert_not_instantiable(resolver.resolve::<dyn View>().await);
                        Ok(true)
                    }
                },
            )),
    )?;
    container.set_binding_environment(Environment::Testing)?;
    assert_eq!(resolve(&container).await?.selected(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test]
async fn concurrent_first_resolutions_share_one_successful_evaluation() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let evaluations = Arc::clone(&calls);
    let gate = Arc::clone(&release);
    let container = fixture(view().with_bind_when(BindWhen::new(
        key::<Concrete<1>>(),
        move |_| {
            let calls = Arc::clone(&evaluations);
            let gate = Arc::clone(&gate);
            async move {
                calls.fetch_add(1, Ordering::Relaxed);
                gate.notified().await;
                Ok(true)
            }
        },
    )))?;

    let mut first = Box::pin(resolve(&container));
    let mut second = Box::pin(resolve(&container));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    release.notify_waiters();

    let (first, second) = tokio::try_join!(first, second)?;
    assert_eq!(first.selected(), 1);
    assert_eq!(second.selected(), 1);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

fn fixture_with_pending_first_condition() -> Result<(Container, Arc<AtomicUsize>)> {
    let calls = Arc::new(AtomicUsize::new(0));
    let evaluations = Arc::clone(&calls);
    let container = fixture(view().with_bind_when(BindWhen::new(
        key::<Concrete<1>>(),
        move |_| {
            let calls = Arc::clone(&evaluations);
            async move {
                if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    std::future::pending::<()>().await;
                }
                Ok(true)
            }
        },
    )))?;
    Ok((container, calls))
}

#[tokio::test]
async fn cancellation_of_first_evaluation_allows_waiter_to_retry() -> Result<()> {
    let (container, calls) = fixture_with_pending_first_condition()?;
    let mut first = Box::pin(resolve(&container));
    let mut second = Box::pin(resolve(&container));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(first);
    assert_eq!(second.await?.selected(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    Ok(())
}

#[tokio::test]
async fn explicit_binding_wakes_waiter_and_reselects_pending_owner() -> Result<()> {
    let (container, calls) = fixture_with_pending_first_condition()?;
    let mut first = Box::pin(resolve(&container));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    let shared = container.clone();
    let waiter = start_waiting(async move { shared.resolve::<dyn View>().await }).await?;
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    tokio::time::timeout(
        LIMIT,
        container.bind::<dyn View, _>(|_| {
            Box::pin(async { Ok(Arc::new(Concrete::<2>) as Arc<dyn View>) })
        }),
    )
    .await
    .map_err(Error::internal)??;

    assert_eq!(finish_waiting(waiter).await?.selected(), 2);
    assert_eq!(first.await?.selected(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test]
async fn closing_owner_scope_wakes_waiter_in_other_scope() -> Result<()> {
    let (container, calls) = fixture_with_pending_first_condition()?;
    let owner_scope = container.scope()?;
    let waiter_scope = container.scope()?;
    let mut first = Box::pin(owner_scope.resolve::<dyn View>());
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    let waiter = start_waiting(async move { waiter_scope.resolve::<dyn View>().await }).await?;
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    owner_scope.close();

    assert_eq!(finish_waiting(waiter).await?.selected(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    drop(first);
    Ok(())
}
