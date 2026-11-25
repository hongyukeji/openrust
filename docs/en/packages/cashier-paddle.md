# Cashier (Paddle)

## Installation and configuration {#configuration}

`openrust-cashier` supports Paddle Billing API v1: customers, addresses, Checkout transactions, subscription changes, invoices, refunds and durable webhooks. It shares `Billable`, `BillingStore` and its provider with Stripe. See [Cashier (Stripe)](billing.md) for installation, the five tables and transaction rules. Paddle Classic is not supported.

To customize the compiled configuration, run `artisan vendor:publish --tag=cashier-config`. The package publishes `config/cashier.rs`; repeated publishing preserves application edits unless `--force` is supplied. Add `mod cashier;` to your application `config/mod.rs`, insert `("cashier".into(), cashier::load(env)?)` in its configuration groups, then rebuild the application. Environment defaults work without publishing.

```dotenv
CASHIER_DRIVER=paddle
PADDLE_SANDBOX=true
PADDLE_API_KEY=pdl_sdbx_apikey_...
PADDLE_WEBHOOK_SECRET=pdl_ntfset_...
```

`PADDLE_SANDBOX` defaults to `false`, following Cashier Paddle; set it to `true` for sandbox credentials. `PADDLE_AUTH_CODE` takes precedence over `PADDLE_API_KEY`. An explicitly set `CASHIER_LIVE` retains the previous mode override. Named accounts specify `live` directly. `create_as_customer(options, key)` reuses an attached customer and otherwise requires an explicit email. It returns provider JSON with a stored owner mapping; it does not implement PHP morphOne or automatically attach an existing customer found by email. Use authenticated `attach_customer` when linking an existing account. Keep API keys and webhook secrets on the server. Paddle.js uses a public client-side token. Configure products, prices, a default payment link and a notification destination in Paddle.

Paddle also supports [account-specific HTTP middleware](billing.md#account-http) for a named account.

## Checkout and trials {#checkout}

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("paddle")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.create_as_customer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let transaction = user.new_subscription("default", vec![Price::new("pri_monthly", 1)?])?
    .with_coupon("dsc_welcome")?
    .checkout(json!({}), "subscription:42:first").await?;
```

The result is a Paddle transaction. Pass its `id` to an initialized Paddle.js `Paddle.Checkout.open({transaction_id: id})` or use `checkout.url`. Your approved default payment link must integrate Paddle.js. Grant access only after webhook verification and the business transaction commit, not browser completion events.

Paddle creates subscriptions through completed transactions rather than a Stripe-style direct subscription creation API. `new_subscription(...).create(...)` returns `Unsupported`. Configure trial periods on catalog prices; `trial_days` / `trial_until` do not emulate Paddle trials. `activate` starts billing early. `with_coupon` takes a Paddle discount ID; the subscription name uses `custom_data.cashier_type`. Use `user.checkout(...)` for one-time prices.

## Subscriptions, invoices and customers {#subscriptions}

`subscription`, `subscription_by_id`, `sync_subscription` and status helpers follow the [shared subscription API](billing.md#subscriptions). Webhooks synchronize multiple prices and quantities and reject unmapped customers.

- `swap(prices, proration, key)` and `update_quantity` update the complete item set. `Prorate` bills at the next period, `Invoice` prorates immediately and `None` does not bill for the change.
- `cancel` schedules cancellation for the next billing period; `cancel_now` cancels immediately. `resume` clears a pending cancellation / pause or resumes a paused subscription. Completed cancellations require a new Checkout.
- `pause(now, until, key)` pauses immediately or next period, optionally resuming at an RFC3339 timestamp. `activate` ends a trial early.
- `charge(prices, key)` adds a one-time subscription charge. `preview(options, key)` previews changes; `update` and `as_provider_subscription` expose owned provider parameters / objects.
- `payment_method_update_transaction()` obtains the transaction for changing payment details through Paddle.js. `billing_portal` creates a customer portal session.

`addresses` / `create_address` manage billing addresses; `update_customer` updates details. `transactions(parameters)` / `invoices(parameters)` preserves pagination with `per_page` / `after`; `find_invoice` retrieves a transaction and `invoice_document` obtains its hosted invoice link. `refund(transaction_id, options, key)` creates a refund adjustment: supply `type`, `reason` and `items` for partial refunds. Adjustments can require approval; acceptance does not mean funds have settled.

Paddle handles taxes and settlement. Stripe SetupIntent, card attachment, tax ID, balance, PaymentIntent and Meter Events methods explicitly return `Unsupported` on Paddle. Use the corresponding Paddle feature instead of mixing provider parameters.

## Webhooks and retries {#webhooks}

Subscribe the notification destination to `subscription.*`, `transaction.*` and application-specific adjustment events. Call the account's `Client::verify_webhook` with the original body, then `BillingStore::process` or `handle_webhook`, and return `event.success()` after commit. Verification checks `Paddle-Signature`, raw-byte HMAC and a five-second timestamp window; it supports multiple `h1` values and secret rotation. The event's `occurred_at` is not the signature's delivery time.

Subscriptions and transactions are projected automatically; other verified events go to your application SQL handler. Deduplication uses Paddle's `event_id`. Current API state is fetched under the customer lock to avoid state regression. See [webhook transactions](billing.md#webhooks) for rollback, cancellation recovery, ownership and transactional outboxes.

Paddle does not guarantee Stripe-style idempotency. High-level mutations durably claim their business operation key in `cashier_operations` before sending once. Reusing a key never sends again, even after a crash or successful response. `operation_status(client, key)` returns `pending`, `done`, `rejected` or `unknown`. Reconcile unknown outcomes in Paddle using the original transaction, subscription or customer identity.

A reused operation returns `OutcomeUnknown`; changing its parameters returns `EventConflict`. Use a new key only after reconciliation, never to blindly resend. For unknown customer creation, inspect the Paddle account, verify ownership and use `attach_customer`. Retain operation records throughout the business retry period; deleting them makes historical replays unsafe.

`Client::api` exposes a general Paddle REST API entry point with `/subscriptions/...` paths, JSON and `Paddle-Version:1`, preserving response envelopes. It bypasses durable operation claims and local ownership checks; use `BillingStore::execute` when deduplication is required. The client does not follow provider pagination URLs automatically: extract an approved cursor and query the fixed API endpoint.
