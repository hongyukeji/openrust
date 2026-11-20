# Cashier (Paddle)

## Installation and configuration {#configuration}

`openrust-cashier` supports Paddle Billing API v1: customers, addresses, Checkout transactions, subscription changes, invoices, refunds and durable webhooks. It shares `Billable`, `BillingStore` and its provider with Stripe. See [Cashier (Stripe)](billing.md) for installation, the five tables and transaction rules. Paddle Classic is not supported.

To customize the compiled configuration, run `artisan vendor:publish --tag=cashier-config`. The package publishes `config/cashier.rs`; repeated publishing preserves application edits unless `--force` is supplied. Add `mod cashier;` to your application `config/mod.rs`, insert `("cashier".into(), cashier::load(env)?)` in its configuration groups, then rebuild the application. Environment defaults work without publishing.

```dotenv
CASHIER_DRIVER=paddle
CASHIER_LIVE=false
PADDLE_API_KEY=pdl_sdbx_apikey_...
PADDLE_WEBHOOK_SECRET=pdl_ntfset_...
```

The default endpoint is sandbox. Use a new named account or separate database, `live:true` and production credentials for production. Paddle.js uses a public client-side token; keep API and webhook secrets on the server. Configure products, prices, a default payment link and a notification destination in Paddle.

## Checkout and trials {#checkout}

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("paddle")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.createOrGetCustomer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let transaction = user.newSubscription("default", vec![Price::new("pri_monthly", 1)?])?
    .withCoupon("dsc_welcome")?
    .checkout(json!({}), "subscription:42:first").await?;
```

The result is a Paddle transaction. Pass its `id` to an initialized Paddle.js `Paddle.Checkout.open({transactionId: id})` or use `checkout.url`. Your approved default payment link must integrate Paddle.js. Grant access only after webhook verification and the business transaction commit, not browser completion events.

Paddle creates subscriptions through completed transactions rather than a Stripe-style direct subscription creation API. `newSubscription(...).create(...)` returns `Unsupported`. Configure trial periods on catalog prices; `trialDays` / `trialUntil` do not emulate Paddle trials. `activate` starts billing early. `withCoupon` takes a Paddle discount ID; the subscription name uses `custom_data.cashier_type`. Use `user.checkout(...)` for one-time prices.

## Subscriptions, invoices and customers {#subscriptions}

`subscription`, `subscriptionById`, `syncSubscription` and status helpers follow the [shared subscription API](billing.md#subscriptions). Webhooks synchronize multiple prices and quantities and reject unmapped customers.

- `swap(prices, proration, key)` and `updateQuantity` update the complete item set. `Prorate` bills at the next period, `Invoice` prorates immediately and `None` does not bill for the change.
- `cancel` schedules cancellation for the next billing period; `cancelNow` cancels immediately. `resume` clears a pending cancellation / pause or resumes a paused subscription. Completed cancellations require a new Checkout.
- `pause(now, until, key)` pauses immediately or next period, optionally resuming at an RFC3339 timestamp. `activate` ends a trial early.
- `charge(prices, key)` adds a one-time subscription charge. `preview(options, key)` previews changes; `update` and `asProviderSubscription` expose owned provider parameters / objects.
- `paymentMethodUpdateTransaction()` obtains the transaction for changing payment details through Paddle.js. `billingPortal` creates a customer portal session.

`addresses` / `createAddress` manage billing addresses; `updateCustomer` updates details. `transactions(parameters)` / `invoices(parameters)` preserves pagination with `per_page` / `after`; `findInvoice` retrieves a transaction and `invoiceDocument` obtains its hosted invoice link. `refund(transaction_id, options, key)` creates a refund adjustment: supply `type`, `reason` and `items` for partial refunds. Adjustments can require approval; acceptance does not mean funds have settled.

Paddle handles taxes and settlement. Stripe SetupIntent, card attachment, tax ID, balance, PaymentIntent and Meter Events methods explicitly return `Unsupported` on Paddle. Use the corresponding Paddle feature instead of mixing provider parameters.

## Webhooks and retries {#webhooks}

Subscribe the notification destination to `subscription.*`, `transaction.*` and application-specific adjustment events. Call the account's `Client::verifyWebhook` with the original body, then `BillingStore::process` or `handleWebhook`, and return `event.success()` after commit. Verification checks `Paddle-Signature`, raw-byte HMAC and a five-second timestamp window; it supports multiple `h1` values and secret rotation. The event's `occurred_at` is not the signature's delivery time.

Subscriptions and transactions are projected automatically; other verified events go to your application SQL handler. Deduplication uses Paddle's `event_id`. Current API state is fetched under the customer lock to avoid state regression. See [webhook transactions](billing.md#webhooks) for rollback, cancellation recovery, ownership and transactional outboxes.

Paddle does not guarantee Stripe-style idempotency. High-level mutations durably claim their business operation key in `cashier_operations` before sending once. Reusing a key never sends again, even after a crash or successful response. `operationStatus(client, key)` returns `pending`, `done`, `rejected` or `unknown`. Reconcile unknown outcomes in Paddle using the original transaction, subscription or customer identity.

A reused operation returns `OutcomeUnknown`; changing its parameters returns `EventConflict`. Use a new key only after reconciliation, never to blindly resend. For unknown customer creation, inspect the Paddle account, verify ownership and use `attachCustomer`. Retain operation records throughout the business retry period; deleting them makes historical replays unsafe.

`Client::api` exposes a general Paddle REST API entry point with `/subscriptions/...` paths, JSON and `Paddle-Version:1`, preserving response envelopes. It bypasses durable operation claims and local ownership checks; use `BillingStore::execute` when deduplication is required. The client does not follow provider pagination URLs automatically: extract an approved cursor and query the fixed API endpoint.
