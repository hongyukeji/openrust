use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    future::Future as _,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
use tokio::sync::Notify;

#[tokio::test]
async fn shared_async_factories_wait_without_blocking_and_publish_once() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (a, b, n) = (started.clone(), release.clone(), calls.clone());
    c.singletonAsync::<u32, _>(move |_| {
        let (a, b, n) = (a.clone(), b.clone(), n.clone());
        Box::pin(async move {
            n.fetch_add(1, Ordering::SeqCst);
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(42))
        })
    })
    .await
    .unwrap();
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    let mut one = Box::pin(c.makeAsync::<u32>());
    tokio::select! {v=&mut one=>panic!("early value {v:?}"),_=started.notified()=>{}}
    let mut two = Box::pin(c.makeAsync::<u32>());
    std::future::poll_fn(|cx| {
        assert!(two.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    release.notify_one();
    let (a, b) = tokio::join!(one, two);
    let a = a.unwrap();
    assert!(Arc::ptr_eq(&a, &b.unwrap()));
    assert!(Arc::ptr_eq(&a, &c.make::<u32>().unwrap()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn dropping_a_constructor_reports_one_attempt_failure_then_allows_an_explicit_retry() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let started = Arc::new(Notify::new());
    let (n, a, b) = (calls.clone(), started.clone(), release.clone());
    c.singletonAsync::<u32, _>(move |_| {
        let (n, a, b) = (n.clone(), a.clone(), b.clone());
        Box::pin(async move {
            n.fetch_add(1, Ordering::SeqCst);
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(8))
        })
    })
    .await
    .unwrap();
    let mut one = Box::pin(c.makeAsync::<u32>());
    tokio::select! {v=&mut one=>panic!("early value {v:?}"),_=started.notified()=>{}}
    let mut two = Box::pin(c.makeAsync::<u32>());
    std::future::poll_fn(|cx| {
        assert!(two.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(one);
    release.notify_one();
    assert!(two.await.is_err());
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 8);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn async_factories_resolve_other_async_factories_using_current_context() {
    let c = Container::default();
    c.bindAsync::<u16, _>(|_| {
        Box::pin(async {
            tokio::task::yield_now().await;
            Ok(Arc::new(3))
        })
    })
    .await
    .unwrap();
    c.bindAsync::<u32, _>(|c| {
        Box::pin(async move {
            let value = c.makeAsync::<u16>().await?;
            Ok(Arc::new(u32::from(*value) + 1))
        })
    })
    .await
    .unwrap();
    c.when::<u32>()
        .unwrap()
        .needs::<u16>()
        .give(|_| Ok(Arc::new(7)))
        .unwrap();
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 8);
    assert_eq!(*c.makeAsync::<u16>().await.unwrap(), 3);
}
#[tokio::test]
async fn pending_decorators_and_resolution_hooks_keep_publication_order() {
    let c = Container::default();
    c.singletonAsync::<u32, _>(|_| Box::pin(async { Ok(Arc::new(4)) }))
        .await
        .unwrap();
    c.extend::<u32, _>(ServiceKey::typed::<u32>(), |v, _| Ok(Arc::new(*v + 2)))
        .unwrap();
    c.afterResolving(Some(ServiceKey::typed::<u32>()), |event, c| {
        assert_eq!(*event.value::<u32>().unwrap(), 6);
        c.extend::<u32, _>(ServiceKey::typed::<u32>(), |v, _| Ok(Arc::new(*v + 3)))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 6);
    assert_eq!(*c.make::<u32>().unwrap(), 9);
}

#[tokio::test]
async fn cross_task_async_dependency_cycles_fail_both_attempts_without_replaying_factories() {
    let c = Container::default();
    let gate = Arc::new(tokio::sync::Barrier::new(2));
    let n = Arc::new(AtomicUsize::new(0));
    let (a, count) = (gate.clone(), n.clone());
    c.singletonAsync::<u16, _>(move |c| {
        let (a, count) = (a.clone(), count.clone());
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            a.wait().await;
            c.makeAsync::<u32>().await?;
            Ok(Arc::new(1))
        })
    })
    .await
    .unwrap();
    let (a, count) = (gate, n.clone());
    c.singletonAsync::<u32, _>(move |c| {
        let (a, count) = (a.clone(), count.clone());
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            a.wait().await;
            c.makeAsync::<u16>().await?;
            Ok(Arc::new(2))
        })
    })
    .await
    .unwrap();
    let (a, b) = tokio::join!(c.makeAsync::<u16>(), c.makeAsync::<u32>());
    assert!(matches!(
        a,
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    assert!(matches!(
        b,
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    assert_eq!(n.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn scope_caches_and_instance_overrides_stay_local_across_awaits() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let count = n.clone();
    c.scopedAsync::<usize, _>(move |_| {
        let count = count.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(Arc::new(count.fetch_add(1, Ordering::SeqCst) + 1))
        })
    })
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        c.scope(async {
            let first = c.makeAsync::<usize>().await.unwrap();
            assert!(Arc::ptr_eq(&first, &c.makeAsync::<usize>().await.unwrap()));
            c.instance(Arc::new(99_usize)).unwrap();
            assert_eq!(*c.makeAsync::<usize>().await.unwrap(), 99);
            *first
        }),
        c.scope(async {
            let first = c.makeAsync::<usize>().await.unwrap();
            assert!(Arc::ptr_eq(&first, &c.makeAsync::<usize>().await.unwrap()));
            *first
        })
    );
    assert_ne!(a, b);
    assert_eq!(n.load(Ordering::SeqCst), 2);
    assert_ne!(*c.makeAsync::<usize>().await.unwrap(), 99);
    c.forgetScopedInstances();
    assert!(matches!(
        c.make::<usize>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
}

#[tokio::test]
async fn named_lifetimes_conditions_and_output_checks_preserve_the_registry_contract() {
    let c = Container::default();
    c.bindAsyncNamed::<u32, _>("transient", |_| Box::pin(async { Ok(Arc::new(1)) }))
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(
        &c.getAsync::<u32>("transient").await.unwrap(),
        &c.getAsync::<u32>("transient").await.unwrap()
    ));
    c.singletonAsyncNamed::<u32, _>("shared", |_| Box::pin(async { Ok(Arc::new(2)) }))
        .await
        .unwrap();
    let shared = c.getAsync::<u32>("shared").await.unwrap();
    c.scopedIfAsyncNamed::<u32, _>("shared", |_| {
        Box::pin(async { panic!("conditional scoped overwrite") })
    })
    .await
    .unwrap();
    c.bindIfAsyncNamed::<u32, _>("shared", |_| {
        Box::pin(async { panic!("conditional transient overwrite") })
    })
    .await
    .unwrap();
    c.singletonIfAsyncNamed::<u32, _>("shared", |_| {
        Box::pin(async { panic!("conditional singleton overwrite") })
    })
    .await
    .unwrap();
    c.forgetScopedInstances();
    assert!(Arc::ptr_eq(
        &shared,
        &c.getAsync::<u32>("shared").await.unwrap()
    ));
    c.alias("shared", "alias").unwrap();
    assert!(Arc::ptr_eq(
        &shared,
        &c.getAsync::<u32>("alias").await.unwrap()
    ));
    assert!(matches!(
        c.getAsync::<u16>("shared").await,
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    c.scopedAsyncNamed::<u16, _>("scoped", |_| Box::pin(async { Ok(Arc::new(3)) }))
        .await
        .unwrap();
    let a = c
        .scope(async { c.getAsync::<u16>("scoped").await.unwrap() })
        .await;
    let b = c
        .scope(async { c.getAsync::<u16>("scoped").await.unwrap() })
        .await;
    assert!(!Arc::ptr_eq(&a, &b));
}

#[tokio::test]
async fn consumer_frames_do_not_escape_a_pending_poll_or_inherit_another_roots_rules() {
    let c = Container::default();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    c.bind::<u16, _>(|_| Ok(Arc::new(1))).unwrap();
    let (a, b) = (started.clone(), release.clone());
    c.bindAsyncNamed::<u32, _>("a", move |c| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(u32::from(*c.makeAsync::<u16>().await?)))
        })
    })
    .await
    .unwrap();
    c.bindAsyncNamed::<u32, _>("b", |c| {
        Box::pin(async move { Ok(Arc::new(u32::from(*c.makeAsync::<u16>().await?))) })
    })
    .await
    .unwrap();
    c.whenNamed("a")
        .unwrap()
        .needs::<u16>()
        .giveAsync(|_| {
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(7))
            })
        })
        .unwrap();
    c.whenNamed("b")
        .unwrap()
        .needs::<u16>()
        .giveAsync(|_| Box::pin(async { Ok(Arc::new(9)) }))
        .unwrap();
    let mut a = Box::pin(c.getAsync::<u32>("a"));
    tokio::select! {v=&mut a=>panic!("early value {v:?}"),_=started.notified()=>{}}
    assert_eq!(*c.makeAsync::<u16>().await.unwrap(), 1);
    assert_eq!(*c.getAsync::<u32>("b").await.unwrap(), 9);
    release.notify_one();
    assert_eq!(*a.await.unwrap(), 7);
    assert_eq!(*c.makeAsync::<u16>().await.unwrap(), 1);
}

