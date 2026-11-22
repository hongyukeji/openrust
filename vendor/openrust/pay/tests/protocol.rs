mod support;
use openrust_pay::{Amount, Error, Payment, Refund, Scene};
use serde_json::json;
use support::*;

#[test]
fn exact_amounts_identifiers_and_debug_do_not_expose_payloads() {
    for (yuan, fen) in [("12.34", 1234), ("12", 1200), ("0.01", 1), ("1.2", 120)] {
        assert_eq!(Amount::fromYuan(yuan).unwrap().fen(), fen);
    }
    for invalid in [
        "0",
        "0.00",
        "-1",
        "1e2",
        "1.234",
        "1.",
        "NaN",
        " 1.00",
        "10000000000000000000",
        "+1",
    ] {
        assert!(Amount::fromYuan(invalid).is_err(), "{invalid}");
    }
    for invalid in [-1, 0, i64::MAX] {
        assert!(Amount::fromFen(invalid).is_err());
    }
    assert_eq!(
        format!("{:?}", Payment::App("secret".into())),
        "Payment([REDACTED])"
    );
}
#[tokio::test]
async fn wechat_real_http_all_scenes_query_close_refund_and_client_signatures() {
    let server = Server::new().await;
    let (_, manager) = manager("wechat", &server.endpoint);
    let driver = manager.driver("main").unwrap();
    let mut o = order();
    o.openid = Some("openid-private".into());
    o.client_ip = Some("192.0.2.1".parse().unwrap());
    assert!(matches!(
        driver.pay(Scene::Scan, &o).await.unwrap(),
        Payment::QrCode(_)
    ));
    if let Payment::Jsapi(v) = driver.pay(Scene::Jsapi, &o).await.unwrap() {
        assert_eq!(v["signType"], "RSA");
        verify(
            format!(
                "{}\n{}\n{}\n{}\n",
                v["appId"].as_str().unwrap(),
                v["timeStamp"].as_str().unwrap(),
                v["nonceStr"].as_str().unwrap(),
                v["package"].as_str().unwrap()
            )
            .as_bytes(),
            v["paySign"].as_str().unwrap(),
        );
    } else {
        panic!("JSAPI payload");
    }
    assert!(matches!(
        driver.pay(Scene::Wap, &o).await.unwrap(),
        Payment::Redirect(_)
    ));
    if let Payment::WechatApp(v) = driver.pay(Scene::App, &o).await.unwrap() {
        verify(
            format!(
                "{}\n{}\n{}\n{}\n",
                v["appid"].as_str().unwrap(),
                v["timestamp"].as_str().unwrap(),
                v["noncestr"].as_str().unwrap(),
                v["prepayid"].as_str().unwrap()
            )
            .as_bytes(),
            v["sign"].as_str().unwrap(),
        );
    } else {
        panic!("App payload");
    }
    assert_eq!(
        driver.find("order_001").await.unwrap()["trade_state"],
        "SUCCESS"
    );
    assert!(driver.close("order_001").await.unwrap().is_null());
    let refund = Refund {
        out_trade_no: "order_001".into(),
        out_refund_no: "refund_001".into(),
        total: o.amount,
        amount: Amount::fromFen(100).unwrap(),
        reason: Some("退款".into()),
    };
    driver.refund(&refund).await.unwrap();
    driver.findRefund("order_001", "refund_001").await.unwrap();
    assert_eq!(server.calls(), 8);
    let requests = server.state.requests.lock().unwrap();
    assert_eq!(requests[0].1["amount"]["total"], 1234);
    assert_eq!(requests[1].1["payer"]["openid"], "openid-private");
    assert_eq!(requests[2].1["scene_info"]["payer_client_ip"], "192.0.2.1");
    assert_eq!(requests[6].1["amount"]["refund"], 100);
}
#[tokio::test]
async fn alipay_real_http_all_scenes_raw_signed_json_and_refunds() {
    let server = Server::new().await;
    let (_, manager) = manager("alipay", &server.endpoint);
    let driver = manager.driver("main").unwrap();
    let mut o = order();
    o.return_url = Some("https://shop.example/return".into());
    assert!(matches!(
        driver.pay(Scene::Scan, &o).await.unwrap(),
        Payment::QrCode(_)
    ));
    for scene in [Scene::Web, Scene::Wap, Scene::App] {
        let query = match driver.pay(scene, &o).await.unwrap() {
            Payment::Redirect(url) => url::Url::parse(&url).unwrap().query().unwrap().to_owned(),
            Payment::App(query) => query,
            _ => panic!("Alipay payment"),
        };
        let params = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        verify(canonical(&params, false).as_bytes(), &params["sign"]);
        assert_eq!(params["return_url"], "https://shop.example/return");
        assert!(params["biz_content"].contains("12.34"));
    }
    driver.find("order_001").await.unwrap();
    driver.close("order_001").await.unwrap();
    driver
        .refund(&Refund {
            out_trade_no: "order_001".into(),
            out_refund_no: "refund_001".into(),
            total: o.amount,
            amount: Amount::fromFen(100).unwrap(),
            reason: None,
        })
        .await
        .unwrap();
    driver.findRefund("order_001", "refund_001").await.unwrap();
    assert_eq!(server.calls(), 5);
}
#[tokio::test]
async fn signature_errors_redirects_and_invalid_inputs_never_retry_or_return_payment_success() {
    for driver_name in ["wechat", "alipay"] {
        let server = Server::new().await;
        let (_, manager) = manager(driver_name, &server.endpoint);
        let driver = manager.driver("main").unwrap();
        for mode in [1, 2, 3] {
            if mode == 3 && driver_name == "alipay" {
                continue;
            }
            server.mode(mode);
            assert!(driver.find("order_001").await.is_err());
        }
        assert_eq!(server.calls(), if driver_name == "wechat" { 3 } else { 2 });
        let count = server.calls();
        let mut invalid = order();
        invalid.out_trade_no = "../escape".into();
        assert!(driver.pay(Scene::Scan, &invalid).await.is_err());
        invalid = order();
        invalid.notify_url = "http://unsafe.example".into();
        assert!(driver.pay(Scene::Scan, &invalid).await.is_err());
        assert!(driver.find("a?mchid=wrong").await.is_err());
        assert_eq!(server.calls(), count);
    }
}
#[test]
fn callbacks_verify_identity_signature_amount_currency_and_strict_form_decoding() {
    for name in ["wechat", "alipay"] {
        let (_, m) = manager(name, "https://example.test/");
        let g = m.driver("main").unwrap();
        let build = if name == "wechat" {
            wechat_callback
        } else {
            alipay_callback
        };
        let (h, b) = build(json!({}));
        let n = g.callback(&h, &b).unwrap();
        n.validate(&expected()).unwrap();
        assert_eq!(n.amount().fen(), 1234);
        assert_eq!(
            n.success().status(),
            if name == "wechat" { 204 } else { 200 }
        );
        assert!(!format!("{n:?}").contains("order_001"));
        let mut wrong = expected();
        wrong.amount = Amount::fromFen(1).unwrap();
        assert!(matches!(n.validate(&wrong), Err(Error::OrderMismatch)));
        let mut tampered = b.clone();
        tampered[0] ^= 1;
        assert!(g.callback(&h, &tampered).is_err());
        for fields in if name == "wechat" {
            vec![
                json!({"appid":"wrong"}),
                json!({"mchid":"wrong"}),
                json!({"trade_state":"NOTPAY"}),
                json!({"amount":{"total":1234,"currency":"USD"}}),
            ]
        } else {
            vec![
                json!({"app_id":"wrong"}),
                json!({"seller_id":"wrong"}),
                json!({"trade_status":"WAIT_BUYER_PAY"}),
                json!({"refund_fee":"1.00"}),
                json!({"sign_type":"RSA"}),
            ]
        } {
            let (h, b) = build(fields);
            assert!(g.callback(&h, &b).is_err());
        }
        if name == "wechat" {
            let mut expired = signed_headers(&b, -301);
            assert!(g.callback(&expired, &b).is_err());
            expired.insert(
                "wechatpay-serial",
                http::HeaderValue::from_static("PUB_KEY_ID_UNKNOWN"),
            );
            assert!(g.callback(&expired, &b).is_err());
            let mut duplicate = h.clone();
            duplicate.append("wechatpay-timestamp", http::HeaderValue::from_static("1"));
            assert!(g.callback(&duplicate, &b).is_err());
            let mut resource: serde_json::Value = serde_json::from_slice(&b).unwrap();
            resource["resource"]["ciphertext"] = json!("bad");
            let bad = serde_json::to_vec(&resource).unwrap();
            assert!(g.callback(&signed_headers(&bad, 0), &bad).is_err());
        } else {
            for extra in [
                b"&app_id=other".as_slice(),
                b"&bad=%ZZ",
                b"&bad=%FF",
                b"&bad=%",
            ] {
                let mut duplicate = b.clone();
                duplicate.extend_from_slice(extra);
                assert!(g.callback(&h, &duplicate).is_err());
            }
        }
    }
}
#[tokio::test]
async fn named_profiles_cache_keys_until_forget_and_keep_existing_instances_isolated() {
    use openrust::contracts::config::Repository as _;
    let server = Server::new().await;
    let (config, manager) = manager("wechat", &server.endpoint);
    let held = manager.driver("main").unwrap();
    config
        .set("pay.drivers.main.private_key", json!("invalid"))
        .unwrap();
    held.find("order_001").await.unwrap();
    manager
        .driver("main")
        .unwrap()
        .find("order_001")
        .await
        .unwrap();
    manager.forgetDrivers();
    assert!(manager.driver("main").is_err());
    held.find("order_001").await.unwrap();
    config
        .set("pay.drivers.main.private_key", json!(MERCHANT))
        .unwrap();
    assert!(manager.driver("main").is_ok());
    assert_eq!(manager.getDefaultDriver().unwrap(), "main");
    assert!(manager.driver("missing").is_err());
    config
        .set("pay.drivers.other", profile("alipay", &server.endpoint))
        .unwrap();
    config.set("pay.default", json!("other")).unwrap();
    assert_eq!(manager.getDefaultDriver().unwrap(), "other");
    manager
        .driver("other")
        .unwrap()
        .find("order_001")
        .await
        .unwrap();
    let (_, other) = support::manager("wechat", &server.endpoint);
    assert!(other.driver("missing").is_err());
    assert!(!format!("{held:?}").contains(MERCHANT));
}

