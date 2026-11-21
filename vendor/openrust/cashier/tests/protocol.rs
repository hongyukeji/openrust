mod support;
use openrust_cashier::{Error, Price, Proration, Provider};
use serde_json::json;
use support::*;
#[tokio::test]
async fn stripe_checkout_trials_discounts_sca_refunds_and_customer_portal_use_official_wire_contracts()
 {
    let server = Server::new(Provider::Stripe).await;
    let db = Db::new(false).await;
    let b = db.billable(server.client()).await;
    let checkout=b.newSubscription("default",vec![Price::new("price_basic",2).unwrap()]).unwrap().trialDays(14).unwrap().withCoupon("coupon_1").unwrap().checkout(json!({"success_url":"https://shop.test/success?session_id={CHECKOUT_SESSION_ID}","cancel_url":"https://shop.test/cancel"}),"checkout:1").await.unwrap();
    assert_eq!(checkout["id"], "cs_1");
    let r = server.requests();
    let c = r
        .iter()
        .find(|r| r.path == "/v1/checkout/sessions")
        .unwrap();
    assert_eq!(
        c.headers["stripe-version"],
        openrust_cashier::STRIPE_VERSION
    );
    assert_eq!(c.headers["idempotency-key"].len(), 64);
    assert_ne!(c.headers["idempotency-key"], "checkout:1");
    assert_eq!(c.headers["authorization"], "Bearer api_secret_fixture");
    for (k, v) in [
        ("customer", "cus_1"),
        ("mode", "subscription"),
        ("line_items[0][price]", "price_basic"),
        ("line_items[0][quantity]", "2"),
        ("subscription_data[trial_period_days]", "14"),
        ("subscription_data[metadata][cashier_type]", "default"),
        ("discounts[0][coupon]", "coupon_1"),
    ] {
        assert_eq!(c.params[k], v, "{k}");
    }
    assert!(
        c.params
            .get("subscription_data[discounts][0][coupon]")
            .is_none()
    );
    let payment = b
        .pay(
            1234,
            "USD",
            json!({"automatic_payment_methods":{"enabled":true}}),
            "payment:1",
        )
        .await
        .unwrap();
    assert_eq!(payment["status"], "requires_action");
    assert_eq!(payment["client_secret"], "pi_client_secret_fixture");
    b.confirmPayment("pi_1", json!({}), "confirm:1")
        .await
        .unwrap();
    b.refund("pi_1", json!({"amount":200}), "refund:1")
        .await
        .unwrap();
    b.billingPortal("https://shop.test/account", "portal:1")
        .await
        .unwrap();
    let r = server.requests();
    let refund = r.iter().find(|r| r.path == "/v1/refunds").unwrap();
    assert_eq!(refund.params["payment_intent"], "pi_1");
    assert_eq!(refund.params["amount"], "200");
    assert!(matches!(
        b.pay(0, "USD", json!({}), "no").await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        b.checkout(
            vec![Price::new("price_basic", 1).unwrap()],
            json!({"success_url":"http://unsafe.test"}),
            "no"
        )
        .await,
        Err(Error::InvalidInput(_))
    ));
}
#[tokio::test]
async fn stripe_subscription_items_proration_trial_cancel_resume_and_immediate_cancel() {
    let server = Server::new(Provider::Stripe).await;
    let db = Db::new(false).await;
    let b = db.billable(server.client()).await;
    let s = b
        .newSubscription("default", vec![Price::new("price_basic", 1).unwrap()])
        .unwrap()
        .create(Some("pm_1"), "subscribe:1")
        .await
        .unwrap();
    assert_eq!(
        s.asStripeSubscription(json!({})).await.unwrap()["id"],
        "sub_1"
    );
    assert!(matches!(
        s.asPaddleSubscription(json!({})).await,
        Err(Error::Unsupported)
    ));
    assert!(s.active());
    assert!(b.subscribed("default").await.unwrap());
    let s = s
        .updateQuantity(3, None, Proration::Invoice, "quantity:1")
        .await
        .unwrap();
    assert_eq!(s.items[0].quantity, Some(3));
    let r = server.requests();
    let q = r
        .iter()
        .find(|r| r.params["proration_behavior"] == "always_invoice")
        .unwrap();
    assert_eq!(q.params["items[0][id]"], "si_1");
    assert_eq!(q.params["items[0][quantity]"], "3");
    s.swap(
        vec![Price::new("price_new", 1).unwrap()],
        Proration::None,
        "swap:1",
    )
    .await
    .unwrap();
    let r = server.requests();
    let swap = r
        .iter()
        .find(|r| r.params["items[0][price]"] == "price_new")
        .unwrap();
    assert_eq!(swap.params["items[1][id]"], "si_1");
    assert_eq!(swap.params["items[1][deleted]"], "true");
    let s = s.cancel("cancel:1").await.unwrap();
    assert!(s.canceled() && s.onGracePeriod());
    let s = s.resume("resume:1").await.unwrap();
    assert!(!s.canceled());
    s.extendTrial(jiff::Timestamp::now().as_second() + 86400, "trial:1")
        .await
        .unwrap();
    s.pauseCollection("void", "pause-collection:1")
        .await
        .unwrap();
    s.resumeCollection("resume-collection:1").await.unwrap();
    let s = s.cancelNow("cancel-now:1").await.unwrap();
    assert!(!s.valid());
    assert!(matches!(
        s.resume("invalid").await,
        Err(Error::InvalidInput(_))
    ));
}
#[tokio::test]
async fn stripe_payment_methods_invoices_tax_balance_metered_usage_and_customer_ownership() {
    let server = Server::new(Provider::Stripe).await;
    let db = Db::new(false).await;
    let b = db.billable(server.client()).await;
    b.createOrGetCustomer(json!({"email":"ignored@existing.test"}), "existing")
        .await
        .unwrap();
    assert!(!server.requests().iter().any(|r| r.path == "/v1/customers"));
    b.updateCustomer(json!({"name":"用户"}), "update")
        .await
        .unwrap();
    b.createSetupIntent(json!({}), "setup").await.unwrap();
    b.paymentMethods("card").await.unwrap();
    b.addPaymentMethod("pm_1", "attach").await.unwrap();
    b.updateDefaultPaymentMethod("pm_1", "default")
        .await
        .unwrap();
    assert!(b.defaultPaymentMethod().await.unwrap().is_some());
    b.deletePaymentMethod("pm_1", "detach").await.unwrap();
    let n = server.requests().len();
    assert!(matches!(
        b.deletePaymentMethod("pm_other", "no").await,
        Err(Error::Ownership)
    ));
    assert_eq!(server.requests().len(), n + 1);
    b.invoiceItem(
        200,
        "JPY",
        json!({"description":"zero-decimal currency"}),
        "item",
    )
    .await
    .unwrap();
    b.createInvoice(json!({"automatic_tax":{"enabled":true}}), "invoice")
        .await
        .unwrap();
    b.previewInvoice(json!({}), "preview").await.unwrap();
    b.finalizeInvoice("in_1", "finalize").await.unwrap();
    b.payInvoice("in_1", json!({}), "pay-invoice")
        .await
        .unwrap();
    b.voidInvoice("in_1", "void").await.unwrap();
    b.invoices(json!({"limit":2,"starting_after":"in_previous"}))
        .await
        .unwrap();
    assert_eq!(
        b.invoiceDocument("in_1").await.unwrap()["invoice_pdf"],
        "https://files.stripe.com/invoice.pdf"
    );
    b.taxIds().await.unwrap();
    b.createTaxId("eu_vat", "DE123456789", "tax").await.unwrap();
    b.deleteTaxId("txi_1", "del-tax").await.unwrap();
    assert_eq!(b.balance().await.unwrap(), -300);
    b.balanceTransactions(json!({"limit":2})).await.unwrap();
    b.applyBalance(-200, "USD", "credit", "credit")
        .await
        .unwrap();
    b.reportUsage("tokens", 42, "usage_unique", None, "usage")
        .await
        .unwrap();
    let r = server.requests();
    let meter = r
        .iter()
        .find(|r| r.path == "/v1/billing/meter_events")
        .unwrap();
    assert_eq!(meter.params["payload[stripe_customer_id]"], "cus_1");
    assert_eq!(meter.params["payload[value]"], "42");
    let n = r.len();
    assert!(matches!(
        b.client()
            .api(http::Method::POST, "/v1/customers", &json!({}), None)
            .await,
        Err(Error::InvalidInput(_))
    ));
    for path in [
        "//attacker.test",
        "/v1/../customers",
        "/v1/customers%2fpi_1",
        "/v1/customers?api_key=secret",
    ] {
        assert!(matches!(
            b.client()
                .api(http::Method::GET, path, &json!({}), None)
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(server.requests().len(), n);
}
#[tokio::test]
async fn paddle_hosted_checkout_pause_resume_proration_refund_documents_and_durable_mutation_keys()
{
    let server = Server::new(Provider::Paddle).await;
    let db = Db::new(false).await;
    let client = server.client();
    let b = db.billable(client.clone()).await;
    let checkout = b
        .newSubscription("default", vec![Price::new("price_basic", 2).unwrap()])
        .unwrap()
        .withCoupon("dsc_1")
        .unwrap()
        .checkout(json!({}), "checkout:1")
        .await
        .unwrap();
    assert_eq!(checkout["id"], "txn_1");
    let r = server.requests();
    let c = r.iter().find(|r| r.path == "/transactions").unwrap();
    assert_eq!(c.params["customer_id"], "cus_1");
    assert_eq!(c.params["custom_data"]["cashier_type"], "default");
    assert_eq!(c.params["items"][0]["quantity"], 2);
    assert_eq!(c.params["discount_id"], "dsc_1");
    assert!(!c.headers.contains_key("idempotency-key"));
    assert_eq!(c.headers["paddle-version"], "1");
    assert!(matches!(
        b.checkout(
            vec![Price::new("price_basic", 2).unwrap()],
            json!({"discount_id":"dsc_1","custom_data":{"cashier_type":"default"}}),
            "checkout:1"
        )
        .await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path == "/transactions")
            .count(),
        1
    );
    let s = b.syncSubscription("sub_1").await.unwrap();
    let s = s
        .updateQuantity(4, None, Proration::Invoice, "quantity")
        .await
        .unwrap();
    assert_eq!(s.items[0].quantity, Some(4));
    let s = s.pause(true, None, "pause").await.unwrap();
    assert!(s.paused());
    let s = s.resume("resume").await.unwrap();
    assert!(s.active());
    let s = s.cancel("cancel").await.unwrap();
    assert!(s.onGracePeriod());
    let s = s.resume("resume-cancel").await.unwrap();
    assert!(!s.canceled());
    s.preview(json!({"items":[{"price_id":"price_basic","quantity":2}],"proration_billing_mode":"prorated_immediately"}),"preview").await.unwrap();
    s.charge(vec![Price::new("price_one_time", 1).unwrap()], "charge")
        .await
        .unwrap();
    s.activate("activate").await.unwrap();
    s.cancelNow("cancel-now").await.unwrap();
    b.billingPortal("https://shop.test/account", "portal")
        .await
        .unwrap();
    b.refund("txn_1",json!({"type":"partial","reason":"customer request","items":[{"item_id":"txnitm_1","type":"partial","amount":"200"}]}),"refund").await.unwrap();
    b.invoices(json!({"per_page":2})).await.unwrap();
    b.invoiceDocument("txn_1").await.unwrap();
    b.addresses().await.unwrap();
    b.createAddress(
        json!({"country_code":"US","postal_code":"10001"}),
        "address",
    )
    .await
    .unwrap();
    assert!(matches!(
        b.createSetupIntent(json!({}), "unsupported").await,
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        b.newSubscription("x", vec![Price::new("pri_1", 1).unwrap()])
            .unwrap()
            .trialDays(14),
        Err(Error::Unsupported)
    ));
    assert_eq!(
        db.store
            .operationStatus(&client, "checkout:1")
            .await
            .unwrap()
            .as_deref(),
        Some("done")
    );
}
#[tokio::test]
async fn failures_do_not_retry_or_expose_secrets_and_paddle_unknown_keys_never_resend() {
    for provider in [Provider::Stripe, Provider::Paddle] {
        let server = Server::new(provider).await;
        let db = Db::new(false).await;
        let client = server.client();
        let b = db.billable(client.clone()).await;
        server.state.lock().unwrap().failure = Some(503);
        let e = if provider == Provider::Stripe {
            b.pay(1, "usd", json!({}), "unknown").await
        } else {
            b.checkout(vec![Price::new("pri_1", 1).unwrap()], json!({}), "unknown")
                .await
        }
        .unwrap_err();
        assert!(matches!(e, Error::OutcomeUnknown));
        assert!(!format!("{e:?} {e}").contains("api_secret_fixture"));
        if provider == Provider::Paddle {
            let count = server.requests().len();
            assert!(matches!(
                b.checkout(vec![Price::new("pri_1", 1).unwrap()], json!({}), "unknown")
                    .await,
                Err(Error::OutcomeUnknown)
            ));
            assert_eq!(server.requests().len(), count);
            assert_eq!(
                db.store
                    .operationStatus(&client, "unknown")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("unknown")
            );
        }
        server.state.lock().unwrap().failure = Some(400);
        let e = db
            .store
            .execute(
                &client,
                http::Method::POST,
                if provider == Provider::Stripe {
                    "/v1/customers"
                } else {
                    "/customers"
                },
                &json!({}),
                "rejected",
            )
            .await
            .unwrap_err();
        assert!(matches!(e, Error::Rejected(400)));
        assert!(!format!("{client:?}").contains("api_secret_fixture"));
    }
}
#[tokio::test]
async fn real_transport_timeout_is_unknown_and_sends_only_one_mutation() {
    let server = Server::new(Provider::Paddle).await;
    let db = Db::new(false).await;
    let b = db.billable(server.client()).await;
    server.state.lock().unwrap().delay = true;
    assert!(matches!(
        b.checkout(vec![Price::new("pri_1", 1).unwrap()], json!({}), "timeout")
            .await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path == "/transactions")
            .count(),
        1
    );
    let n = server.requests().len();
    assert!(matches!(
        b.checkout(vec![Price::new("pri_1", 1).unwrap()], json!({}), "timeout")
            .await,
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(server.requests().len(), n);
}
#[tokio::test]
async fn http_recording_redacts_authorization_keys_and_request_response_payloads() {
    use std::sync::Arc;
    let server = Server::new(Provider::Stripe).await;
    let http = Arc::new(openrust::http::client::Factory::new().unwrap());
    http.fake(|_| None);
    let client = openrust_cashier::Client::new(
        "recording",
        &profile(Provider::Stripe, &server.url),
        http.clone(),
    )
    .unwrap();
    client
        .api(
            http::Method::POST,
            "/v1/payment_intents",
            &json!({"amount":1234,"currency":"usd"}),
            Some("private-operation"),
        )
        .await
        .unwrap();
    assert_eq!(http.recorded().len(), 1);
    for (request, response) in http.recorded() {
        assert!(request.body.is_none());
        assert!(!request.headers.contains_key("authorization"));
        assert_ne!(
            request
                .headers
                .get("idempotency-key")
                .and_then(|v| v.to_str().ok()),
            Some("private-operation")
        );
        assert!(response.unwrap().body().is_empty());
    }
}

#[tokio::test]
async fn stripe_original_customer_and_portal_names_enforce_provider_semantics() {
    let db = Db::new(true).await;
    let stripe = Server::new(Provider::Stripe).await;
    let b = openrust_cashier::Billable::new(stripe.client(), db.store.clone(), "original-customer")
        .unwrap();
    assert!(!b.hasStripeId().await.unwrap());
    assert_eq!(
        b.createAsStripeCustomer(json!({"name":"Agent"}), "create-original")
            .await
            .unwrap()["id"],
        "cus_1"
    );
    assert!(b.hasStripeId().await.unwrap());
    assert!(matches!(
        b.createAsStripeCustomer(json!({}), "duplicate").await,
        Err(Error::CustomerAlreadyCreated)
    ));
    b.createOrGetStripeCustomer(json!({"expand":["tax"]}), "read-existing")
        .await
        .unwrap();
    assert_eq!(stripe.requests().last().unwrap().params["expand[0]"], "tax");
    b.updateStripeCustomer(json!({"name":"Updated"}), "update-original")
        .await
        .unwrap();
    b.updateOrCreateStripeCustomer(json!({"email":"user@example.test"}), "upsert-original")
        .await
        .unwrap();
    assert_eq!(b.asStripeCustomer(&[]).await.unwrap()["id"], "cus_1");
    assert_eq!(
        b.billingPortalUrl(
            "https://shop.test/account",
            json!({"locale":"en"}),
            "url-original"
        )
        .await
        .unwrap(),
        "https://billing.example/portal"
    );
    let response = b
        .redirectToBillingPortal("https://shop.test/account", json!({}), "redirect-original")
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::FOUND);
    assert_eq!(
        response.headers()[http::header::LOCATION],
        "https://billing.example/portal"
    );
    stripe.state.lock().unwrap().failure = Some(401);
    assert!(matches!(
        b.updateStripeCustomer(json!({}), "rejected-original").await,
        Err(Error::Rejected(401))
    ));
    let paddle = Server::new(Provider::Paddle).await;
    let p = openrust_cashier::Billable::new(paddle.client(), db.store.clone(), "paddle-original")
        .unwrap();
    assert!(matches!(
        p.createOrGetStripeCustomer(json!({}), "wrong-provider")
            .await,
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        p.billingPortalUrl("https://shop.test/account", json!({}), "wrong-portal")
            .await,
        Err(Error::Unsupported)
    ));
    assert!(paddle.requests().is_empty());
    assert!(matches!(
        p.createAsCustomer(json!({}), "missing-email").await,
        Err(Error::InvalidInput(_))
    ));
    assert!(paddle.requests().is_empty());
    assert!(matches!(
        b.createAsCustomer(json!({}), "wrong-provider").await,
        Err(Error::Unsupported)
    ));
    p.createAsCustomer(json!({"email":"paddle@example.test"}), "paddle-create")
        .await
        .unwrap();
    assert_eq!(
        p.createAsCustomer(json!({}), "paddle-existing")
            .await
            .unwrap()["id"],
        "cus_1"
    );
}
