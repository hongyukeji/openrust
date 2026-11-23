use std::sync::{Arc, Mutex};

use axum::{Router, extract::State, response::IntoResponse};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::HeaderMap;
use openrust::{
    contracts::config::Repository, foundation::Application, notifications::sms::SmsManager,
};
use serde_json::json;

type Requests = Arc<Mutex<Vec<(String, String, String)>>>;

async fn receive(
    State(requests): State<Requests>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let form: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(form["to"], "12025550123");
    if form["text"] == "second" {
        assert!(!form.contains_key("client-ref"));
        assert!(!form.contains_key("callback"));
    } else {
        assert_eq!(form["client-ref"], "profile-ref");
        assert_eq!(form["callback"], "https://example.test/sms-status");
    }
    requests.lock().unwrap().push((
        headers["authorization"].to_str().unwrap().into(),
        form["from"].clone(),
        form["text"].clone(),
    ));
    (
        [("content-type", "application/json")],
        json!({"messages":[{"status":"0","message-id":"fixture-id"}]}).to_string(),
    )
}

#[tokio::test]
async fn vonage_profiles_own_credentials_and_refresh_with_the_manager() {
    let requests = Requests::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/send", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/send", axum::routing::post(receive))
        .with_state(requests.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.get::<dyn Repository>("config").await.unwrap();
    config.set("vonage", json!({"api_key":"wrong-global-key","api_secret":"wrong-global-secret","sms_from":"Wrong"})).unwrap();
    config.set("sms", json!({"default":"alpha","drivers":{
        "alpha":{"driver":"vonage","api_key":"key-a","api_secret":"secret-a","from":"BrandA","endpoint":endpoint,"client_reference":"profile-ref","status_callback":"https://example.test/sms-status"},
        "beta":{"driver":"vonage","api_key":"key-b","api_secret":"secret-b","from":"BrandB","endpoint":endpoint}
    }})).unwrap();
    let channels = app
        .make::<openrust::notifications::ChannelManager>()
        .await
        .unwrap();
    assert!(channels.channel(Some("sms")).is_ok());
    assert!(channels.channel(Some("vonage")).is_err());
    let manager = app.make::<SmsManager>().await.unwrap();
    let first = manager.driver("alpha").unwrap();
    let second = manager.driver("beta").unwrap();
    app.call(
        openrust::container::Callback::new(&[], |_| async {
            first.send_text("+12025550123", "first").await.unwrap();
            second.send_text("+12025550123", "second").await.unwrap();
            config
                .set("sms.drivers.alpha.api_secret", json!("rotated-a"))
                .unwrap();
            config
                .set("sms.drivers.alpha.from", json!("BrandNew"))
                .unwrap();
            first.send_text("+12025550123", "snapshot").await.unwrap();
            manager.forget_drivers();
            manager
                .driver("alpha")
                .unwrap()
                .send_text("+12025550123", "fresh")
                .await
                .unwrap();
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    let basic = |credentials: &str| format!("Basic {}", STANDARD.encode(credentials));
    assert_eq!(
        *requests.lock().unwrap(),
        vec![
            (basic("key-a:secret-a"), "BrandA".into(), "first".into()),
            (basic("key-b:secret-b"), "BrandB".into(), "second".into()),
            (basic("key-a:secret-a"), "BrandA".into(), "snapshot".into()),
            (basic("key-a:rotated-a"), "BrandNew".into(), "fresh".into()),
        ]
    );
    server.abort();
}
