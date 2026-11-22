use openrust::support::{Lottery, LotteryChance};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn integer_odds_probabilities_and_callback_values_follow_the_selected_branch() {
    assert!(Lottery::odds(1, 0).is_err());
    assert!(Lottery::odds(1.5, None::<u64>).is_err());
    assert!(Lottery::odds(f64::NAN, None::<u64>).is_err());
    assert!(Lottery::odds(2, 1).unwrap().choose());
    assert!(!Lottery::odds(-1, 1).unwrap().choose());
    assert!(Lottery::odds(1, None::<u64>).unwrap().choose());
    assert!(!Lottery::odds(-1, None::<u64>).unwrap().choose());
    assert!(Lottery::odds(u64::MAX, u64::MAX).unwrap().choose());
    let wins = Lottery::odds(1, 1)
        .unwrap()
        .winner(|| "winner")
        .loser(|| "loser");
    let losses = Lottery::odds(0, 1)
        .unwrap()
        .winner(|| "winner")
        .loser(|| "loser");
    assert_eq!(wins.choose(), "winner");
    assert_eq!(losses.chooseTimes(3), ["loser"; 3]);
    assert!(wins.chooseTimes(0).is_empty());
}

#[test]
fn sequences_restore_normal_sampling_and_support_a_custom_missing_handler() {
    Lottery::scopeSync(|| {
        let lottery = Lottery::odds(0, 1).unwrap();
        Lottery::fix([true, false, true]);
        assert_eq!(lottery.chooseTimes(5), [true, false, true, false, false]);
        let missing = Arc::new(AtomicUsize::new(0));
        let counter = missing.clone();
        Lottery::forceResultWithSequence(
            [false],
            Some(Arc::new(move |chances, out_of| {
                assert!(matches!(chances, LotteryChance::Integer(0)));
                assert_eq!(out_of, Some(1));
                counter.fetch_add(1, Ordering::Relaxed);
                true
            })),
        );
        assert_eq!(lottery.chooseTimes(3), [false, true, true]);
        assert_eq!(missing.load(Ordering::Relaxed), 2);
        Lottery::determineResultsNormally();
        assert!(!lottery.choose());
        Lottery::setResultFactory(|_, _| true);
        assert!(lottery.choose());
        Lottery::determineResultNormally();
        assert!(!lottery.choose());
    });
}

#[test]
fn nested_scopes_restore_controls_even_after_a_callback_panics() {
    Lottery::scopeSync(|| {
        Lottery::alwaysWin();
        let result = std::panic::catch_unwind(|| {
            Lottery::scopeSync(|| {
                Lottery::alwaysLose();
                assert!(!Lottery::odds(1, 1).unwrap().choose());
                panic!("模拟回调失败");
            })
        });
        assert!(result.is_err());
        assert!(Lottery::odds(0, 1).unwrap().choose());
        Lottery::determineResultNormally();
        assert!(!Lottery::odds(0, 1).unwrap().choose());
    });
    assert!(!Lottery::odds(0, 1).unwrap().choose());
}

#[tokio::test]
async fn async_controls_remain_task_local_across_yields_and_cancellation() {
    assert!(std::panic::catch_unwind(Lottery::alwaysWin).is_err());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let left_barrier = barrier.clone();
    let left = tokio::spawn(Lottery::scope(async move {
        Lottery::alwaysWin();
        left_barrier.wait().await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            assert!(Lottery::odds(0, 1).unwrap().choose());
        }
    }));
    let right = tokio::spawn(Lottery::scope(async move {
        Lottery::alwaysLose();
        barrier.wait().await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            assert!(!Lottery::odds(1, 1).unwrap().choose());
        }
    }));
    left.await.unwrap();
    right.await.unwrap();
    let (ready, started) = tokio::sync::oneshot::channel();
    let cancelled = tokio::spawn(Lottery::scope(async move {
        Lottery::alwaysWin();
        ready.send(()).unwrap();
        std::future::pending::<()>().await;
    }));
    started.await.unwrap();
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    assert!(!Lottery::odds(0, 1).unwrap().choose());
}

#[test]
fn concurrent_runtime_threads_keep_independent_result_factories() {
    let barrier = Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|threads| {
        for win in [false, true] {
            let barrier = barrier.clone();
            threads.spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap()
                    .block_on(Lottery::scope(async move {
                        if win {
                            Lottery::alwaysWin();
                        } else {
                            Lottery::alwaysLose();
                        }
                        barrier.wait();
                        tokio::task::yield_now().await;
                        assert_eq!(Lottery::odds(1, 2).unwrap().choose(), win);
                    }));
            });
        }
    });
}
