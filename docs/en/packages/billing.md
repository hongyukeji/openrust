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

`Cashier::driver("stripe")` and `Cashier::defaultDriver()` resolve clients in application scope. `CashierManager::getDefaultDriver()` returns the configured name. `forgetDrivers()` reloads configuration on subsequent resolutions; existing clients retain their credential snapshots. Cached configuration remains authoritative; rebuild it after changing environment settings as described in [configuration](../getting-started/configuration.md).

## Customers and Checkout {#customers-and-checkout}

`Billable` binds a trusted, authenticated application identity. Do not infer ownership from an email, webhook field or Checkout query string. Profile names and local identities isolate customer mappings; a remote customer can be associated only once.

Stripe customer APIs retain `stripeId`, `hasStripeId`, `createAsStripeCustomer`, `createOrGetStripeCustomer`, `updateStripeCustomer`, `updateOrCreateStripeCustomer` and `asStripeCustomer(&["tax"])`. Creation rejects an existing customer with `CustomerAlreadyCreated`; create-or-get retrieves the existing customer and honors `expand`. `billingPortalUrl(return_url, options, key)` returns a validated HTTPS URL; `redirectToBillingPortal` returns an HTTP 302 response. Stripe-only methods reject Paddle accounts before sending a request. Existing `createOrGetCustomer`, `updateCustomer` and `billingPortal` remain explicit cross-provider conveniences.

`Billable` is a trusted identity adapter rather than a PHP model trait. Provide customer name/email/address in `options`; no reflection reads application model fields. Provider objects remain JSON values, while local subscription/invoice records are typed. `subscription()` returns `SubscriptionHandle`, which owns operations and dereferences the stored `Subscription`; `asStripeSubscription` and `asPaddleSubscription` retain their provider boundaries. `rawBalance()` returns Stripe minor units; the existing integer `balance()` is a convenience alias, without PHP localized currency formatting. Mutation keys are explicit Rust arguments to retain retry safety.

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("stripe")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.createOrGetStripeCustomer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let session = user.newSubscription("default", vec![Price::new("price_monthly", 1)?])?
    .trialDays(14)?
    .withCoupon("coupon_welcome")?
    .checkout(json!({
        "success_url":"https://shop.example/success?session_id={CHECKOUT_SESSION_ID}",
        "cancel_url":"https://shop.example/account"
    }), "subscription:42:first").await?;
```

Redirect to the returned `url`. A browser redirect is not proof of payment: use committed, signature-verified webhooks to synchronize payment and subscription status. `checkout(prices, options, key)` creates one-time Checkout sessions; use `newSubscription(...).checkout(...)` for subscriptions. Select catalog prices on the server instead of accepting arbitrary prices, amounts or customer IDs from the browser.

`createOrGetCustomer` returns the associated customer or creates one; concurrent calls must reuse the same stable operation key. `attachCustomer(id)` is an explicit administrative association after checking ownership, including recovery of an unknown creation outcome. Never pass an unchecked request parameter to it. `updateCustomer` updates details; `billingPortal(return_url, key)` creates a Stripe customer portal session.

## Subscription lifecycle {#subscriptions}

`newSubscription(name, prices)` accepts multiple prices, `Price::new(id, quantity)`, `Price::metered(id)`, `trialDays`, `trialUntil`, `skipTrial`, `quantity`, `anchorBillingCycleOn`, `withMetadata`, `withCoupon` and `withOptions`. Direct `create(payment_method, key)` uses Stripe's `default_incomplete` behavior and requires a supplied payment method to belong to this customer.

```rust
use openrust_cashier::Proration;

