//! 区分旧 owner 的准入快照、新代装饰目录及同代在途构造。

use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

#[derive(Default)]
struct AdmissionPause {
    entered: AtomicBool,
    resume: Notify,
}

tokio::task_local! {
    static PAUSE: Arc<AdmissionPause>;
}

pub(super) async fn after_owner_admission() {
    if let Ok(pause) = PAUSE.try_with(Arc::clone) {
        pause.entered.store(true, Ordering::Release);
        pause.resume.notified().await;
    }
}

#[tokio::test]
async fn admitted_factory_event_uses_old_chain_before_the_first_user_call() -> Result<()> {
    for lifetime in [Lifetime::Bind, Lifetime::Singleton] {
        let mut builder = ContainerBuilder::new();
        builder.bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let old_extends = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&old_extends);
        builder.extend::<usize, _>(move |value, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Arc::new(*value + 10)) })
        })?;
        let container = builder.build();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let values = Arc::clone(&seen);
        container
            .rebinding::<usize, _>(move |_, value| {
                lock(&values).push(*value);
                Box::pin(async { Ok(()) })
            })
            .await?;
        assert_eq!(old_extends.load(Ordering::SeqCst), 1);

        let pause = Arc::new(AdmissionPause::default());
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&factory_calls);
        let install = container.register::<usize, _>(lifetime, move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Arc::new(2)) })
        });
        let mut old = Box::pin(PAUSE.scope(Arc::clone(&pause), install));
        assert!(futures_util::poll!(old.as_mut()).is_pending());
        assert!(pause.entered.load(Ordering::Acquire));
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);

        // instance 事件交付自己的原值。覆盖期间清除旧链，再在新代安装另一条链。
        container.instance(Arc::new(999_usize)).await?;
        container.forget_extenders::<usize>().await?;
        container.forget_instance::<usize>().await?;
        container
            .register::<usize, _>(lifetime, |_| Box::pin(async { Ok(Arc::new(3)) }))
            .await?;
        let new_extends = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&new_extends);
        let decorate = container.extend::<usize, _>(move |value, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Arc::new(*value + 1000)) })
        });
        tokio::time::timeout(Duration::from_secs(3), decorate)
            .await
            .map_err(Error::internal)??;
        assert_eq!(*lock(&seen), vec![999, 3, 1003]);
        pause.resume.notify_one();
        old.await?;
        assert_eq!(*lock(&seen), vec![999, 3, 1003, 12]);
        assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
        assert_eq!(old_extends.load(Ordering::SeqCst), 2);
        assert_eq!(new_extends.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn scoped_owner_keeps_global_and_own_local_extenders_from_admission() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 10)) }))?;
    let container = builder.build();
    let a = container.scope()?;
    let b = container.scope()?;
    a.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 100)) }))
        .await?;
    let pause = Arc::new(AdmissionPause::default());
    let mut old = Box::pin(PAUSE.scope(Arc::clone(&pause), a.resolve::<usize>()));
    assert!(futures_util::poll!(old.as_mut()).is_pending());
    assert!(pause.entered.load(Ordering::Acquire));
    container
        .scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await?;
    a.forget_extenders::<usize>().await?;
    container.forget_extenders::<usize>().await?;
    a.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 1000)) }))
        .await?;
    pause.resume.notify_one();
    assert_eq!(*old.await?, 111);
    assert_eq!(*a.resolve::<usize>().await?, 1002);
    assert_eq!(*b.resolve::<usize>().await?, 2);
    Ok(())
}

#[tokio::test]
async fn new_epoch_extend_and_forget_complete_while_retired_transient_is_pending() -> Result<()> {
    for forget in [false, true] {
        let mut builder = ContainerBuilder::new();
        let release = Arc::new(Notify::new());
        let gate = Arc::clone(&release);
        let started = Arc::new(AtomicBool::new(false));
        let entered = Arc::clone(&started);
        builder.bind::<usize, _>(move |_| {
            entered.store(true, Ordering::Release);
            let gate = Arc::clone(&gate);
            Box::pin(async move {
                gate.notified().await;
                Ok(Arc::new(1))
            })
        });
        builder.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 10)) }))?;
        let container = builder.build();
        let mut retired = Box::pin(container.resolve::<usize>());
        assert!(futures_util::poll!(retired.as_mut()).is_pending());
        assert!(started.load(Ordering::Acquire));
        container
            .bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
            .await?;
        let operation: BoxFuture<'_, Result<()>> = if forget {
            Box::pin(container.forget_extenders::<usize>())
        } else {
            Box::pin(
                container
                    .extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 100)) })),
            )
        };
        tokio::time::timeout(Duration::from_secs(3), operation)
            .await
            .map_err(Error::internal)??;
        assert!(futures_util::poll!(retired.as_mut()).is_pending());
        release.notify_one();
        assert_eq!(*retired.await?, 11);
        assert!(
            !lock(&container.inner.state)
                .resolved_history
                .contains(&ServiceKey::typed::<usize>())
        );
        assert_eq!(
            *container.resolve::<usize>().await?,
            if forget { 2 } else { 112 }
        );
    }
    Ok(())
}

#[tokio::test]
async fn extension_waits_for_current_transient_but_not_older_one_in_same_scope() -> Result<()> {
    let old_release = Arc::new(Notify::new());
    let gate = Arc::clone(&old_release);
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(move |_| {
        let gate = Arc::clone(&gate);
        Box::pin(async move {
            gate.notified().await;
            Ok(Arc::new(1))
        })
    });
    let container = builder.build();
    let mut old = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(old.as_mut()).is_pending());

    let current_release = Arc::new(Notify::new());
    let gate = Arc::clone(&current_release);
    container
        .bind::<usize, _>(move |_| {
            let gate = Arc::clone(&gate);
            Box::pin(async move {
                gate.notified().await;
                Ok(Arc::new(2))
            })
        })
        .await?;
    let mut current = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(current.as_mut()).is_pending());
    let mut extend = Box::pin(
        container.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 100)) })),
    );
    assert!(futures_util::poll!(extend.as_mut()).is_pending());
    current_release.notify_one();
    assert_eq!(*current.await?, 2);
    tokio::time::timeout(Duration::from_secs(3), extend)
        .await
        .map_err(Error::internal)??;
    assert!(futures_util::poll!(old.as_mut()).is_pending());
    old_release.notify_one();
    assert_eq!(*old.await?, 1);
    current_release.notify_one();
    assert_eq!(*container.resolve::<usize>().await?, 102);
    Ok(())
}
