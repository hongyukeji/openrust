# Cashier (Stripe)

## Installation and configuration {#configuration}

`openrust-cashier` integrates Stripe and [Paddle Billing](cashier-paddle.md): customers, Checkout, subscriptions, invoices and durable webhooks. Use the separate [payment extension](pay.md) for WeChat Pay and Alipay. Both packages can be enabled together; the domestic CNY amount type is not used for international currencies.

Enable the root application's `cashier` Cargo feature. Other applications add `openrust-cashier` and use OpenRust package discovery. `CashierServiceProvider` binds `CashierManager` and registers the compiled migration without contacting the provider or querying billing tables during boot. Run native `artisan migrate` to create `cashier_customers`, `cashier_subscriptions`, `cashier_invoices`, `cashier_events` and `cashier_operations`.

To customize the compiled configuration, run `artisan vendor:publish --tag=cashier-config`. The package publishes `config/cashier.rs`; repeated publishing preserves application edits unless `--force` is supplied. Add `mod cashier;` to your application `config/mod.rs`, insert `("cashier".into(), cashier::load(env)?)` in its configuration groups, then rebuild the application. Environment defaults work without publishing.

```dotenv
CASHIER_DRIVER=stripe
CASHIER_LIVE=false
STRIPE_SECRET=sk_test_...
STRIPE_WEBHOOK_SECRET=whsec_...
```

The Stripe API is pinned to `2026-11-18.endive`; configure the webhook endpoint with the same version. Use `CASHIER_LIVE=true` and production credentials in production. Keep API and webhook secrets on the server. Stripe.js uses a public publishable key, never `STRIPE_SECRET`.

The package loads `cashier.default`, Stripe `cashier.key`, `cashier.secret`, `cashier.webhook.secret`, and Paddle `cashier.paddle.api_key`, `cashier.paddle.webhook_secret`, `cashier.paddle.sandbox`. The Paddle namespace separates the two upstream packages within one Rust crate. `cashier.drivers` contains explicit named accounts; a matching account overrides its canonical provider configuration. Your application configuration loader can override the `cashier` group: application values take precedence and `drivers` is replaced as a whole. Each named profile specifies `driver`, `secret`, `webhook_secret` and `live`; `webhook_secrets` supports secret rotation. A profile name identifies one stable merchant account and mode. Use another name or database for a different merchant or for production instead of reassigning an existing profile.

`Cashier::driver("stripe")` and `Cashier::default_driver()` resolve clients in application scope. `CashierManager::get_default_driver()` returns the configured name. `forget_drivers()` reloads configuration on subsequent resolutions; existing clients retain their credential snapshots. Cached configuration remains authoritative; rebuild it after changing environment settings as described in [configuration](../getting-started/configuration.md).

## Account-specific HTTP middleware {#account-http}

From your application Provider, resolve `CashierManager` and call `set_http_factory(account, Some(factory))` with an `Arc<openrust::http::client::Factory>` dedicated to the named account. Use that factory to attach trusted request middleware or test doubles. It can see outgoing billing requests, so register only server-controlled code and keep credentials and payloads out of logs. A subsequent resolution uses the new factory. Existing clients keep their previous factory, and `forget_drivers()` reloads account configuration while retaining the override. Pass `None` to restore the default factory. Register the override in every HTTP, Artisan and Worker process that uses the account.

This extension point applies to Stripe and Paddle transport. Cashier continues to own provider versions, request encoding, signature verification, ownership checks, and transaction and idempotency rules.

## Customers and Checkout {#customers-and-checkout}

`Billable` binds a trusted, authenticated application identity. Do not infer ownership from an email, webhook field or Checkout query string. Profile names and local identities isolate customer mappings; a remote customer can be associated only once.

Stripe customer APIs retain `stripe_id`, `has_stripe_id`, `create_as_stripe_customer`, `create_or_get_stripe_customer`, `update_stripe_customer`, `update_or_create_stripe_customer` and `as_stripe_customer(&["tax"])`. Creation rejects an existing customer with `CustomerAlreadyCreated`; create-or-get retrieves the existing customer and honors `expand`. `billing_portal_url(return_url, options, key)` returns a validated HTTPS URL; `redirect_to_billing_portal` returns an HTTP 302 response. Stripe-only methods reject Paddle accounts before sending a request. Existing `create_or_get_customer`, `update_customer` and `billing_portal` remain explicit cross-provider conveniences.

