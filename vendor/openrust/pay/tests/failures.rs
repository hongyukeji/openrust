mod support;
use openrust::{
    config::Repository,
    http::client::{Factory, Response},
};
use openrust_pay::{Error, PayManager};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use support::*;

#[tokio::test]
async fn signed_rejections_are_distinct_from_unsigned_or_invalid_responses_and_keys_are_explicitly_trusted()
 {
    for name in ["wechat", "alipay"] {
        for signed in [true, false] {
            let client = Arc::new(Factory::new().unwrap());
            let driver_name = name.to_owned();
            client.fake(move |_| {
                if driver_name == "wechat" {
                    let body = br#"{"code":"PARAM_ERROR","message":"private upstream details"}"#;
                    let headers = if signed {
                        signed_headers(body, 0)
                    } else {
                        http::HeaderMap::new()
                    };
                    Some(Ok(Response::new(
                        http::StatusCode::BAD_REQUEST,
                        headers,
                        body.to_vec(),
                    )))
                } else {
                    let payload = r#"{ "code":"40004", "sub_msg":"private upstream details" }"#;
                    let signature = if signed {
                        sign(PLATFORM, payload.as_bytes())
                    } else {
                        "invalid".to_owned()
                    };
                    let body = format!(
                        "{{\"alipay_trade_query_response\":{payload},\"sign\":{}}}",
                        serde_json::to_string(&signature).unwrap()
                    );
                    Some(Ok(Response::new(
                        http::StatusCode::OK,
                        http::HeaderMap::new(),
                        body.into_bytes(),
                    )))
                }
            });
            let config = Arc::new(Repository::new(BTreeMap::from([(
                "pay".into(),
                json!({"default":"main","drivers":{"main":profile(name,"https://example.test/")}}),
            )])));
            let manager = PayManager::new(config, client.clone());
            let error = manager
                .driver("main")
                .unwrap()
                .find("order_001")
                .await
                .unwrap_err();
            assert!(if signed {
                matches!(error, Error::Rejected)
            } else {
                matches!(error, Error::OutcomeUnknown)
            });
            assert!(!format!("{error:?} {error}").contains("private upstream details"));
            assert_eq!(client.recorded().len(), 1);
        }
    }
    use openrust::contracts::config::Repository as _;
    let (config, manager) = manager("wechat", "https://example.test/");
    config
        .set("pay.drivers.main.public_key_id", json!("PUB_KEY_ID_NEW"))
        .unwrap();
    config
        .set(
            "pay.drivers.main.public_keys",
            json!({"PUB_KEY_ID_123":PUBLIC}),
        )
        .unwrap();
    let (headers, body) = wechat_callback(json!({}));
    manager
        .driver("main")
        .unwrap()
        .callback(&headers, &body)
        .unwrap();
    config
        .set("pay.drivers.main.public_keys", json!({}))
        .unwrap();
    manager.forgetDrivers();
    assert!(
        manager
            .driver("main")
            .unwrap()
            .callback(&headers, &body)
            .is_err()
    );
    for endpoint in [
        "http://merchant.example/",
        "https://name:password@example.test/",
        "https://example.test/?token=secret",
        "https://example.test/#fragment",
    ] {
        config
            .set("pay.drivers.main.endpoint", json!(endpoint))
            .unwrap();
        manager.forgetDrivers();
        assert!(manager.driver("main").is_err());
    }
}