#[tokio::test]
async fn asynchronous_contextual_tags_and_config_keep_lazy_resolution() {
    let c = Container::default();
    c.bindAsyncNamed::<u16, _>("one", |_| Box::pin(async { Ok(Arc::new(1)) }))
        .await
        .unwrap();
    c.bindAsyncNamed::<u16, _>("two", |_| Box::pin(async { Ok(Arc::new(2)) }))
        .await
        .unwrap();
    c.tag(
        [ServiceKey::named("one"), ServiceKey::named("two")],
        ["numbers"],
    );
    c.bindAsync::<Vec<Arc<u16>>, _>(|c| {
        Box::pin(async move {
            c.tagged::<u16>("numbers")
                .collectAsync()
                .await
                .map(Arc::new)
        })
    })
    .await
    .unwrap();
    assert_eq!(
        c.makeAsync::<Vec<Arc<u16>>>()
            .await
            .unwrap()
            .iter()
            .map(|x| **x)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    c.bindAsync::<u32, _>(|c| {
        Box::pin(async move {
            let all = c.makeAsync::<Vec<Arc<u16>>>().await?;
            Ok(Arc::new(all.iter().map(|x| u32::from(**x)).sum()))
        })
    })
    .await
    .unwrap();
    c.when::<u32>()
        .unwrap()
        .needs::<Vec<Arc<u16>>>()
        .giveTaggedAsync("numbers")
        .unwrap();
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 3);
    c.singletonAsyncNamed::<dyn openrust::contracts::config::Repository, _>("config", |_| {
        Box::pin(async {
            let config = openrust::config::Repository::new(std::collections::BTreeMap::from([(
                "limit".into(),
                serde_json::json!(11),
            )]));
            Ok(Arc::new(config) as Arc<dyn openrust::contracts::config::Repository>)
        })
    })
    .await
    .unwrap();
    c.bindAsyncNamed::<u16, _>("from-config", |c| {
        Box::pin(async move { c.makeAsync::<u16>().await })
    })
    .await
    .unwrap();
    c.whenNamed("from-config")
        .unwrap()
        .needs::<u16>()
        .giveServiceAsync("two")
        .unwrap();
    assert_eq!(*c.getAsync::<u16>("from-config").await.unwrap(), 2);
    c.whenNamed("from-config")
        .unwrap()
        .needs::<u16>()
        .giveConfigAsync("limit", None)
        .unwrap();
    assert_eq!(*c.getAsync::<u16>("from-config").await.unwrap(), 11);
}

