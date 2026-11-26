use openrust::{
    auth::{AuthError, AuthManager},
    contracts::{auth::Factory as _, container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{Body, Request},
};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn request() -> Request {
    let (parts, body) = http::Request::new(Body::empty()).into_parts();
    Request::from_parts(parts, body)
}
async fn application() -> Application {
    let mut app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            Ok([(
                "auth".into(),
                json!({"defaults":{"guard":"first"},"guards":{
                    "first":{"driver":"configured","provider":"one"},
                    "second":{"driver":"configured","provider":"two"},
                    "legacy":{"driver":"legacy"}
                }}),
            )]
            .into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn configured_extensions_receive_each_guard_and_cache_only_within_its_request() {
    let app = application().await;
    let manager = app.container().make::<AuthManager>().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observations = seen.clone();
    manager.extend("configured", move |_, name, config| {
        observations
            .lock()
            .unwrap()
            .push((name.to_owned(), config["provider"].clone()));
        Box::pin(async { Ok(None) })
    });
    manager.via_request("legacy", |_| Box::pin(async { Ok(None) }));
    let mut first = request();
    for name in ["first", "second", "first", "second", "legacy"] {
        assert!(
            manager
                .guard(&mut first, Some(name))
                .unwrap()
                .user()
                .await
                .unwrap()
                .is_none()
        );
    }
    let mut second = request();
    assert!(
        manager
            .guard(&mut second, Some("first"))
            .unwrap()
            .user()
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            ("first".into(), json!("one")),
            ("second".into(), json!("two")),
            ("first".into(), json!("one")),
        ]
    );
    let other = application().await;
    assert!(
        other
            .container()
            .make::<AuthManager>()
            .unwrap()
            .guard(&mut request(), Some("first"))
            .is_err()
    );
}

#[tokio::test]
async fn failed_configured_resolution_can_retry_without_poisoning_other_guards() {
    let app = application().await;
    let manager = app.container().make::<AuthManager>().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = attempts.clone();
    manager.extend("configured", move |_, _, _| {
        let attempt = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Box::pin(async move {
            if attempt == 0 {
                Err(AuthError::InvalidIdentity)
            } else {
                Ok(None)
            }
        })
    });
    let mut request = request();
    assert!(
        manager
            .guard(&mut request, Some("first"))
            .unwrap()
            .user()
            .await
            .is_err()
    );
    assert!(
        manager
            .guard(&mut request, Some("second"))
            .unwrap()
            .user()
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        manager
            .guard(&mut request, Some("first"))
            .unwrap()
            .user()
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        manager
            .guard(&mut request, Some("first"))
            .unwrap()
            .user()
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 3);
}
