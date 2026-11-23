# Payments (Extension)

`openrust-pay` provides one-time payments for direct WeChat Pay API v3 merchants using public-key mode and Alipay merchants using RSA2 public-key mode. It uses the application HTTP client, container and database transactions. Subscription billing, Stripe, Paddle, service-provider merchants, profit sharing, automatic debit, certificate mode and certificate download are outside this package's supported scope.

Stripe subscriptions and Paddle Billing use [Cashier](billing.md) and [Cashier (Paddle)](cashier-paddle.md).

## Enable and configure {#configuration}

Enable the root application's `pay` Cargo feature. Package discovery registers `PayServiceProvider`, whose boot hook registers the callback inbox migration without querying the database. Run migrations before using `DatabaseNotifications`:

```sh
cargo run --features pay -- artisan migrate
```

The package supplies its own defaults; the default application contains no `config/pay.rs`. To customize them, run `./artisan vendor:publish --tag=pay-config`, add `mod pay;` and `("pay".to_owned(), pay::load(env)?),` to the module declarations and configuration map in `config/mod.rs`, then rebuild. Publication preserves existing files unless `--force` is supplied. Configuration cache snapshots retain the selected merchant values.

Configure `pay.default` with `PAY_DRIVER` (`wechat` by default). Named merchant profiles live under `pay.drivers.<name>` with `driver` set to `wechat` or `alipay`. Add separate names for different merchants or application IDs. No network call or key read occurs until a gateway is resolved.

| Driver | Environment variables |
| --- | --- |
| WeChat | `PAY_WECHAT_APP_ID`, `PAY_WECHAT_MCH_ID`, `PAY_WECHAT_SERIAL_NO`, `PAY_WECHAT_PRIVATE_KEY_PATH`, `PAY_WECHAT_API_V3_KEY`, `PAY_WECHAT_PUBLIC_KEY_ID`, `PAY_WECHAT_PUBLIC_KEY_PATH` |
| Alipay | `PAY_ALIPAY_APP_ID`, `PAY_ALIPAY_SELLER_ID`, `PAY_ALIPAY_PRIVATE_KEY_PATH`, `PAY_ALIPAY_PUBLIC_KEY_PATH` |

Private keys are 2048-bit RSA PKCS#8 or PKCS#1 PEM files. Platform public keys are SPKI or PKCS#1 PEM, obtained through the merchant's trusted management channel. WeChat's `serial_no` is the merchant signing certificate serial, while `public_key_id` is the platform verification key ID beginning with `PUB_KEY_ID_`; they serve different purposes. Its API v3 key must be exactly 32 bytes. For rotation, configure additional trusted IDs and PEM values in `public_keys`.

Profiles accept `private_key`/`public_key` inline PEM instead of the corresponding path, with exactly one source per key. Resolve relative paths from the process working directory or use absolute paths. Protect configuration caches and key files with deployment access controls. Optional `endpoint` must be HTTPS; HTTP is accepted only for loopback protocol testing. Defaults are the official WeChat API v3 origin and Alipay gateway. Alipay sandbox requires the sandbox endpoint and its matching credentials and verification key.

Other applications may depend on `openrust-pay` and register `PayServiceProvider`; it registers `PayServiceProvider::migrations()` through `load_migrations_from`. No database access is required for gateway registration or direct operations.

## Create a payment {#create-payment}

Amounts use integer CNY fen. `Amount::from_yuan("12.34")` is also exact; negative values, zero, exponents and more than two decimal places are rejected. Keep the merchant order number stable across queries and retries. Numbers use 1–32 ASCII letters, digits, underscores or hyphens. Notify and optional return URLs must use HTTPS.

```rust
use openrust_pay::{Amount, Order, Scene, facades::Pay};

let order = Order::new(
    "order_001",
    "Coffee",
    Amount::from_fen(1234)?,
    "https://shop.example/pay/notify",
);
let payment = Pay::driver("wechat")?.pay(Scene::Scan, &order).await?;
```

`Payment` is a UI payload, never proof of payment. Match its variant: `QrCode` for a QR renderer, `Redirect` for a browser redirect, `App` for an Alipay app order string, `Jsapi` for WeChat JSAPI/mini-program parameters, or `WechatApp` for WeChat app parameters. WeChat client parameters are signed by the merchant key.