#[tokio::test]
async fn async_rebinding_refresh_and_persistent_extensions_deliver_the_committed_value() {
    let c = Container::default();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    c.bindAsync::<u32, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await
        .unwrap();
    c.refreshAsync::<u32, _, _>(ServiceKey::typed::<u32>(), seen.clone(), |seen, value| {
        seen.lock().unwrap().push(*value);
        Ok(())
    })
    .await
    .unwrap();
    assert!(seen.lock().unwrap().is_empty());
    c.extendAsync::<u32, _>(ServiceKey::typed::<u32>(), |value, _| {
        Ok(Arc::new(*value + 3))
    })
    .await
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), [5]);
    c.bindAsync::<u32, _>(|_| Box::pin(async { Ok(Arc::new(4)) }))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), [5, 7]);
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 7);
}

#[tokio::test]
async fn a_cancelled_async_tag_read_does_not_skip_its_pending_identity() {
    let c = Container::default();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (a, b) = (started.clone(), release.clone());
    c.bindAsync::<u16, _>(move |_| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(3))
        })
    })
    .await
    .unwrap();
    c.tag([ServiceKey::typed::<u16>()], ["pending"]);
    let tagged = c.tagged::<u16>("pending");
    let mut iter = tagged.iter();
    let mut first = Box::pin(iter.nextAsync());
    tokio::select! {value=&mut first=>panic!("early value {value:?}"),_=started.notified()=>{}}
    drop(first);
    assert_eq!(iter.len(), 1);
    release.notify_one();
    assert_eq!(*iter.nextAsync().await.unwrap().unwrap(), 3);
    assert!(iter.nextAsync().await.is_none());
}

