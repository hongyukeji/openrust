//! 检出factory被instance覆盖、forget误触hook/旧构造回填及锁内析构的回归。
use super::*;
use std::sync::atomic::AtomicUsize;

fn resolver(container: &Container) -> Resolver {
    let state = lock(&container.inner.state);
    container.resolver(
        state.root_scope,
        Arc::clone(&state.root_closed),
        state.root_resources.registrar(),
    )
}

fn factory(container: &Container) -> Result<Factory> {
    lock(&container.inner.state)
        .bindings
        .get(&ServiceKey::typed::<usize>())
        .and_then(|binding| binding.factory.clone())
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))
}

#[tokio::test]
async fn original_factory_identity_lifetime_and_history_survive_instance_and_forget() -> Result<()>
{
    for lifetime in [Lifetime::Bind, Lifetime::Singleton, Lifetime::Scoped] {
        let mut builder = ContainerBuilder::new();
        builder.register::<usize, _>(lifetime, |_| Box::pin(async { Ok(Arc::new(11)) }));
        let before = builder
            .bindings
            .get(&ServiceKey::typed::<usize>())
            .and_then(|binding| binding.factory.clone())
            .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
        builder.instance(Arc::new(22_usize));
        let entry = builder
            .bindings
            .get(&ServiceKey::typed::<usize>())
            .ok_or_else(|| container_error("test_binding_missing", "缺少条目"))?;
        assert!(matches!(&entry.factory, Some(current) if Arc::ptr_eq(&before,current)));
        assert!(entry.lifetime == lifetime);
        assert!(entry.effective_lifetime() == Lifetime::Singleton);
        let container = builder.build();
        assert_eq!(*container.resolve::<usize>().await?, 22);
        let raw = (factory(&container)?)(resolver(&container)).await?;
        assert!(matches!(raw.downcast_ref::<Arc<usize>>(),Some(value) if **value==11));
        assert!(
            !lock(&container.inner.state)
                .resolved_history
                .contains(&ServiceKey::typed::<usize>())
        );
        container.forget_instance::<usize>().await?;
        assert!(container.contains::<usize>());
        assert!(Arc::ptr_eq(&before, &factory(&container)?));
        assert_eq!(*container.resolve::<usize>().await?, 11);
        assert!(
            lock(&container.inner.state)
                .resolved_history
                .contains(&ServiceKey::typed::<usize>())
        );
        container.instance(Arc::new(33_usize)).await?;
        container.forget_instance::<usize>().await?;
        assert!(
            lock(&container.inner.state)
                .resolved_history
                .contains(&ServiceKey::typed::<usize>())
        );
        assert_eq!(*container.resolve::<usize>().await?, 11);
    }
    Ok(())
}

#[tokio::test]
async fn instance_only_has_no_factory_and_forget_removes_the_entry_without_history() -> Result<()> {
    let container = ContainerBuilder::new().build();
    container.instance(Arc::new(3_usize)).await?;
    assert!(container.contains::<usize>());
    assert!(factory(&container).is_err());
    assert_eq!(*container.resolve::<usize>().await?, 3);
    container.forget_instance::<usize>().await?;
    assert!(!container.contains::<usize>());
    assert_eq!(container.len(), 0);
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&ServiceKey::typed::<usize>())
    );
    assert!(
        matches!(container.resolve::<usize>().await,Err(error) if error.code()=="service_not_bound")
    );
    Ok(())
}

#[tokio::test]
async fn forget_does_not_notify_rebinding_and_existing_arcs_remain_valid() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(5)) }));
    let container = builder.build();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    container
        .rebinding::<usize, _>(move |_, _| {
            let calls = Arc::clone(&observed);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await?;
    let original = container.resolve::<usize>().await?;
    container.instance(Arc::new(9_usize)).await?;
    let replacement = container.resolve::<usize>().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    container.forget_instance::<usize>().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*original, 5);
    assert_eq!(*replacement, 9);
    assert_eq!(*container.resolve::<usize>().await?, 5);
    container.forget_instance::<usize>().await?;
    container
        .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(17)) }))
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(*container.resolve::<usize>().await?, 17);
    Ok(())
}

