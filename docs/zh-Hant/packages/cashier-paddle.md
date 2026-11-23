# Cashier（Paddle）

## 安裝與設定 {#configuration}

`openrust-cashier` 整合 Paddle Billing API v1，支援客戶、地址、Checkout 交易、訂閱變更、帳單、退款與持久 Webhook。與 Stripe 共用 `Billable`、`BillingStore` 和套件 Provider；安裝、五張表與通用交易約定見[Cashier（Stripe）](billing.md)。Paddle Classic 不適用。

需要自訂編譯設定時，執行 `artisan vendor:publish --tag=cashier-config` 發布 `config/cashier.rs`。重複發布保留應用修改，只有 `--force` 才覆寫。在應用 `config/mod.rs` 宣告 `mod cashier;`，並在設定群組中加入 `("cashier".into(), cashier::load(env)?)`，然後重新編譯應用。未發布設定也可使用套件內環境預設值。

```dotenv
CASHIER_DRIVER=paddle
PADDLE_SANDBOX=true
PADDLE_API_KEY=pdl_sdbx_apikey_...
PADDLE_WEBHOOK_SECRET=pdl_ntfset_...
```

`PADDLE_SANDBOX` 依 Cashier Paddle 預設 `false`，沙箱憑據應明確設定 `true`；`PADDLE_AUTH_CODE` 優先於 `PADDLE_API_KEY`。明確設定的 `CASHIER_LIVE` 保留舊模式覆寫，具名帳戶直接設定 `live`。`create_as_customer(options, key)` 重用已綁定客戶，新客戶要求明確電子郵件；回傳供應商 JSON 並儲存身分映射，不模擬 PHP morphOne，也不依電子郵件自動關聯既有客戶。關聯既有帳戶使用經過身分驗證的 `attach_customer`。API 與 Webhook 密鑰僅儲存在伺服器端。

## Checkout 與試用 {#checkout}

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

回傳 Paddle 交易物件。將 `id` 交給已初始化的 Paddle.js `Paddle.Checkout.open({transaction_id: id})`，或使用交易中的 `checkout.url`。預設付款連結需要獲准並整合 Paddle.js。瀏覽器完成事件不授予會員權限，等待 Webhook 與業務交易提交。

Paddle 透過交易完成建立訂閱，不提供 Stripe 式直接建立訂閱 API；`new_subscription(...).create(...)` 明確回傳 `Unsupported`。試用週期在 Paddle 價格目錄設定，`trial_days`／`trial_until` 不會模擬試用；`activate` 可提前開始收費。`with_coupon` 使用 Paddle discount ID，名稱存入 `custom_data.cashier_type`。一次性價格可透過 `user.checkout(...)` 銷售。

## 訂閱、帳單與客戶 {#subscriptions}

`subscription`、`subscription_by_id`、`sync_subscription` 與狀態判斷沿用[通用訂閱介面](billing.md#subscriptions)。訂閱 Webhook 同步客戶與多個價格／數量，拒絕處理未關聯客戶。

- `swap(prices, proration, key)`、`update_quantity` 更新完整價格集合。`Prorate` 計入下一帳期，`Invoice` 立即按比例計費，`None` 不計費。
- `cancel` 在下次計費期取消，`cancel_now` 立即取消；`resume` 清除未執行的取消／暫停計畫，或恢復已暫停訂閱。已完成的取消不能恢復，需重新 Checkout。
- `pause(now, until, key)` 可在目前或下一計費期暫停，可選 RFC3339 自動恢復時間；`activate` 提前結束試用。
- `charge(prices, key)` 對訂閱加入一次性收費；`preview(options, key)` 預覽變更。`update` 與 `as_provider_subscription` 提供核對歸屬後的供應商參數／物件。
- `payment_method_update_transaction()` 取得變更付款方式的交易，交給 Paddle.js；`billing_portal` 建立客戶入口網站工作階段。

`addresses`／`create_address` 管理帳單地址；`update_customer` 更新客戶。`transactions(parameters)` / `invoices(parameters)` 回傳原生分頁回應，使用 `per_page`／`after`，`find_invoice` 取得交易，`invoice_document` 取得 Paddle 託管帳單連結。`refund(transaction_id, options, key)` 建立退款 adjustment，options 填寫 `type`、`reason` 和部分退款的 `items`。退款可能等待審批，不能把受理回應當成退款到帳。

Paddle 負責稅費與結算；Stripe 的 SetupIntent、卡片綁定、稅號、餘額流水、PaymentIntent 與 Meter Events 在 Paddle 上明確回傳 `Unsupported`。供應商專屬能力使用 Paddle 對應介面，不能混傳兩家參數。

## Webhook 與重試 {#webhooks}

Paddle 通知目標訂閱 `subscription.*`、`transaction.*` 與業務需要的 adjustment 等事件。使用該帳戶的 `Client::verify_webhook` 檢查原始請求，再呼叫 `BillingStore::process` 或 `handle_webhook`，交易提交後才回傳 `event.success()`。簽章使用 `Paddle-Signature`、原始位元組 HMAC 與五秒時間窗口，支援多個 `h1` 與可信金鑰輪替。不要用事件的 `occurred_at` 代替此次投遞簽章時間。

套件自動同步訂閱與交易的精簡投影；其他已驗證事件交給應用 SQL 處理器。通知去重以 Paddle `event_id` 為準。舊事件到達時在客戶鎖內讀取目前 API 物件，防止本地狀態倒退。回復、取消恢復、業務身分與交易寄件匣約定見[Webhook 與交易](billing.md#webhooks)。

Paddle 不承諾 Stripe 式冪等鍵。所有高階變更先在 `cashier_operations` 持久認領業務操作鍵，再發送一次；同鍵再次呼叫不會發送，即使上次程序中斷或已回傳成功。`operation_status(client, key)` 回傳 `pending`／`done`／`rejected`／`unknown`；結果未知時先在 Paddle 透過原交易／訂閱／客戶識別碼核對。

同鍵已執行回傳 `OutcomeUnknown`，同鍵變更參數回傳 `EventConflict`。核對後才能決定是否使用新的業務鍵，不要透過換鍵盲目補發。客戶建立結果未知時需先查詢 Paddle 帳戶，驗證歸屬後 `attach_customer`。操作紀錄的保留週期必須涵蓋可能重試的時間，不要清空後重播歷史操作。

`Client::api` 可呼叫 Paddle REST API；保留原生回應封裝，使用 `/subscriptions/...` 等路徑、JSON 請求與 `Paddle-Version:1`。此進階入口略過持久操作認領和客戶歸屬檢查；需要防重時使用 `BillingStore::execute`。套件不自動跟隨供應商分頁 URL；應用擷取認可的游標後，再向固定 API 端點查詢。