#[tokio::test]
async fn a_suspended_resolution_cannot_reopen_a_closed_scoped_cache() {
    let c = Container::default();
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    c.scopedAsync::<usize, _>(move |_| {
        let n = n.clone();
        Box::pin(async move { Ok(Arc::new(n.fetch_add(1, Ordering::SeqCst) + 1)) })
    })
    .await
    .unwrap();
    let (a, b) = (started.clone(), release.clone());
    c.bindAsync::<u32, _>(move |c| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(*c.makeAsync::<usize>().await? as u32))
        })
    })
    .await
    .unwrap();
    let mut pending = Box::pin(c.makeAsync::<u32>());
    let original = c
        .scope(async {
            let original = c.makeAsync::<usize>().await.unwrap();
            tokio::select! {v=&mut pending=>panic!("early value {v:?}"),_=started.notified()=>{}}
            original
        })
        .await;
    c.scope(async {
        let current = c.makeAsync::<usize>().await.unwrap();
        assert_ne!(*current, *original);
        release.notify_one();
        assert!(matches!(
            pending.await,
            Err(BindingResolutionError::ScopeClosed)
        ));
        assert!(Arc::ptr_eq(
            &current,
            &c.makeAsync::<usize>().await.unwrap()
        ));
    })
    .await;
}

#[tokio::test]
async fn concurrent_factory_errors_are_shared_without_replaying_user_code() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (count, a, b) = (n.clone(), started.clone(), release.clone());
    c.singletonAsync::<u32, _>(move |_| {
        let (count, a, b) = (count.clone(), a.clone(), b.clone());
        Box::pin(async move {
            let attempt = count.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                a.notify_one();
                b.notified().await;
                return Err(BindingResolutionError::FactoryFailed {
                    service: "probe",
                    source: std::io::Error::other("private cause").into(),
                });
            }
            Ok(Arc::new(9))
        })
    })
    .await
    .unwrap();
    let mut a = Box::pin(c.makeAsync::<u32>());
    tokio::select! {v=&mut a=>panic!("early value {v:?}"),_=started.notified()=>{}}
    let mut b = Box::pin(c.makeAsync::<u32>());
    std::future::poll_fn(|cx| {
        assert!(b.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    release.notify_one();
    let (a, b) = tokio::join!(a, b);
    for result in [a, b] {
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            BindingResolutionError::FactoryFailed { .. }
        ));
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut found = false;
        while let Some(error) = source {
            found |= error.to_string().contains("private cause");
            source = error.source();
        }
        assert!(found);
    }
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 9);
}

#[tokio::test]
async fn retired_in_flight_caches_cannot_create_history_for_a_new_epoch() {
    let c = Container::default();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (a, b) = (started.clone(), release.clone());
    c.singletonAsync::<u32, _>(move |_| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(4))
        })
    })
    .await
    .unwrap();
    let mut old = Box::pin(c.makeAsync::<u32>());
    tokio::select! {v=&mut old=>panic!("early value {v:?}"),_=started.notified()=>{}}
    c.forgetInstance(ServiceKey::typed::<u32>());
    release.notify_one();
    assert_eq!(*old.await.unwrap(), 4);
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
}