#[tokio::test]
async fn forgotten_in_flight_factory_cannot_fill_the_new_epoch_or_write_history() -> Result<()> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    let (started, gate, count) = (
        Arc::clone(&entered),
        Arc::clone(&release),
        Arc::clone(&calls),
    );
    builder.singleton::<usize, _>(move |_| {
        let (started, gate, count) = (Arc::clone(&started), Arc::clone(&gate), Arc::clone(&count));
        Box::pin(async move {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                started.notify_one();
                gate.notified().await;
                Ok(Arc::new(11))
            } else {
                Ok(Arc::new(22))
            }
        })
    });
    let container = builder.build();
    let mut old = Box::pin(container.resolve::<usize>());
    tokio::select! {result=&mut old=>{result?;return Err(container_error("test_factory_not_blocked","旧工厂应等待"));},()=entered.notified()=>{}}
    // 未发布instance时forget是no-op；先提交真实覆盖，旧构造才应失去发布资格。
    container.instance(Arc::new(999_usize)).await?;
    container.forget_instance::<usize>().await?;
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&ServiceKey::typed::<usize>())
    );
    release.notify_one();
    assert_eq!(*old.await?, 11);
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&ServiceKey::typed::<usize>())
    );
    assert!(container.get_ready::<usize>().is_none());
    assert_eq!(*container.resolve::<usize>().await?, 22);
    assert_eq!(
        *container
            .get_ready::<usize>()
            .ok_or_else(|| container_error("test_cache_missing", "缺少新缓存"))?,
        22
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

struct DropProbe {
    inner: Weak<Inner>,
    outside_lock: Arc<AtomicBool>,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            self.outside_lock
                .store(inner.state.try_lock().is_ok(), Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn forgotten_last_instance_is_dropped_outside_state_lock() -> Result<()> {
    let container = ContainerBuilder::new().build();
    let outside_lock = Arc::new(AtomicBool::new(false));
    container
        .instance(Arc::new(DropProbe {
            inner: Arc::downgrade(&container.inner),
            outside_lock: Arc::clone(&outside_lock),
        }))
        .await?;
    container.forget_instance::<DropProbe>().await?;
    assert!(outside_lock.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn original_scoped_factory_and_persistent_extenders_resume_after_forget() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let sequence = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&sequence);
    builder.scoped::<usize, _>(move |_| {
        let sequence = Arc::clone(&observed);
        Box::pin(async move { Ok(Arc::new(sequence.fetch_add(1, Ordering::SeqCst) + 1)) })
    });
    builder.extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 100)) }))?;
    builder.instance(Arc::new(999_usize));
    let container = builder.build();
    let first = container.scope()?;
    let second = container.scope()?;
    let overridden = first.resolve::<usize>().await?;
    assert!(Arc::ptr_eq(&overridden, &second.resolve::<usize>().await?));
    assert_eq!(*overridden, 999);
    assert_eq!(sequence.load(Ordering::SeqCst), 0);
    container.forget_instance::<usize>().await?;
    let one = first.resolve::<usize>().await?;
    let two = second.resolve::<usize>().await?;
    assert_eq!(*one, 101);
    assert_eq!(*two, 102);
    assert!(!Arc::ptr_eq(&one, &two));
    assert_eq!(*overridden, 999);
    first.close();
    second.close();
    Ok(())
}

#[tokio::test]
async fn failed_or_cancelled_factory_never_creates_successful_history() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(|_| {
        Box::pin(async { Err(Error::custom("test_factory_failed", "测试工厂失败")) })
    });
    let container = builder.build();
    assert!(container.resolve::<usize>().await.is_err());
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&ServiceKey::typed::<usize>())
    );
    let entered = Arc::new(Notify::new());
    let announced = Arc::clone(&entered);
    container
        .singleton::<usize, _>(move |_| {
            let entered = Arc::clone(&announced);
            Box::pin(async move {
                entered.notify_one();
                std::future::pending::<Result<Arc<usize>>>().await
            })
        })
        .await?;
    let mut pending = Box::pin(container.resolve::<usize>());
    tokio::select! {result=&mut pending=>{result?;return Err(container_error("test_factory_not_pending","工厂应等待取消"));},()=entered.notified()=>{}}
    drop(pending);
    assert!(
        !lock(&container.inner.state)
            .resolved_history
            .contains(&ServiceKey::typed::<usize>())
    );
    assert!(container.get_ready::<usize>().is_none());
    Ok(())
}

#[tokio::test]
async fn resolving_explicit_instance_does_not_fabricate_factory_rebind_history() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(2_usize));
    let container = builder.build();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    container
        .rebinding::<usize, _>(move |_, _| {
            let calls = Arc::clone(&observed);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
        .await?;
    container
        .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(3)) }))
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(container.get_ready::<usize>().is_none());
    assert_eq!(*container.resolve::<usize>().await?, 3);
    Ok(())
}
