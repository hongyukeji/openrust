//! v7：真正用户poll/Drop边界，以及claim→回收完整责任的精确门控。
use super::*;
use std::cell::RefCell;
type Hook = (Weak<Inner>, Box<dyn FnOnce() + Send>);
std::thread_local! {
    static CLAIM_HOOK:RefCell<Option<Hook>>=const{RefCell::new(None)};
    static SNAPSHOT_HOOK:RefCell<Option<Hook>>=const{RefCell::new(None)};
}
pub(super) fn install_claim_hook(owner: Weak<Inner>, action: Box<dyn FnOnce() + Send>) {
    CLAIM_HOOK.set(Some((owner, action)));
}
pub(super) fn install_snapshot_hook(owner: Weak<Inner>, action: Box<dyn FnOnce() + Send>) {
    SNAPSHOT_HOOK.set(Some((owner, action)));
}
pub(super) fn after_claim(inner: &Arc<Inner>) {
    let action = CLAIM_HOOK.with_borrow_mut(|hook| {
        if hook
            .as_ref()
            .is_some_and(|(owner, _)| Weak::ptr_eq(owner, &Arc::downgrade(inner)))
        {
            hook.take().map(|(_, action)| action)
        } else {
            None
        }
    });
    if let Some(action) = action {
        action();
    }
}
pub(super) fn after_observed_snapshot(inner: &Inner) {
    let action = SNAPSHOT_HOOK.with_borrow_mut(|hook| {
        if hook.as_ref().is_some_and(|(owner, _)| {
            owner
                .upgrade()
                .is_some_and(|owner| std::ptr::eq(Arc::as_ptr(&owner), std::ptr::from_ref(inner)))
        }) {
            hook.take().map(|(_, action)| action)
        } else {
            None
        }
    });
    if let Some(action) = action {
        action();
    }
}
struct FirstPayload(Arc<AtomicBool>);
impl Drop for FirstPayload {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct BothPanics {
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    first_gone: Arc<AtomicBool>,
    wrong_order: Arc<AtomicBool>,
}
impl Future for BothPanics {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        self.polls.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new(FirstPayload(Arc::clone(&self.first_gone))))
    }
}
impl Drop for BothPanics {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
        if !self.first_gone.load(Ordering::Acquire) {
            self.wrong_order.store(true, Ordering::Release);
        }
        std::panic::resume_unwind(Box::new("受控独立future析构异常"));
    }
}
async fn user_poll_drop_case(mode: usize) -> Result<()> {
    let owner = ResourceOwner::new();
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let first = Arc::new(AtomicBool::new(false));
    let wrong = Arc::new(AtomicBool::new(false));
    let future = BothPanics {
        polls: Arc::clone(&polls),
        drops: Arc::clone(&drops),
        first_gone: Arc::clone(&first),
        wrong_order: Arc::clone(&wrong),
    };
    let completion = ResourceTaskCompletion::new(|| {});
    match mode {
        0 => owner.registrar().spawn(move |_| future)?,
        1 => owner
            .registrar()
            .background_handle()?
            .spawn(move |_| future)?,
        _ => owner
            .registrar()
            .spawn_observed(completion.clone(), move |_| future)?,
    }
    // shutdown负责驱动尚未首次poll的任务；先让它真的poll，不能用未poll Drop替代。
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while polls.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| Error::custom("test_poll_timeout", "任务没有实际poll"))?;
    if mode == 2 {
        assert!(
            matches!(completion.wait().await,Err(error)if error.code()=="resource_task_interrupted")
        );
    }
    assert!(
        matches!(owner.shutdown(Instant::now()+std::time::Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert_eq!(polls.load(Ordering::Acquire), 1);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert!(first.load(Ordering::Acquire));
    assert!(!wrong.load(Ordering::Acquire));
    Ok(())
}
fn isolated(test: &str, marker: &str, body: impl FnOnce() -> Result<()>) -> Result<()> {
    if std::env::var_os(marker).is_some() {
        body()?;
        std::io::Write::write_all(
            &mut std::io::stdout().lock(),
            format!("{marker}:completed\n").as_bytes(),
        )
        .map_err(Error::internal)?;
        return Ok(());
    }
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let output = std::process::Command::new(std::env::current_exe().map_err(Error::internal)?)
        .args(["--exact", &format!("{module}::{test}"), "--nocapture"])
        .env(marker, "1")
        .output()
        .map_err(Error::internal)?;
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("{marker}:completed")));
    assert!(
        output.status.success(),
        "受控poll+Drop的隔离进程必须正常结束：{}",
        output.status
    );
    Ok(())
}
#[test]
fn formal_ordinary_direct_and_observed_poll_plus_drop_panics_are_isolated() -> Result<()> {
    isolated(
        "formal_ordinary_direct_and_observed_poll_plus_drop_panics_are_isolated",
        "OPENRUST_CT7_TASK_POLL_DROP_CHILD",
        || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(Error::internal)?;
            runtime.block_on(async {
                for mode in 0..3 {
                    user_poll_drop_case(mode).await?;
                }
                Ok(())
            })
        },
    )
}