#[test]
fn independent_php_openssl_signatures_and_gcm_ciphertext_match_the_rust_clients() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/php-reference.json")).unwrap();
    let (_, m) = manager("alipay", "https://example.test/");
    let gateway = m.driver("main").unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "content-type",
        http::HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    let n = gateway
        .callback(
            &headers,
            fixture["alipay_form"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
    n.validate(&expected()).unwrap();
    assert_eq!(n.transactionId(), "ali_reference");
    let (_, m) = manager("wechat", "https://example.test/");
    let gateway = m.driver("main").unwrap();
    let envelope=serde_json::to_vec(&json!({"id":"php-event","event_type":"TRANSACTION.SUCCESS","resource_type":"encrypt-resource","resource":fixture["wechat_resource"]})).unwrap();
    let n = gateway
        .callback(&signed_headers(&envelope, 0), &envelope)
        .unwrap();
    n.validate(&expected()).unwrap();
    assert_eq!(n.transactionId(), "wx_reference");
}

fn payment_routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::post("/payment-verify", verify_handler)?;
    Ok(())
}
fn verify_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let body = request.body_bytes().await?;
        let notice = openrust_pay::facades::Pay::defaultDriver()
            .and_then(|g| g.callback(request.headers(), &body))
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        notice
            .validate(&expected())
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        // This route verifies only. Production acknowledgement also requires committed business work.
        Ok(openrust::http::Response::new(
            http::StatusCode::NO_CONTENT,
            openrust::http::Body::empty(),
        ))
    })
}
#[tokio::test]
async fn provider_facade_http_callbacks_are_application_local_and_http_observations_are_redacted() {
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::client::Factory,
    };
    use std::{collections::BTreeMap, sync::Arc};
    let server = Server::new().await;
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "pay".into(),
                json!({"default":"main","drivers":{"main":profile("wechat","https://example.test/")}}),
            )]))
        })
        .withProviders(vec![Box::new(openrust_pay::PayServiceProvider)])
        .withRouting(Routing::default().api(payment_routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap()
        .set("pay.drivers.main.endpoint", json!(server.endpoint))
        .unwrap();
    let http = app.make::<Factory>().await.unwrap();
    http.fake(|_| None);
    let m = app.make::<openrust_pay::PayManager>().await.unwrap();
    m.driver("main")
        .unwrap()
        .pay(Scene::Scan, &order())
        .await
        .unwrap();
    assert_eq!(http.recorded().len(), 1);
    for (request, response) in http.recorded() {
        assert!(request.body.is_none());
        assert!(!request.headers.contains_key("authorization"));
        assert!(request.url.query().is_none());
        assert!(response.unwrap().body().unwrap().is_empty());
    }
    let config = app
        .get::<dyn openrust::contracts::config::Repository>("config")
        .await
        .unwrap();
    config
        .set("pay.drivers.main.private_key", json!("not a key"))
        .unwrap();
    // Existing gateway is a snapshot; forgetting forces validation on the next resolution.
    m.driver("main").unwrap();
    m.forgetDrivers();
    assert!(m.driver("main").is_err());
    config
        .set("pay.drivers.main.private_key", json!(MERCHANT))
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let (h, b) = wechat_callback(json!({}));
    for valid in [true, false] {
        let mut request = http::Request::builder()
            .method("POST")
            .uri("/api/payment-verify")
            .body(openrust::http::Body::from(b.clone()))
            .unwrap();
        *request.headers_mut() = h.clone();
        if !valid {
            request.headers_mut().insert(
                "wechatpay-serial",
                http::HeaderValue::from_static("PUB_KEY_ID_UNKNOWN"),
            );
        }
        let result = kernel.clone().handleTransport(request).await;
        assert_eq!(
            result.status(),
            if valid {
                http::StatusCode::NO_CONTENT
            } else {
                http::StatusCode::BAD_REQUEST
            }
        );
        assert!(!result.headers().contains_key("set-cookie"));
    }
    let mut other = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "pay".into(),
                json!({"default":"missing","drivers":{}}),
            )]))
        })
        .withProviders(vec![Box::new(openrust_pay::PayServiceProvider)])
        .create()
        .unwrap();
    other.boot().await.unwrap();
    assert!(
        other
            .make::<openrust_pay::PayManager>()
            .await
            .unwrap()
            .driver("main")
            .is_err()
    );
}

#[tokio::test]
async fn transport_timeout_reports_unknown_and_never_retries_an_order() {
    let server = Server::new().await;
    server.mode(4);
    let (_, manager) = manager("wechat", &server.endpoint);
    let g = manager.driver("main").unwrap();
    assert!(matches!(
        g.pay(Scene::Scan, &order()).await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(server.calls(), 1);
}
