use openrust::support::{Benchmark, Sleep, Timebox};
use std::{
    cell::RefCell,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::time::Instant;

#[tokio::test]
async fn benchmark_returns_values_and_preserves_named_iteration_order() {
    let (value, milliseconds) = Benchmark::value(|| 42);
    assert_eq!(value, 42);
    assert!(milliseconds >= 0.0);
    let calls = RefCell::new(Vec::new());
    let mut first = || calls.borrow_mut().push("first");
    let mut second = || calls.borrow_mut().push("second");
    let iterations = NonZeroU32::new(3).unwrap();
    let measured = Benchmark::measure_many(
        &mut [("first", &mut first), ("second", &mut second)],
        iterations,
    );
    assert_eq!(
        measured.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(
        calls.into_inner(),
        ["first", "first", "first", "second", "second", "second"]
    );

    let (value, elapsed) = Benchmark::value_async(async {
        Sleep::r#for(Duration::from_millis(3))
            .then(|| "ready")
            .await
    })
    .await;
    assert_eq!(value, "ready");
    assert!(elapsed >= 3.0);
}

#[tokio::test]
async fn sleep_fake_captures_the_sequence_and_respects_skips() {
    let fake = Sleep::fake();
    let separate = Sleep::fake();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&observed);
    fake.when_faking_sleep(move |duration| log.lock().unwrap().push(duration));
    let total = Duration::from_micros(1500);
    assert_eq!(
        Sleep::usleep(1000)
            .and(Duration::from_micros(500))
            .using(&fake)
            .then(|| 42)
            .await,
        42
    );
    Sleep::sleep(2).unless(true).using(&fake).wait().await;
    Sleep::until(SystemTime::UNIX_EPOCH)
        .using(&fake)
        .wait()
        .await;
    fake.assert_slept(|duration| *duration == total, 1);
    fake.assert_sequence(&[Some(total), Some(Duration::ZERO)]);
    assert_eq!(*observed.lock().unwrap(), [total, Duration::ZERO]);
    separate.assert_never_slept();
    separate.assert_insomniac();
}

#[tokio::test]
async fn timebox_pads_ok_and_err_results_and_leaves_overrunning_work_alone() {
    let minimum = Duration::from_millis(15);
    let mut timebox = Timebox::default();
    let start = Instant::now();
    assert_eq!(
        timebox.call(async |_| Ok::<_, &str>("ok"), minimum).await,
        Ok("ok")
    );
    assert!(start.elapsed() >= minimum);
    let start = Instant::now();
    assert_eq!(
        timebox.call(async |_| Err::<(), _>("error"), minimum).await,
        Err("error")
    );
    assert!(start.elapsed() >= minimum);

    let fake = Sleep::fake();
    let mut timebox = Timebox::default().using(&fake);
    timebox
        .call(
            async |_| Sleep::r#for(minimum).wait().await,
            Duration::from_millis(1),
        )
        .await;
    fake.assert_never_slept();
}

#[tokio::test]
async fn timebox_early_return_persists_until_reset() {
    let fake = Sleep::fake();
    let minimum = Duration::from_secs(1);
    let mut timebox = Timebox::default().using(&fake);
    timebox
        .call(
            async |control| {
                control.return_early();
            },
            minimum,
        )
        .await;
    timebox.call(async |_| (), minimum).await;
    fake.assert_never_slept();
    timebox
        .call(
            async |control| {
                control.dont_return_early();
            },
            minimum,
        )
        .await;
    fake.assert_slept(
        |duration| *duration > Duration::ZERO && *duration <= minimum,
        1,
    );
}

#[tokio::test]
async fn conditional_wait_rechecks_the_predicate_and_yields() {
    let mut attempts = 0;
    Sleep::r#for(Duration::ZERO)
        .r#while(|| {
            attempts += 1;
            attempts < 3
        })
        .await;
    assert_eq!(attempts, 3);
}