`Billable` is a trusted identity adapter rather than a PHP model trait. Provide customer name/email/address in `options`; no reflection reads application model fields. Provider objects remain JSON values, while local subscription/invoice records are typed. `subscription()` returns `SubscriptionHandle`, which owns operations and dereferences the stored `Subscription`; `as_stripe_subscription` and `as_paddle_subscription` retain their provider boundaries. `raw_balance()` returns Stripe minor units; the existing integer `balance()` is a convenience alias, without PHP localized currency formatting. Mutation keys are explicit Rust arguments to retain retry safety.

Use the existing `Billable` and `BillingStore` APIs for billing operations and verified webhooks. Billing and webhook services coordinate remote calls and business decisions; storage keeps operation claims, account locks and durable writes. Remote reads and the application SQL handler retain their existing transaction order, so a rollback keeps the event retryable.

`subscription(name)` chooses within the current account and owner. A single candidate whose status is neither `canceled`, `cancelled` nor `incomplete_expired` takes precedence; multiple such candidates return an error, so use `subscription_by_id`. When only terminal candidates remain, the greatest `(created_at, id)` wins. An empty set returns `None`; malformed stored payloads propagate an error.

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("stripe")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.create_or_get_stripe_customer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let session = user.new_subscription("default", vec![Price::new("price_monthly", 1)?])?
    .trial_days(14)?
    .with_coupon("coupon_welcome")?
    .checkout(json!({
        "success_url":"https://shop.example/success?session_id={CHECKOUT_SESSION_ID}",
        "cancel_url":"https://shop.example/account"
    }), "subscription:42:first").await?;
```

Redirect to the returned `url`. A browser redirect is not proof of payment: use committed, signature-verified webhooks to synchronize payment and subscription status. `checkout(prices, options, key)` creates one-time Checkout sessions; use `new_subscription(...).checkout(...)` for subscriptions. Select catalog prices on the server instead of accepting arbitrary prices, amounts or customer IDs from the browser.

`create_or_get_customer` returns the associated customer or creates one; concurrent calls must reuse the same stable operation key. `attach_customer(id)` is an explicit administrative association after checking ownership, including recovery of an unknown creation outcome. Never pass an unchecked request parameter to it. `update_customer` updates details; `billing_portal(return_url, key)` creates a Stripe customer portal session.

## Subscription lifecycle {#subscriptions}

`new_subscription(name, prices)` accepts multiple prices, `Price::new(id, quantity)`, `Price::metered(id)`, `trial_days`, `trial_until`, `skip_trial`, `quantity`, `anchor_billing_cycle_on`, `with_metadata`, `with_coupon` and `with_options`. Direct `create(payment_method, key)` uses Stripe's `default_incomplete` behavior and requires a supplied payment method to belong to this customer.

```rust
use openrust_cashier::Proration;

