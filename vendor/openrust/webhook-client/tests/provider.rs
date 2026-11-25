use openrust::{
    contracts::{config::Repository, foundation::Application as _},
    foundation::Application,
};
use openrust_webhook_client::{
    WebhookClientServiceProvider, WebhookConfig, WebhookConfigRepository,
};
use serde_json::json;
#[tokio::test]
async fn provider_is_lazy_scoped_and_keeps_credential_references_out_of_configuration() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let env_name = format!("WEBHOOK_TEST_{}", fastrand_id());
    std::fs::write(
        a.path().join(".env"),
        format!("{env_name}=first-private-secret\n"),
    )
    .unwrap();
    std::fs::write(
        b.path().join(".env"),
        format!("{env_name}=second-private-secret\n"),
    )
    .unwrap();
    let create = |root: std::path::PathBuf| {
        Application::configure(root)
            .with_providers(vec![Box::new(WebhookClientServiceProvider)])
            .create()
            .unwrap()
    };
    let mut first = create(a.path().into());
    let mut second = create(b.path().into());
    first.boot().await.unwrap();
    second.boot().await.unwrap();
    let config = first.get::<dyn Repository>("config").await.unwrap();
    let value = json!({"configs":[{"name":"one","signing_secret":{"env":env_name}},{"name":"one","revision":"two","signing_secret":{"env":env_name}}]});
    config.set("webhook-client", value.clone()).unwrap();
    second
        .get::<dyn Repository>("config")
        .await
        .unwrap()
        .set("webhook-client", value)
        .unwrap();
    let a = first.make::<WebhookConfigRepository>().await.unwrap();
    let b = second.make::<WebhookConfigRepository>().await.unwrap();
    assert_eq!(
        a.get_config("one").unwrap().unwrap().signing_secret(),
        "first-private-secret"
    );
    assert_eq!(a.get_config("one").unwrap().unwrap().revision(), "two");
    assert_eq!(
        b.get_config("one").unwrap().unwrap().signing_secret(),
        "second-private-secret"
    );
    assert!(
        !config
            .get("webhook-client")
            .unwrap()
            .to_string()
            .contains("first-private-secret")
    );
    a.add_config(WebhookConfig::new("runtime", "injected").unwrap());
    assert!(b.get_config("runtime").unwrap().is_none());
    let old = first
        .container()
        .scope(async {
            let one = first.make::<WebhookConfigRepository>().await.unwrap();
            one.add_config(WebhookConfig::new("request-only", "private").unwrap());
            assert!(std::sync::Arc::ptr_eq(
                &one,
                &first.make::<WebhookConfigRepository>().await.unwrap()
            ));
            one
        })
        .await;
    first
        .container()
        .scope(async {
            let next = first.make::<WebhookConfigRepository>().await.unwrap();
            assert!(!std::sync::Arc::ptr_eq(&old, &next));
            assert!(next.get_config("request-only").unwrap().is_none());
        })
        .await;
    let env = first.make::<openrust::config::Environment>().await.unwrap();
    for (policy, enabled) in [
        (json!(null), true),
        (json!(true), true),
        (json!(false), false),
    ] {
        let config = WebhookConfig::from_config(
            &json!({"name":"attachments","store_attachments":policy}),
            &env,
        )
        .unwrap();
        assert_eq!(config.store_attachments().unwrap(), enabled);
        assert_eq!(
            config
                .with_store_attachments(!enabled)
                .store_attachments()
                .unwrap(),
            !enabled
        );
    }
    assert!(
        WebhookConfig::from_config(
            &json!({"name":"attachments","store_attachments":"false"}),
            &env,
        )
        .is_err()
    );
    for policy in [
        json!(["*"]),
        json!(true),
        json!(["bad\nheader"]),
        json!(vec!["x"; 129]),
    ] {
        assert!(
            WebhookConfig::from_config(&json!({"name":"headers","store_headers":policy}), &env)
                .is_err()
        );
    }
    for policy in [json!("*"), json!(["X-Source"]), json!([]), json!(null)] {
        assert!(
            WebhookConfig::from_config(&json!({"name":"headers","store_headers":policy}), &env)
                .is_ok()
        );
    }
    assert!(
        WebhookConfig::from_config(
            &json!({"name":"bad","signing_secret":"unsafe-inline"}),
            &env
        )
        .is_err()
    );
    assert!(
        WebhookConfig::from_config(
            &json!({"name":"bad","signing_secret":{"env":"NO_SUCH_WEBHOOK_SECRET_422"}}),
            &env
        )
        .is_err()
    );
}
fn fastrand_id() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
