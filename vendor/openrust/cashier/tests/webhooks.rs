mod support;
use openrust_cashier::{Client, Error, Provider};
use serde_json::json;
use std::sync::Arc;
use support::*;
#[tokio::test]
async fn raw_body_hmac_timestamp_multisignatures_rotation_and_account_isolation() {
    for provider in [Provider::Stripe, Provider::Paddle] {
        let server = Server::new(provider).await;
        let client = server.client();
        let kind = if provider == Provider::Stripe {
            "customer.subscription.updated"
        } else {
            "subscription.updated"
        };
        let (h, b) = event(&client, "evt_1", kind, subscription(provider, "active"), 0);
        let e = client.verifyWebhook(&h, &b).unwrap();
        assert_eq!(e.eventType(), kind);
        assert_eq!(e.id(), "evt_1");
        assert_eq!(e.success().status(), http::StatusCode::OK);
        let mut wrong = b.clone();
        wrong.push(b' ');
        assert!(matches!(
            client.verifyWebhook(&h, &wrong),
            Err(Error::Verification)
        ));
        for offset in [-1000, 1000] {
            let (h, b) = event(&client, "evt_1", kind, json!({"id":"sub_1"}), offset);
            assert!(client.verifyWebhook(&h, &b).is_err());
        }
        let name = if provider == Provider::Stripe {
            "stripe-signature"
        } else {
            "paddle-signature"
        };
        let mut duplicate = h.clone();
        duplicate.append(name, h[name].clone());
        assert!(client.verifyWebhook(&duplicate, &b).is_err());
        let mut mixed = h.clone();
        let value = format!(
            "{}{}{}={}",
            h[name].to_str().unwrap(),
            if provider == Provider::Stripe {
                ","
            } else {
                ";"
            },
            if provider == Provider::Stripe {
                "v1"
            } else {
                "h1"
            },
            "0".repeat(64)
        );
        mixed.insert(name, http::HeaderValue::from_str(&value).unwrap());
        assert!(client.verifyWebhook(&mixed, &b).is_ok());
        let mut malformed = h.clone();
        let value = format!(
            "{}{}{}={}",
            h[name].to_str().unwrap(),
            if provider == Provider::Stripe {
                ","
            } else {
                ";"
            },
            if provider == Provider::Stripe {
                "v1"
            } else {
                "h1"
            },
            "é".repeat(32)
        );
        malformed.insert(name, http::HeaderValue::from_str(&value).unwrap());
        assert!(client.verifyWebhook(&malformed, &b).is_err());
        let mut p = profile(provider, &server.url);
        p["webhook_secrets"] = json!(["new_secret", SECRET]);
        let rotated = Client::new(
            "test",
            &p,
            Arc::new(openrust::http::client::Factory::new().unwrap()),
        )
        .unwrap();
        assert!(rotated.verifyWebhook(&h, &b).is_ok());
        let mut p = profile(provider, &server.url);
        p["webhook_secret"] = json!("other_secret");
        let other = Client::new(
            "other",
            &p,
            Arc::new(openrust::http::client::Factory::new().unwrap()),
        )
        .unwrap();
        assert!(other.verifyWebhook(&h, &b).is_err());
        if provider == Provider::Stripe {
            let mut v: serde_json::Value = serde_json::from_slice(&b).unwrap();
            for (k, x) in [
                ("livemode", json!(true)),
                ("account", json!("acct_attacker")),
            ] {
                v[k] = x;
                let b = serde_json::to_vec(&v).unwrap();
                assert!(
                    client
                        .verifyWebhook(&headers(provider, &b, 0, SECRET), &b)
                        .is_err()
                );
                v = serde_json::from_slice(&b).unwrap();
            }
        }
    }
}
#[tokio::test]
async fn named_account_configuration_is_lazy_snapshotted_and_explicitly_reloaded() {
    use openrust::{config::Repository, contracts::config::Repository as _, http::client::Factory};
    use openrust_cashier::CashierManager;
    use std::collections::BTreeMap;
    let server = Server::new(Provider::Stripe).await;
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "cashier".into(),
        json!({"default":"main","drivers":{"main":profile(Provider::Stripe,&server.url)}}),
    )])));
    let manager = CashierManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    assert_eq!(manager.get_default_driver().unwrap(), "main");
    let old = manager.driver("main").unwrap();
    config
        .set("cashier.drivers.main.secret", json!("rotated_api_key"))
        .unwrap();
    manager
        .driver("main")
        .unwrap()
        .retrieve("customers", "cus_1")
        .await
        .unwrap();
    assert_eq!(
        server.requests().last().unwrap().headers["authorization"],
        "Bearer api_secret_fixture"
    );
    manager.forget_drivers();
    manager
        .driver("main")
        .unwrap()
        .retrieve("customers", "cus_1")
        .await
        .unwrap();
    assert_eq!(
        server.requests().last().unwrap().headers["authorization"],
        "Bearer rotated_api_key"
    );
    old.retrieve("customers", "cus_1").await.unwrap();
    assert_eq!(
        server.requests().last().unwrap().headers["authorization"],
        "Bearer api_secret_fixture"
    );
    for endpoint in [
        "http://outside.test",
        "https://u:p@api.stripe.com",
        "https://api.stripe.com/evil",
        "https://api.stripe.com/?q=x",
    ] {
        let mut p = profile(Provider::Stripe, endpoint);
        assert!(Client::new("x", &p, Arc::new(Factory::new().unwrap())).is_err());
        p["secret"] = json!("");
        assert!(Client::new("x", &p, Arc::new(Factory::new().unwrap())).is_err());
    }
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.method == http::Method::GET)
    );
}

/// Run the compiled binary immediately after regenerating timestamps with reference/generate.php.
#[test]
#[ignore = "requires fresh signatures from fixed external official PHP SDK source"]
fn official_sdk_signatures_validate_without_reencoding_the_body() {
    let path = std::env::var("CASHIER_REFERENCE_JSON").expect("CASHIER_REFERENCE_JSON is required");
    let reference: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for provider in [Provider::Stripe, Provider::Paddle] {
        let client = Client::new(
            "reference",
            &profile(provider, "https://example.test"),
            Arc::new(openrust::http::client::Factory::new().unwrap()),
        )
        .unwrap();
        let r = &reference[provider.as_str()];
        let body = r["body"].as_str().unwrap().as_bytes();
        let mut h = http::HeaderMap::new();
        h.insert(
            if provider == Provider::Stripe {
                "stripe-signature"
            } else {
                "paddle-signature"
            },
            http::HeaderValue::from_str(r["header"].as_str().unwrap()).unwrap(),
        );
        assert_eq!(
            client.verifyWebhook(&h, body).unwrap().id(),
            "evt_reference"
        );
        let mut changed = body.to_vec();
        changed.push(b' ');
        assert!(client.verifyWebhook(&h, &changed).is_err());
    }
}