let subscription = user.subscription("default").await?.ok_or("missing subscription")?;
let subscription = subscription.update_quantity(3, None, Proration::Invoice, "seats:42:v2").await?;
let subscription = subscription.cancel("cancel:42:v1").await?;
if subscription.on_grace_period() {
    subscription.resume("resume:42:v1").await?;
}
```

`swap(prices, proration, key)` replaces the full price set, removes old items and retains matching item IDs. `Proration::Prorate` adds prorations to the next invoice, `Invoice` invoices immediately and `None` disables proration. Specify the price when changing quantities on a subscription with multiple prices. `cancel` schedules cancellation at period end, `cancel_now` cancels immediately, and `resume` resumes an eligible grace-period or Stripe-paused subscription. `extend_trial` extends a trial. `pause_collection` / `resume_collection` control Stripe collection, which is distinct from a paused subscription status.

`subscription(name)` returns the current named subscription, or the latest terminated record if none remain. Multiple nonterminal subscriptions with the same name fail explicitly; use `subscription_by_id(id)` to disambiguate. `sync_subscription(id)` verifies customer ownership and reconciles remote state; `refresh()` refreshes a handle. `subscribed`, `valid`, `active`, `on_trial`, `canceled`, `on_grace_period`, `paused`, `past_due` and `incomplete` support entitlement decisions. Incomplete and past-due subscriptions do not grant valid access by default.

`as_provider_subscription(parameters)` retrieves the native subscription after checking ownership. For example, `{"expand":["latest_invoice.confirmation_secret"]}` can obtain confirmation details for a directly created subscription. Pass the applicable client secret only to that customer's Stripe.js flow. Never log or return it across customers.

## Payments, invoices and tax {#payments-and-invoices}

`create_setup_intent`, `payment_methods`, `add_payment_method`, `default_payment_method`, `update_default_payment_method` and `delete_payment_method` manage Stripe payment methods. Collect card details with Stripe.js Payment Element; server methods accept Stripe identifiers, not card numbers or CVCs.

`pay(amount, currency, options, key)` creates a PaymentIntent; `charge` confirms an off-session payment with a stored method; `confirm_payment` confirms an existing Intent. Amounts are positive integers in the currency's minor unit, such as USD cents or whole JPY, never floating point. `requires_action`, `requires_payment_method` and `processing` are not successful payments: complete authentication through Stripe.js and await webhooks. `refund(payment_intent_id, options, key)` supports partial amounts; acceptance is not settlement.

`invoices(parameters)` preserves provider pagination, including `limit` / `starting_after`. `find_invoice` and `invoice_document` retrieve owned invoices and provider PDF / hosted URLs. `invoice_item`, `create_invoice`, `finalize_invoice`, `pay_invoice` and `void_invoice` support manual invoices. `preview_invoice` and subscription `preview` preview changes; `local_invoices` returns the minimal webhook projection.

`tax_ids`, `create_tax_id` and `delete_tax_id` manage tax identifiers. `balance` returns the unformatted balance in integer minor units, `balance_transactions` lists its ledger, and `apply_balance` adds a credit or debit. `report_usage(event_name, value, identifier, timestamp, key)` reports Stripe Billing Meter Events; use stable unique event identifiers. Pass tax rates, automatic tax, discounts and billing anchors through `with_options`, `update`, or the relevant Checkout / invoice options. Construct options on the server instead of forwarding a complete HTTP request.

## Webhooks and transactions {#webhooks}

Configure a fixed HTTPS endpoint for each named account. Subscribe to Stripe `customer.subscription.*`, `invoice.*` and application-specific PaymentIntent or refund events. Exclude this route from browser-form CSRF checks and authenticate it through its signature. Verify the original bytes without decoding and re-encoding JSON:

```rust
let event = client.verify_webhook(&headers, &raw_body)?;
store.process(&client, &event, |tx, event| Box::pin(async move {
    // Update business records or a transactional outbox using tx.
    // currentSubscription() / currentInvoice() contain the reconciled current projection.
    // data() is the original event and may be stale; use current state for entitlements.
    let _ = (tx, event);
    Ok(())
})).await?;
let response = event.success(); // HTTP 200 only after commit or confirmed duplicate.
```

`handle_webhook` synchronizes without additional application SQL. `process` deduplicates the event, updates subscription / invoice projections, runs application SQL and marks completion in one transaction. Errors and cancellation roll back, allowing provider redelivery. A reused event ID with changed content produces `EventConflict`. Fetching current provider state under the customer lock prevents late notifications from regressing state; provider API failure must cause a failed response so delivery can retry.

Unmapped subscription / invoice customers are rejected. Associate a trusted customer before opening Checkout. Dashboard subscriptions must use an associated customer; `metadata.cashier_type` names the subscription and defaults to `default`. Other verified events are available to your handler, without automatically treating them as payment or fulfillment success. Handlers must perform only transactional SQL. Use an application transactional outbox for shipping or email; do not initiate billing mutations or recursively process the same event inside the handler.

Stripe `customer.deleted` is reconciled against the provider, disables the mapping and cancels local subscription projections atomically. Handlers can use `customer_deleted()` to revoke business access. Re-association after deletion requires an explicitly verified new customer through `attach_customer`; old customer events remain rejected.

Stripe HMAC verification uses a 300-second window and rejects future timestamps outside that window, duplicate signature headers, wrong mode and Connect account events. Multiple trusted secrets support rotation. This integration targets direct Stripe accounts; Connect platform onboarding and split payments require a dedicated integration.

## Errors and native API {#errors-and-api}

Use a stable business operation key for every mutation. Stripe receives a stable hash of the account scope and key as `Idempotency-Key`. Requests are never automatically retried. Transport failures, timeouts, redirects, 5xx, 429 and malformed successful responses produce `OutcomeUnknown`: reconcile the original operation or safely reuse its Stripe key with the identical request. Other explicit HTTP refusals produce `Rejected(status)` without secrets or raw provider error bodies.

HTTP requests disable redirects, use a 15-second timeout and limit responses to 1 MiB. Observations omit payloads, authorization and raw business operation keys. `Client::api(method, path, parameters, key)` exposes a general server-side REST entry point: Stripe paths use `/v1/...`, nested form encoding and query encoding for GET, preserving pagination. This advanced entry point does not check local ownership or synchronize projections; applications must do so. Prefer identity-bound `Billable` methods for user actions.

This Rust package does not implement PHP Traits, Blade components or PHP method-for-method compatibility. Use hosted provider pages / official JS SDKs and provider invoice PDFs. Production requires merchant access, product permissions, catalog prices and a reachable webhook endpoint.