#[tokio::test]
async fn a_panicking_constructor_releases_waiters_and_allows_a_new_attempt() {
    let c = Arc::new(Container::default());
    let count = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (n, a, b) = (count.clone(), started.clone(), release.clone());
    c.singletonAsync::<u16, _>(move |_| {
        let (n, a, b) = (n.clone(), a.clone(), b.clone());
        Box::pin(async move {
            if n.fetch_add(1, Ordering::SeqCst) == 0 {
                a.notify_one();
                b.notified().await;
                panic!("constructor panic")
            };
            Ok(Arc::new(4))
        })
    })
    .await
    .unwrap();
    let first = c.clone();
    let task = tokio::spawn(async move { first.makeAsync::<u16>().await });
    started.notified().await;
    let mut waiter = Box::pin(c.makeAsync::<u16>());
    std::future::poll_fn(|cx| {
        assert!(waiter.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    release.notify_one();
    assert!(task.await.unwrap_err().is_panic());
    assert!(waiter.await.is_err());
    assert!(!c.resolved(ServiceKey::typed::<u16>()).unwrap());
    assert_eq!(*c.makeAsync::<u16>().await.unwrap(), 4);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn async_readers_of_a_sync_constructor_wait_without_blocking_the_executor() {
    let c = Arc::new(Container::default());
    let (started, entered) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let wait = std::sync::Mutex::new(wait);
    c.singleton::<u32, _>(move |_| {
        started.send(()).unwrap();
        wait.lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(Arc::new(8))
    })
    .unwrap();
    let other = c.clone();
    let thread = std::thread::spawn(move || other.make::<u32>().unwrap());
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (value, ()) = tokio::join!(c.makeAsync::<u32>(), async {
        tokio::task::yield_now().await;
        release.send(()).unwrap();
    });
    assert!(Arc::ptr_eq(&value.unwrap(), &thread.join().unwrap()));
}

#[tokio::test]
async fn a_post_callback_can_forget_publication_without_erasing_success_history() {
    let c = Container::default();
    c.singletonAsync::<u16, _>(|_| Box::pin(async { Ok(Arc::new(3)) }))
        .await
        .unwrap();
    c.afterResolving(Some(ServiceKey::typed::<u16>()), |_, c| {
        c.forgetInstance(ServiceKey::typed::<u16>());
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.makeAsync::<u16>().await.unwrap(), 3);
    assert!(c.resolved(ServiceKey::typed::<u16>()).unwrap());
    assert!(matches!(
        c.make::<u16>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
}

#[tokio::test]
async fn closing_a_scope_drains_a_service_that_owns_a_suspended_resolution() {
    struct Holder {
        pending: std::sync::Mutex<Option<openrust::container::FactoryFuture<'static, u16>>>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Holder {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Arc::new(Container::default());
    let drops = Arc::new(AtomicUsize::new(0));
    let count = drops.clone();
    c.scoped::<Holder, _>(move |_| {
        Ok(Arc::new(Holder {
            pending: std::sync::Mutex::new(None),
            drops: count.clone(),
        }))
    })
    .unwrap();
    let entered = Arc::new(Notify::new());
    let notify = entered.clone();
    c.scopedAsync::<u16, _>(move |_| {
        let notify = notify.clone();
        Box::pin(async move {
            notify.notify_one();
            std::future::pending().await
        })
    })
    .await
    .unwrap();
    c.scope(async {
        let holder = c.makeAsync::<Holder>().await.unwrap();
        let owned = c.clone();
        let mut pending: openrust::container::FactoryFuture<'static, u16> =
            Box::pin(async move { owned.makeAsync::<u16>().await });
        tokio::select! {value=&mut pending=>panic!("unexpected {value:?}"),_=entered.notified()=>{}}
        *holder.pending.lock().unwrap() = Some(pending);
    })
    .await;
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
