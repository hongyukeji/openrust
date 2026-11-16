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
