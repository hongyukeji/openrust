use openrust::{contracts::config::Repository, foundation::Application};
use openrust_webhook_server::WebhookServerServiceProvider;
#[tokio::test]
async fn provider_merges_fixed_defaults_without_database_or_network_access() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_providers(vec![Box::new(WebhookServerServiceProvider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.get::<dyn Repository>("config").await.unwrap();
    assert_eq!(config.get("webhook-server.http_verb").unwrap(), "post");
    assert_eq!(
        config.get("webhook-server.headers.Content-Type").unwrap(),
        "application/json"
    );
    assert_eq!(config.get("webhook-server.timeout_in_seconds").unwrap(), 3);
    assert_eq!(config.get("webhook-server.tries").unwrap(), 3);
    assert_eq!(config.get("webhook-server.verify_ssl").unwrap(), true);
    assert!(
        config
            .get("webhook-server.retryable_status_codes")
            .unwrap()
            .is_null()
    );
}
