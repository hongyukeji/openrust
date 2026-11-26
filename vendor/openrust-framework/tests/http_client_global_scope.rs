use http::{HeaderValue, StatusCode};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
fn factory() -> Factory {
    let factory = Factory::new().unwrap();
    factory.fake(|request| {
        Some(Response::from_json(
            StatusCode::OK,
            &json!({"global":request.headers().contains_key("x-global")}),
        ))
    });
    factory.global_request_middleware(|request| {
        request
            .headers_mut()
            .insert("x-global", HeaderValue::from_static("yes"));
        Ok(())
    });
    factory
}
#[tokio::test]
async fn global_bypass_is_factory_local_keeps_fake_and_restores_nested_and_panicking_scopes() {
    let first = factory();
    let second = factory();
    let pending = first.without_global_configuration(|http| {
        let pending = http.pending_request();
        let another = second.pending_request();
        (pending, another)
    });
    assert_eq!(
        pending
            .0
            .get("https://fixture.test/")
            .await
            .unwrap()
            .json::<Value>()
            .unwrap()["global"],
        false
    );
    assert_eq!(
        pending
            .1
            .get("https://fixture.test/")
            .await
            .unwrap()
            .json::<Value>()
            .unwrap()["global"],
        true
    );
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        first.without_global_configuration(|_| panic!("restore"))
    }));
    assert_eq!(
        first
            .get("https://fixture.test/")
            .await
            .unwrap()
            .json::<Value>()
            .unwrap()["global"],
        true
    );
    assert_eq!(first.recorded().len(), 2);
}
#[tokio::test]
async fn asynchronous_bypass_does_not_leak_to_other_tasks_or_after_cancellation() {
    let factory = Arc::new(factory());
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    let copy = factory.clone();
    let task = tokio::spawn(async move {
        copy.without_global_configuration_async(async {
            assert_eq!(
                copy.get("https://fixture.test/")
                    .await
                    .unwrap()
                    .json::<Value>()
                    .unwrap()["global"],
                false
            );
            signal.notify_one();
            std::future::pending::<()>().await;
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    assert_eq!(
        factory
            .get("https://fixture.test/")
            .await
            .unwrap()
            .json::<Value>()
            .unwrap()["global"],
        true
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        factory
            .get("https://fixture.test/")
            .await
            .unwrap()
            .json::<Value>()
            .unwrap()["global"],
        true
    );
}
