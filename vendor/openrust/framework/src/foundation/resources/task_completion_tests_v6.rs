//! 同步owner和父Children锁退出后的受控独立回调析构。
use super::*;
fn fail(payload: impl std::any::Any + Send) -> ! {
    std::panic::resume_unwind(Box::new(payload))
}
struct Next(Arc<AtomicUsize>);
impl Drop for Next {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail("PRIVATE-DROP-PAYLOAD");
    }
}
struct Capture {
    attempts: Arc<AtomicUsize>,
    free: Arc<AtomicBool>,
    parent: Weak<Inner>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        if !self.parent.upgrade().is_some_and(|parent| {
            parent.children.try_lock().is_ok() && parent.callbacks.try_lock().is_ok()
        }) {
            self.free.store(false, Ordering::Release);
        }
        fail(Next(Arc::clone(&self.attempts)));
    }
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::internal)
}
#[test]
fn owner_drop_discards_two_independent_callbacks_without_running_them_then_stops_tasks()
-> Result<()> {
    let owner = ResourceOwner::new();
    let inner = Arc::clone(&owner.inner);
    let registrar = owner.registrar();
    let tasks = inner.tasks.handle();
    let attempts = Arc::new(AtomicUsize::new(0));
    let free = Arc::new(AtomicBool::new(true));
    let executed = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let capture = Capture {
            attempts: Arc::clone(&attempts),
            free: Arc::clone(&free),
            parent: Arc::downgrade(&inner),
        };
        let count = Arc::clone(&executed);
        registrar.on_shutdown(move || {
            std::hint::black_box(&capture);
            count.fetch_add(1, Ordering::AcqRel);
            Box::pin(async { Ok(()) })
        })?;
    }
    let runtime = runtime()?;
    let entered = runtime.enter();
    registrar.spawn(|cancel| async move {
        cancel.cancelled().await;
    })?;
    drop(owner);
    assert_eq!(attempts.load(Ordering::Acquire), 4);
    assert_eq!(executed.load(Ordering::Acquire), 0);
    assert!(free.load(Ordering::Acquire));
    assert_eq!(inner.child_failures.load(Ordering::Acquire), 2);
    assert!(lock(&inner.callbacks).is_none());
    assert!(
        matches!(tasks.spawn(|_|async{}),Err(error)if error.code()=="background_tasks_stopping")
    );
    drop(inner);
    assert!(matches!(registrar.spawn(|_|async{}),Err(error)if error.code()=="resources_closed"));
    drop(entered);
    drop(runtime);
    Ok(())
}
#[tokio::test]
async fn parent_finish_child_drops_user_captures_after_children_lock_and_aggregates_unknown()
-> Result<()> {
    let parent = ResourceOwner::new();
    let child = Arc::new(ResourceOwner::new());
    let attempts = Arc::new(AtomicUsize::new(0));
    let free = Arc::new(AtomicBool::new(true));
    let executed = Arc::new(AtomicUsize::new(0));
    let capture = Capture {
        attempts: Arc::clone(&attempts),
        free: Arc::clone(&free),
        parent: Arc::downgrade(&parent.inner),
    };
    let count = Arc::clone(&executed);
    child.registrar().on_shutdown(move || {
        std::hint::black_box(&capture);
        count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(()) })
    })?;
    let id = parent.registrar().track_child(child, || {})?;
    parent.registrar().finish_child(id, false);
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(executed.load(Ordering::Acquire), 0);
    assert!(free.load(Ordering::Acquire));
    assert!(lock(&parent.inner.children).entries.is_empty());
    assert!(
        matches!(parent.shutdown(Instant::now()+std::time::Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}

struct Signal(Arc<Notify>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
struct TaskCapture(Arc<AtomicUsize>);
impl Drop for TaskCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail(Next(Arc::clone(&self.0)));
    }
}
#[test]
fn ordinary_poll_payloads_are_consumed_before_tokio_output_and_owner_drop() -> Result<()> {
    let owner = ResourceOwner::new();
    let inner = Arc::clone(&owner.inner);
    let attempts = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&callbacks);
    owner.registrar().on_shutdown(move || {
        count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(()) })
    })?;
    let runtime = runtime()?;
    runtime.block_on(async {
        let mut signals = Vec::new();
        for _ in 0..2 {
            let done = Arc::new(Notify::new());
            let signal = Signal(Arc::clone(&done));
            let payload = TaskCapture(Arc::clone(&attempts));
            owner.registrar().spawn(move |_| async move {
                let _signal = signal;
                fail(payload)
            })?;
            signals.push(done);
        }
        for done in signals {
            done.notified().await;
        }
        Ok::<(), Error>(())
    })?;
    // 没有后继submit/join来析构payload，完成producer本身必须已经清完两份有限链。
    assert_eq!(attempts.load(Ordering::Acquire), 4);
    drop(owner);
    assert_eq!(callbacks.load(Ordering::Acquire), 0);
    assert_eq!(inner.child_failures.load(Ordering::Acquire), 2);
    assert!(lock(&inner.callbacks).is_none());
    Ok(())
}
#[test]
fn ordinary_unpolled_user_f_drop_is_controlled_and_owned_failure_survives_runtime_change()
-> Result<()> {
    let owner = ResourceOwner::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime_a = runtime()?;
    {
        let _runtime = runtime_a.enter();
        let capture = TaskCapture(Arc::clone(&attempts));
        let count = Arc::clone(&calls);
        owner.registrar().spawn(move |_| async move {
            let _capture = capture;
            count.fetch_add(1, Ordering::AcqRel);
        })?;
        // 只有enter，不block_on；真正F已交付但整个空driver从未首次poll。
    }
    drop(runtime_a);
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    runtime()?.block_on(async {
        assert!(matches!(owner.shutdown(Instant::now()+std::time::Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed"));
        Ok(())
    })
}
struct RacingFuture {
    entered: Option<std::sync::mpsc::SyncSender<()>>,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    ready: bool,
    polls: Arc<AtomicUsize>,
    _capture: TaskCapture,
}
impl Future for RacingFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        self.polls.fetch_add(1, Ordering::AcqRel);
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        let (mutex, changed) = &*self.gate;
        let mut released = lock(mutex);
        while !*released {
            released = changed
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if self.ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
struct ReleaseRace(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Drop for ReleaseRace {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
fn ready_abort_race(ready: bool) -> Result<()> {
    let owner = ResourceOwner::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let polls = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&callbacks);
    owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    })?;
    let runtime_a = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(Error::internal)?;
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = ReleaseRace(Arc::clone(&gate));
    {
        let _runtime = runtime_a.enter();
        let future = RacingFuture {
            entered: Some(entered_tx),
            gate,
            ready,
            polls: Arc::clone(&polls),
            _capture: TaskCapture(Arc::clone(&attempts)),
        };
        owner.registrar().spawn(move |_| future)?;
    }
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(Error::internal)?;
    runtime_a.shutdown_background(); // 首次poll尚未返回，abort与最后Ready/Pending竞争。
    drop(release);
    runtime()?.block_on(async {
        assert!(matches!(owner.shutdown(Instant::now()+std::time::Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed"));
        Ok::<(),Error>(())
    })?;
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(polls.load(Ordering::Acquire), 1);
    assert_eq!(callbacks.load(Ordering::Acquire), 1);
    Ok(())
}
#[test]
fn ordinary_last_ready_or_pending_poll_racing_abort_keeps_cleanup_unknown_and_callbacks()
-> Result<()> {
    ready_abort_race(true)?;
    ready_abort_race(false)
}
