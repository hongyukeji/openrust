use std::sync::{Arc, Mutex};

use openrust::pipeline::{Pipeline, PipelinePipe};

#[tokio::test]
async fn executes_in_nested_order_and_transforms_the_destination() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = Arc::clone(&calls);
    let second = Arc::clone(&calls);
    let finally = Arc::clone(&calls);
    let pipes: Vec<PipelinePipe<i32, String, &'static str>> = vec![
        Box::new(move |value, next| {
            Box::pin(async move {
                first.lock().unwrap().push("first in");
                let result = next.run(value + 1).await?;
                first.lock().unwrap().push("first out");
                Ok(result)
            })
        }),
        Box::new(move |value, next| {
            Box::pin(async move {
                second.lock().unwrap().push("second in");
                let result = next.run(value * 2).await?;
                second.lock().unwrap().push("second out");
                Ok(result)
            })
        }),
    ];
    let result = Pipeline::<_, _, String>::send(20)
        .pipe(|_, _| Box::pin(async { panic!("through replaces previous stages") }))
        .through(pipes)
        .finally(move |original| {
            assert_eq!(original, 20);
            finally.lock().unwrap().push("finally");
        })
        .then(|value| Ok(value.to_string()))
        .await;
    assert_eq!(result, Ok("42".to_string()));
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "first in",
            "second in",
            "second out",
            "first out",
            "finally"
        ]
    );
}

#[tokio::test]
async fn short_circuit_and_error_run_the_finalizer() {
    for expected in [Ok(42), Err("short")] {
        let observed = Arc::new(Mutex::new(None));
        let copy = Arc::clone(&observed);
        let result = Pipeline::<_, &'static str>::send(1)
            .pipe(move |_, _| Box::pin(async move { expected }))
            .finally(move |original| *copy.lock().unwrap() = Some(original))
            .then(|_| panic!("short-circuited destination was called"))
            .await;
        assert_eq!(result, expected);
        assert_eq!(*observed.lock().unwrap(), Some(1));
    }
}

#[tokio::test]
async fn finally_runs_once_when_execution_panics_or_is_cancelled() {
    use futures_util::FutureExt as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    for panic in [false, true] {
        let count = Arc::new(AtomicUsize::new(0));
        let copy = count.clone();
        let pipeline = Pipeline::<_, &'static str>::send(7)
            .pipe(move |_, _| {
                Box::pin(async move {
                    if panic {
                        panic!("受控管道异常");
                    }
                    std::future::pending::<Result<i32, &'static str>>().await
                })
            })
            .finally(move |value| {
                assert_eq!(value, 7);
                copy.fetch_add(1, Ordering::SeqCst);
            });
        let future = std::panic::AssertUnwindSafe(pipeline.then_return()).catch_unwind();
        let result = tokio::time::timeout(std::time::Duration::from_millis(5), future).await;
        if panic {
            assert!(result.unwrap().is_err());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
