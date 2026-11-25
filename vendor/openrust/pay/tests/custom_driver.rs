use aws_lc_rs::hmac;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, Response, StatusCode};
use openrust::{
    config::Repository, contracts::config::Repository as _, http::HttpFuture, http::client::Factory,
};
use openrust_pay::{
    Amount, DriverNotification, Error, ExpectedPayment, Order, PayDriver, PayManager, Payment,
    Refund, Result, Scene,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const DRIVER: &str = "fixture";
const BODY: &[u8] =
    br#"{"merchant":"fixture-main","order":"order-1","transaction":"remittance-1","fen":150}"#;

struct ExternalDriver {
    merchant: String,
    key: hmac::Key,
}

impl ExternalDriver {
    fn from_profile(profile: &Value) -> Result<Arc<dyn PayDriver>> {
        let merchant = profile["merchant"]
            .as_str()
            .ok_or(Error::Configuration("merchant"))?;
        let key = profile["key"].as_str().ok_or(Error::Configuration("key"))?;
        Ok(Arc::new(Self {
            merchant: merchant.into(),
            key: hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
        }))
    }
}

impl PayDriver for ExternalDriver {
    fn pay<'a>(&'a self, scene: Scene, order: &'a Order) -> HttpFuture<'a, Result<Payment>> {
        Box::pin(async move {
            if scene != Scene::Scan {
                return Err(Error::InvalidInput("unsupported fixture payment scene"));
            }
            Ok(Payment::QrCode(format!(
                "{}:{}",
                self.merchant, order.out_trade_no
            )))
        })
    }

    fn find<'a>(&'a self, order: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"merchant":self.merchant,"order":order})) })
    }

    fn close<'a>(&'a self, order: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"closed":order})) })
    }

    fn refund<'a>(&'a self, refund: &'a Refund) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"refund":refund.out_refund_no})) })
    }

    fn find_refund<'a>(&'a self, _: &'a str, _: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async { Err(Error::InvalidInput("unsupported fixture refund lookup")) })
    }

    fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<DriverNotification> {
        let signatures = headers.get_all("x-fixture-signature");
        let mut signatures = signatures.iter();
        let signed = signatures.next().ok_or(Error::Verification)?;
        if signatures.next().is_some() {
            return Err(Error::Verification);
        }
        let signed = STANDARD
            .decode(signed.as_bytes())
            .map_err(|_| Error::Verification)?;
        hmac::verify(&self.key, body, &signed).map_err(|_| Error::Verification)?;
        let payload: Value = serde_json::from_slice(body).map_err(|_| Error::Protocol)?;
        if payload["merchant"].as_str() != Some(self.merchant.as_str()) {
            return Err(Error::OrderMismatch);
        }
        DriverNotification::new(
            &self.merchant,
            payload["order"].as_str().ok_or(Error::Protocol)?,
            payload["transaction"].as_str().ok_or(Error::Protocol)?,
            Amount::from_fen(payload["fen"].as_i64().ok_or(Error::Protocol)?)?,
            Response::builder()
                .status(StatusCode::ACCEPTED)
                .header("content-type", "text/plain")
                .body("received".into())
                .unwrap(),
        )
    }
}

#[tokio::test]
async fn external_driver_is_profile_scoped_and_preserves_gateway_snapshots() {
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "pay".into(),
        json!({"default":"main","drivers":{
            "main":{"driver":DRIVER,"merchant":"fixture-main","key":"test-key"},
            "second":{"driver":DRIVER,"merchant":"fixture-second","key":"second-key"}
        }}),
    )])));
    let http = Arc::new(Factory::new().unwrap());
    let manager = PayManager::new(config.clone(), http.clone());
    let other_application = PayManager::new(config.clone(), http);
    assert!(matches!(
        manager.driver("main"),
        Err(Error::Configuration("driver"))
    ));
    assert!(
        manager
            .extend("alipay", ExternalDriver::from_profile)
            .is_err()
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let factory_calls = calls.clone();
    manager
        .extend(DRIVER, move |profile| {
            if factory_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::Configuration("fixture unavailable"));
            }
            ExternalDriver::from_profile(profile)
        })
        .unwrap();
    assert!(manager.driver("main").is_err());
    let gateway = manager
        .driver(&manager.get_default_driver().unwrap())
        .unwrap();
    assert!(manager.driver("main").is_ok());
    let second = manager.driver("second").unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(other_application.driver("main").is_err());

    let amount = Amount::from_fen(150).unwrap();
    let order = Order::new("order-1", "订单", amount, "https://shop.example/notify");
    assert!(
        matches!(gateway.pay(Scene::Scan, &order).await.unwrap(), Payment::QrCode(value) if value == "fixture-main:order-1")
    );
    assert!(gateway.pay(Scene::Web, &order).await.is_err());
    assert_eq!(
        gateway.find("order-1").await.unwrap()["merchant"],
        "fixture-main"
    );
    assert_eq!(
        second.find("order-1").await.unwrap()["merchant"],
        "fixture-second"
    );
    assert_eq!(gateway.close("order-1").await.unwrap()["closed"], "order-1");
    assert!(gateway.find("bad/order").await.is_err());
    let refund = Refund {
        out_trade_no: "order-1".into(),
        out_refund_no: "refund-1".into(),
        total: amount,
        amount,
        reason: None,
    };
    assert_eq!(gateway.refund(&refund).await.unwrap()["refund"], "refund-1");
    assert!(gateway.find_refund("order-1", "refund-1").await.is_err());

    let mut headers = HeaderMap::new();
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"test-key");
    headers.insert(
        "x-fixture-signature",
        STANDARD.encode(hmac::sign(&key, BODY)).parse().unwrap(),
    );
    let verified = gateway.callback(&headers, BODY).unwrap();
    verified
        .validate(&ExpectedPayment {
            out_trade_no: "order-1".into(),
            amount,
        })
        .unwrap();
    assert_eq!(verified.provider(), DRIVER);
    assert_eq!(verified.transaction_id(), "remittance-1");
    assert_eq!(verified.success().status(), StatusCode::ACCEPTED);
    assert_eq!(verified.success().body(), "received");
    assert!(matches!(
        gateway.callback(&headers, b"changed"),
        Err(Error::Verification)
    ));
    assert!(matches!(
        second.callback(&headers, BODY),
        Err(Error::Verification)
    ));

    manager
        .extend(DRIVER, ExternalDriver::from_profile)
        .unwrap();
    config
        .set("pay.drivers.main.merchant", json!("fixture-new"))
        .unwrap();
    manager.forget_drivers();
    assert_eq!(
        manager
            .driver("main")
            .unwrap()
            .find("order-1")
            .await
            .unwrap()["merchant"],
        "fixture-new"
    );
    assert_eq!(
        gateway.find("order-1").await.unwrap()["merchant"],
        "fixture-main"
    );
}