| Scene | WeChat | Alipay |
| --- | --- | --- |
| `Scene::Scan` | Native QR | Precreate QR |
| `Scene::Jsapi` | JSAPI / mini-program; set `order.openid` for this app ID | Unsupported |
| `Scene::Wap` | H5; set `order.client_ip` from a trusted request/proxy | Mobile web |
| `Scene::Web` | Unsupported | Desktop web |
| `Scene::App` | App | App |

An Alipay web payment may set `order.return_url`. Obtain the payer's OpenID through the corresponding application's authentication flow; a third-party login identifier from a different app is not interchangeable.

## Query, close and refund {#query-and-refund}

Use `gateway.find(out_trade_no)`, `gateway.close(out_trade_no)`, `gateway.refund(&Refund)` and `gateway.find_refund(out_trade_no, out_refund_no)`. They return signature-verified provider JSON; inspect the provider's trade/refund state before deciding that payment or refund completed. `Refund` contains the original order number, a stable merchant refund number, the original `total`, the refund `amount` and optional `reason`. Refund amount cannot exceed total; partial refunds are supported.

Requests are not automatically retried. `Error::OutcomeUnknown` means that acceptance could not be established, including a lost response or invalid response signature. Query with the original order/refund number before retrying. Do not manufacture a new number to retry an uncertain request. Signed provider rejections return `Error::Rejected`; raw provider messages and bodies are excluded from errors and HTTP observations. Request/response bodies and authorization headers are redacted; successful JSON returned to application code still contains business data and requires deliberate logging/serialization.

## Verify and process callbacks {#callbacks}

Pass the original request headers and raw bytes to `gateway.callback`. WeChat checks a trusted key ID, RSA signature and a five-minute signature timestamp window before AES-GCM decryption; Alipay parses the form once, rejects duplicate keys and malformed encoding, and verifies RSA2. Both verify merchant and application identity. Only payment-success events are accepted; refund notifications and other event types must not be routed to this handler. Use refund queries for the supported refund flow.

Load `ExpectedPayment` from the trusted business order. Its immutable original amount and number must match the verified callback. The helper does not create orders, decide inventory, fulfil purchases or grant membership.

```rust
use openrust_pay::{DatabaseNotifications, ExpectedPayment, facades::Pay};

let notification = Pay::driver("wechat")?.callback(&headers, &raw_body)?;
let expected = ExpectedPayment {
    out_trade_no: business_order_number,
    amount: business_order_amount,
};
DatabaseNotifications::new(connection)
    .process(&notification, &expected, |tx, notice| Box::pin(async move {
        // Validate the business state and update it using this transaction.
        tx.table("orders")
            .r#where("number", "=", notice.out_trade_no())
            .r#where("status", "=", "pending")
            .update([("status".into(), "paid".into())].into_iter().collect())
            .await?;
        Ok(())
    }))
    .await?;
let response = notification.success();
```

The `payment_notifications` inbox stores hashed event identity, a normalized fingerprint and processing state; it excludes raw callback bodies and credentials. Deduplication uses merchant/application identity and provider transaction ID, so repeated Alipay success/finished notifications do not fulfil twice. Changed contents under the same identity fail with `NotificationConflict`. `Processed` and `Duplicate` may both be acknowledged after commit; handler failure or cancellation rolls back business updates and the inbox marker together.

The handler must use the supplied `Transaction`, validate the current business state, and perform only transactional SQL work. Load the original expected order from a trusted source; if its monetary fields are mutable, lock and recheck them inside the handler. Send messages or fulfil external services through an application transactional outbox after commit. This helper does not make external effects atomic. Separate applications must not share an inbox unless they also share the same business order ownership.

Without the helper, call `notification.validate(&expected)` and implement equivalent durable deduplication in the business transaction. A front-end return URL or a verified callback alone must not mark an order fulfilled. Return the success acknowledgment only after the business transaction commits. On verification/processing failure return an error response and let the provider retry according to its protocol.

## Gateway lifecycle {#gateway-lifecycle}

`PayManager::driver(name)` lazily resolves and reuses a merchant configuration and key snapshot. `get_default_driver` reads the current default; `Pay::default_driver()` resolves it. `forget_drivers` applies new configuration to future resolutions, while held `Gateway` clones retain their original snapshot. Managers are application-local. Refresh the relevant instances when rotating keys; never obtain a verification key from an untrusted callback.

Real merchant onboarding, enabled product permissions, public HTTPS callbacks and provider end-to-end verification remain deployment responsibilities. Protocol tests do not establish that a merchant account can move real funds.
