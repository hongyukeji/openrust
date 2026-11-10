use crate::{
    container::{
        Container, ServiceKey,
        deferred::{DeferredServices, LoadFuture},
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::future::Future as _;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Notify};

struct Loader {
    container: Weak<Container>,
    calls: AtomicUsize,
    started: Notify,
    release: Notify,
    ready: AtomicBool,
    active: AtomicBool,
    serial: Mutex<()>,
    fail: AtomicBool,
}
impl DeferredServices for Loader {
    fn in_flight(&self, _: &ServiceKey) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn contains(&self, key: &ServiceKey) -> bool {
        key == &ServiceKey::typed::<u32>() && !self.ready.load(Ordering::Acquire)
    }
    fn needs_loading(&self, key: &ServiceKey, has_instance: bool) -> bool {
        key == &ServiceKey::typed::<u32>()
            && !self.ready.load(Ordering::Acquire)
            && (self.active.load(Ordering::Acquire) || !has_instance)
    }
    fn load<'a>(&'a self, _: &'a ServiceKey) -> LoadFuture<'a> {
        Box::pin(async move {
            let _guard = self.serial.lock().await;
            if self.ready.load(Ordering::Acquire) {
                return Ok(());
            }
            self.active.store(true, Ordering::Release);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.container
                .upgrade()
                .unwrap()
                .instance(Arc::new(42_u32))?;
            self.started.notify_one();
            self.release.notified().await;
            if self.fail.load(Ordering::Acquire) {
                return Err(BindingResolutionError::FactoryFailed {
                    service: "provider",
                    source: std::io::Error::other("private initialization error").into(),
                });
            }
            self.ready.store(true, Ordering::Release);
            self.active.store(false, Ordering::Release);
            Ok(())
        })
    }
}
fn fixture() -> (Arc<Container>, Arc<Loader>) {
    let c = Arc::new(Container::default());
    let loader = Arc::new(Loader {
        container: Arc::downgrade(&c),
        calls: AtomicUsize::new(0),
        started: Notify::new(),
        release: Notify::new(),
        ready: AtomicBool::new(false),
        active: AtomicBool::new(false),
        serial: Mutex::new(()),
        fail: AtomicBool::new(false),
    });
    assert!(c.deferred.set(loader.clone()).is_ok());
    (c, loader)
}

#[tokio::test]
async fn async_lookup_waits_for_boot_without_blocking_the_executor_or_leaking_instances() {
    let (c, loader) = fixture();
    c.alias(ServiceKey::typed::<u32>(), "answer").unwrap();
    assert!(matches!(
        c.get::<u32>("answer"),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    assert_eq!(loader.calls.load(Ordering::SeqCst), 0);
    let lookup = c.getAsync::<u32>("answer");
    tokio::pin!(lookup);
    tokio::select! {
        value=&mut lookup => panic!("published before boot: {value:?}"),
        _=loader.started.notified()=>{}
    }
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    let second = c.makeAsync::<u32>();
    tokio::pin!(second);
    std::future::poll_fn(|cx| {
        assert!(second.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    loader.release.notify_one();
    let (first, second) = tokio::join!(lookup, second);
    assert!(Arc::ptr_eq(&first.unwrap(), &second.unwrap()));
    assert_eq!(*c.make::<u32>().unwrap(), 42);
    assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn explicit_instances_bypass_unstarted_loaders_and_apps_do_not_share_them() {
    let (first, a) = fixture();
    let (second, b) = fixture();
    first.instance(Arc::new(9_u32)).unwrap();
    assert_eq!(*first.makeAsync::<u32>().await.unwrap(), 9);
    assert_eq!(*first.make::<u32>().unwrap(), 9);
    second.instance(Arc::new(7_u8)).unwrap();
    assert_eq!(*second.makeAsync::<u8>().await.unwrap(), 7);
    assert!(matches!(
        second.make::<u32>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    assert_eq!(a.calls.load(Ordering::SeqCst), 0);
    assert_eq!(b.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn boot_errors_propagate_without_returning_partially_published_services() {
    let (c, loader) = fixture();
    loader.fail.store(true, Ordering::Release);
    loader.release.notify_one();
    assert!(matches!(
        c.makeAsync::<u32>().await,
        Err(BindingResolutionError::FactoryFailed { .. })
    ));
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
}

#[tokio::test]
async fn cancelled_loads_keep_the_readiness_gate_closed_and_can_be_resumed() {
    let (c, loader) = fixture();
    let mut lookup = Box::pin(c.makeAsync::<u32>());
    tokio::select! {value=&mut lookup=>panic!("early service: {value:?}"),_=loader.started.notified()=>{}}
    // Dropping the caller's future must never make its already published value usable.
    drop(lookup);
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    loader.release.notify_one();
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 42);
    assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
}