let subscription = user.subscription("default").await?.ok_or("missing subscription")?;
let subscription = subscription.updateQuantity(3, None, Proration::Invoice, "seats:42:v2").await?;
let subscription = subscription.cancel("cancel:42:v1").await?;
if subscription.onGracePeriod() {
    subscription.resume("resume:42:v1").await?;
}
```

`swap(prices, proration, key)` replaces the full price set, removes old items and retains matching item IDs. `Proration::Prorate` adds prorations to the next invoice, `Invoice` invoices immediately and `None` disables proration. Specify the price when changing quantities on a subscription with multiple prices. `cancel` schedules cancellation at period end, `cancelNow` cancels immediately, and `resume` resumes an eligible grace-period or Stripe-paused subscription. `extendTrial` extends a trial. `pauseCollection` / `resumeCollection` control Stripe collection, which is distinct from a paused subscription status.

`subscription(name)` returns the current named subscription, or the latest terminated record if none remain. Multiple nonterminal subscriptions with the same name fail explicitly; use `subscriptionById(id)` to disambiguate. `syncSubscription(id)` verifies customer ownership and reconciles remote state; `refresh()` refreshes a handle. `subscribed`, `valid`, `active`, `onTrial`, `canceled`, `onGracePeriod`, `paused`, `pastDue` and `incomplete` support entitlement decisions. Incomplete and past-due subscriptions do not grant valid access by default.

`asProviderSubscription(parameters)` retrieves the native subscription after checking ownership. For example, `{"expand":["latest_invoice.confirmation_secret"]}` can obtain confirmation details for a directly created subscription. Pass the applicable client secret only to that customer's Stripe.js flow. Never log or return it across customers.

## Payments, invoices and tax {#payments-and-invoices}

`createSetupIntent`, `paymentMethods`, `addPaymentMethod`, `defaultPaymentMethod`, `updateDefaultPaymentMethod` and `deletePaymentMethod` manage Stripe payment methods. Collect card details with Stripe.js Payment Element; server methods accept Stripe identifiers, not card numbers or CVCs.

`pay(amount, currency, options, key)` creates a PaymentIntent; `charge` confirms an off-session payment with a stored method; `confirmPayment` confirms an existing Intent. Amounts are positive integers in the currency's minor unit, such as USD cents or whole JPY, never floating point. `requires_action`, `requires_payment_method` and `processing` are not successful payments: complete authentication through Stripe.js and await webhooks. `refund(payment_intent_id, options, key)` supports partial amounts; acceptance is not settlement.

`invoices(parameters)` preserves provider pagination, including `limit` / `starting_after`. `findInvoice` and `invoiceDocument` retrieve owned invoices and provider PDF / hosted URLs. `invoiceItem`, `createInvoice`, `finalizeInvoice`, `payInvoice` and `voidInvoice` support manual invoices. `previewInvoice` and subscription `preview` preview changes; `localInvoices` returns the minimal webhook projection.

`taxIds`, `createTaxId` and `deleteTaxId` manage tax identifiers. `balance` returns the unformatted balance in integer minor units, `balanceTransactions` lists its ledger, and `applyBalance` adds a credit or debit. `reportUsage(event_name, value, identifier, timestamp, key)` reports Stripe Billing Meter Events; use stable unique event identifiers. Pass tax rates, automatic tax, discounts and billing anchors through `withOptions`, `update`, or the relevant Checkout / invoice options. Construct options on the server instead of forwarding a complete HTTP request.

## Webhooks and transactions {#webhooks}

Configure a fixed HTTPS endpoint for each named account. Subscribe to Stripe `customer.subscription.*`, `invoice.*` and application-specific PaymentIntent or refund events. Exclude this route from browser-form CSRF checks and authenticate it through its signature. Verify the original bytes without decoding and re-encoding JSON:

```rust
let event = client.verifyWebhook(&headers, &raw_body)?;
store.process(&client, &event, |tx, event| Box::pin(async move {
    // Update business records or a transactional outbox using tx.
    // currentSubscription() / currentInvoice() contain the reconciled current projection.
    // data() is the original event and may be stale; use current state for entitlements.
    let _ = (tx, event);
    Ok(())
})).await?;
let response = event.success(); // HTTP 200 only after commit or confirmed duplicate.
```

`handleWebhook` synchronizes without additional application SQL. `process` deduplicates the event, updates subscription / invoice projections, runs application SQL and marks completion in one transaction. Errors and cancellation roll back, allowing provider redelivery. A reused event ID with changed content produces `EventConflict`. Fetching current provider state under the customer lock prevents late notifications from regressing state; provider API failure must cause a failed response so delivery can retry.

Unmapped subscription / invoice customers are rejected. Associate a trusted customer before opening Checkout. Dashboard subscriptions must use an associated customer; `metadata.cashier_type` names the subscription and defaults to `default`. Other verified events are available to your handler, without automatically treating them as payment or fulfillment success. Handlers must perform only transactional SQL. Use an application transactional outbox for shipping or email; do not initiate billing mutations or recursively process the same event inside the handler.

Stripe `customer.deleted` is reconciled against the provider, disables the mapping and cancels local subscription projections atomically. Handlers can use `customerDeleted()` to revoke business access. Re-association after deletion requires an explicitly verified new customer through `attachCustomer`; old customer events remain rejected.

Stripe HMAC verification uses a 300-second window and rejects future timestamps outside that window, duplicate signature headers, wrong mode and Connect account events. Multiple trusted secrets support rotation. This integration targets direct Stripe accounts; Connect platform onboarding and split payments require a dedicated integration.

## Errors and native API {#errors-and-api}

Use a stable business operation key for every mutation. Stripe receives a stable hash of the account scope and key as `Idempotency-Key`. Requests are never automatically retried. Transport failures, timeouts, redirects, 5xx, 429 and malformed successful responses produce `OutcomeUnknown`: reconcile the original operation or safely reuse its Stripe key with the identical request. Other explicit HTTP refusals produce `Rejected(status)` without secrets or raw provider error bodies.

HTTP requests disable redirects, use a 15-second timeout and limit responses to 1 MiB. Observations omit payloads, authorization and raw business operation keys. `Client::api(method, path, parameters, key)` exposes a general server-side REST entry point: Stripe paths use `/v1/...`, nested form encoding and query encoding for GET, preserving pagination. This advanced entry point does not check local ownership or synchronize projections; applications must do so. Prefer identity-bound `Billable` methods for user actions.

This Rust package does not implement PHP Traits, Blade components or PHP method-for-method compatibility. Use hosted provider pages / official JS SDKs and provider invoice PDFs. Production requires merchant access, product permissions, catalog prices and a reachable webhook endpoint.
