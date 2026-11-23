# Cashier（Stripe）

## 安裝與設定 {#configuration}

`openrust-cashier` 提供 Stripe 與 [Paddle Billing](cashier-paddle.md) 的客戶、Checkout、訂閱、帳單與 Webhook。微信支付和支付寶使用[支付擴充套件](pay.md)。兩個套件可以同時啟用；國內人民幣金額型別不套用於 Stripe 多幣別計費。

根應用啟用 `cashier` Cargo feature。獨立應用加入 `openrust-cashier` 相依套件並使用 OpenRust 套件探索。`CashierServiceProvider` 自動綁定 `CashierManager` 並登記遷移，啟動時不連線至計費服務、不查詢帳單表。執行原生 `artisan migrate` 建立 `cashier_customers`、`cashier_subscriptions`、`cashier_invoices`、`cashier_events`、`cashier_operations`。

需要自訂編譯設定時，執行 `artisan vendor:publish --tag=cashier-config` 發布 `config/cashier.rs`。重複發布保留應用修改，只有 `--force` 才覆寫。在應用 `config/mod.rs` 宣告 `mod cashier;`，並在設定群組中加入 `("cashier".into(), cashier::load(env)?)`，然後重新編譯應用。未發布設定也可使用套件內環境預設值。

```dotenv
CASHIER_DRIVER=stripe
CASHIER_LIVE=false
STRIPE_SECRET=sk_test_...
STRIPE_WEBHOOK_SECRET=whsec_...
```

Stripe API 固定為 `2026-11-18.endive`；Webhook 使用相同版本。正式環境使用 `CASHIER_LIVE=true` 並替換 API 和 Webhook 金鑰。私密金鑰只保存在伺服器。前端 Stripe.js 使用公開的 publishable key，不能使用 `STRIPE_SECRET`。

套件載入 `cashier.default`、Stripe 的 `cashier.key`／`cashier.secret`／`cashier.webhook.secret`，以及 Paddle 的 `cashier.paddle.api_key`／`cashier.paddle.webhook_secret`／`cashier.paddle.sandbox`。同一 Rust crate 使用 Paddle 設定命名空間區分兩個上游套件。`cashier.drivers` 儲存明確具名帳戶，同名帳戶優先於供應商原生設定。應用設定載入器可回傳 `cashier` 群組覆寫預設值；應用設定優先，`drivers` 整組替換。具名帳戶需明確填寫 `driver`、`secret`、`webhook_secret`、`live`。`webhook_secrets` 陣列支援金鑰輪替。每個名稱代表穩定的商戶帳戶與模式；不要把已使用的名稱重新分配給其他商戶或從測試切換到正式模式，改用獨立資料庫或新帳戶名稱。

`Cashier::driver("stripe")`、`Cashier::default_driver()` 在應用作用域解析帳戶。`CashierManager::get_default_driver()` 回傳設定名稱；`forget_drivers()` 後新解析實例載入新設定，已持有實例保留原憑證快照。啟用設定快取時保留快取快照；變更環境後需按[設定文件](../getting-started/configuration.md)重建快取。

## 客戶與 Checkout {#customers-and-checkout}

`Billable` 綁定已經應用驗證的身分，不透過通知參數、電子郵件或 Checkout 查詢字串推斷使用者。帳戶名稱與本地身分共同隔離客戶對應，同一遠端客戶只能關聯一次。

Stripe 客戶介面保留 `stripe_id`、`has_stripe_id`、`create_as_stripe_customer`、`create_or_get_stripe_customer`、`update_stripe_customer`、`update_or_create_stripe_customer` 和 `as_stripe_customer(&["tax"])`。已存在客戶時建立回傳 `CustomerAlreadyCreated`；建立或取得會讀取現有客戶並傳遞 `expand`。`billing_portal_url(return_url, options, key)` 回傳經過驗證的 HTTPS 位址，`redirect_to_billing_portal` 回傳 HTTP 302 回應。Stripe 專用介面在送出請求前拒絕 Paddle 帳戶。既有 `create_or_get_customer`、`update_customer` 和 `billing_portal` 保留為明確的跨供應商便利入口。

`Billable` 是綁定可信身分的適配器；客戶姓名、電子郵件、地址透過 `options` 提供，不反射應用模型欄位。供應商物件回傳 JSON，本地訂閱和帳單紀錄使用型別。`subscription()` 回傳持有操作能力並解參考至 `Subscription` 資料的 `SubscriptionHandle`；`as_stripe_subscription` 與 `as_paddle_subscription` 分別驗證供應商。`raw_balance()` 回傳 Stripe 最小貨幣單位；既有整數 `balance()` 是便利別名，不提供 PHP 的在地化貨幣格式。修改請求明確傳入業務冪等鍵以保留重試安全。

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

導向 Stripe 回傳的 `url`。成功跳轉只表示瀏覽器返回，應以經過驗簽並持久處理的 Webhook 同步付款與訂閱狀態。`checkout(prices, options, key)` 建立一次性 Checkout；訂閱使用 `new_subscription(...).checkout(...)`。價格必須來自伺服器認可的 Stripe 目錄，不接受客戶端任意提交價格、金額或客戶 ID。

`create_or_get_customer` 回傳已關聯客戶或建立客戶；並行呼叫必須重用同一穩定操作鍵。`attach_customer(id)` 只用於應用已驗證歸屬的既有客戶或結果未知後的人工核對，不能直接傳入任意請求參數。`update_customer` 更新資料；`billing_portal(return_url, key)` 建立 Stripe 客戶入口網站工作階段。

## 訂閱生命週期 {#subscriptions}

`new_subscription(name, prices)` 支援多個價格、`Price::new(id, quantity)`、`Price::metered(id)`、`trial_days`、`trial_until`、`skip_trial`、`quantity`、`anchor_billing_cycle_on`、`with_metadata`、`with_coupon` 與 `with_options`。直接 `create(payment_method, key)` 使用 Stripe `default_incomplete`，提供的付款方式必須已屬於該客戶。

```rust
use openrust_cashier::Proration;

let subscription = user.subscription("default").await?.ok_or("missing subscription")?;
let subscription = subscription.update_quantity(3, None, Proration::Invoice, "seats:42:v2").await?;
let subscription = subscription.cancel("cancel:42:v1").await?;
if subscription.on_grace_period() {
    subscription.resume("resume:42:v1").await?;
}
```

`swap(prices, proration, key)` 替換完整價格集合，移除舊項目並保留相符項目 ID。`Proration::Prorate` 按比例計入下次帳單，`Invoice` 立即結算，`None` 不按比例結算。多價格訂閱使用 `update_quantity` 時明確指定價格。`cancel` 在計費週期結束時取消，`cancel_now` 立即取消；`resume` 恢復寬限期內或 Stripe 暫停的訂閱。`extend_trial` 延長試用；`pause_collection`／`resume_collection` 控制 Stripe 收款暫停，與訂閱狀態暫停不同。

`subscription(name)` 回傳該名稱目前的訂閱；若只剩已終止訂閱，回傳最新紀錄。多個仍存在的同名訂閱會明確失敗，此時使用 `subscription_by_id(id)`。`sync_subscription(id)` 在核對客戶歸屬後讀取遠端狀態；`refresh()` 重新整理現有實例。`subscribed`、`valid`、`active`、`on_trial`、`canceled`、`on_grace_period`、`paused`、`past_due`、`incomplete` 可用於應用權限判斷；欠費或未完成付款預設不授予有效訂閱權限。

`as_provider_subscription(parameters)` 取得核對歸屬後的原生物件。例如傳入 `{"expand":["latest_invoice.confirmation_secret"]}` 取得直接建立訂閱所需的付款確認資訊；將適用的 client secret 交給該客戶的 Stripe.js 完成驗證。不要記錄或跨使用者回傳 client secret。

## 付款、帳單與稅費 {#payments-and-invoices}

`create_setup_intent`、`payment_methods`、`add_payment_method`、`default_payment_method`、`update_default_payment_method`、`delete_payment_method` 管理 Stripe 付款方式。使用 Stripe.js Payment Element 收集卡片資訊；伺服器只接收 Stripe 識別碼，不接收卡號或 CVC。

`pay(amount, currency, options, key)` 建立 PaymentIntent；`charge` 用既有付款方式發起離線確認；`confirm_payment` 確認已有 Intent。金額是幣別最小單位的正整數，例如 USD 的美分或 JPY 的日圓，不使用浮點數。回傳 `requires_action`、`requires_payment_method` 或 `processing` 不代表付款成功，應按狀態完成 Stripe.js 驗證並等待 Webhook。`refund(payment_intent_id, options, key)` 支援部分金額；受理不代表資金已退回。

`invoices(parameters)` 保留供應商分頁回應，支援 `limit`／`starting_after`；`find_invoice`、`invoice_document` 回傳屬於該客戶的帳單與供應商 PDF／託管連結。`invoice_item`、`create_invoice`、`finalize_invoice`、`pay_invoice`、`void_invoice` 支援手動帳單流程，`preview_invoice` 與訂閱的 `preview` 用於變更預覽。`local_invoices` 回傳 Webhook 同步的精簡投影。

`tax_ids`、`create_tax_id`、`delete_tax_id` 管理稅號；`balance` 回傳整數最小單位的未格式化餘額，`balance_transactions` 查詢流水，`apply_balance` 加入貸記或借記。`report_usage(event_name, value, identifier, timestamp, key)` 使用 Stripe Billing Meter Events，事件識別碼需穩定且唯一。稅率、自動稅、折扣、計費錨點可透過 `with_options`、`update`、Checkout／帳單 options 傳入。options 只接受應用建立的伺服器參數，不能直接轉交整份 HTTP 請求。

## Webhook 與交易 {#webhooks}

每個具名帳戶建立固定 HTTPS Webhook 端點。Stripe 訂閱 `customer.subscription.*`、`invoice.*`，並按業務加入 PaymentIntent、退款等事件。路由排除只適用於瀏覽器表單的 CSRF 驗證，改依簽章驗證。必須傳入未經 JSON 重新編碼的原始位元組：

```rust
let event = client.verify_webhook(&headers, &raw_body)?;
store.process(&client, &event, |tx, event| Box::pin(async move {
    // 使用 tx 更新業務權限、訂單或交易寄件匣。
    // currentSubscription()／currentInvoice() 是本次核對的目前投影。
    // data() 是原始事件，可能延遲送達；權限變更應依目前投影。
    let _ = (tx, event);
    Ok(())
})).await?;
let response = event.success(); // 僅在交易提交或重複確認後回傳 HTTP 200。
```

`handle_webhook` 是不加入應用 SQL 的同步入口。`process` 將事件去重、訂閱／帳單投影、業務 SQL 與完成標記放進同一交易；錯誤或取消時回復，供應商重送可恢復。同一事件 ID 換內容會回傳 `EventConflict`。客戶鎖內讀取目前供應商物件，避免延遲通知以舊狀態覆寫新狀態；這需要可用的供應商 API，失敗時應回傳失敗讓通知重送。

未關聯的訂閱／帳單客戶會被拒絕。先建立可信客戶對應，再開放 Checkout；從 Dashboard 建立的訂閱也需使用已關聯客戶，名稱可放在 `metadata.cashier_type`，預設為 `default`。其他已驗簽事件可由處理器使用，套件不擅自將其視為付款或履約完成。只進行交易內 SQL；出貨、郵件等外部操作透過應用交易寄件匣完成。不要在處理器中反向發起計費變更或再次處理同一事件。

Stripe `customer.deleted` 會向供應商核實，在同一交易停用客戶對應並取消本地訂閱投影；處理器可透過 `customer_deleted()` 撤銷業務權限。刪除後重新關聯需明確驗證新客戶並呼叫 `attach_customer`；舊客戶通知仍會拒絕。

Stripe HMAC 時間窗口為 300 秒，拒絕未來超窗、重複簽章標頭、跨模式與 Connect 帳戶事件；可設定多個可信金鑰輪替。本套件支援 Stripe 直接帳戶；Connect 平台分帳與代商戶授權需要專用整合。

## 重試與原生 API {#errors-and-api}

所有變更使用穩定業務操作鍵；Stripe 將帳戶與操作鍵的穩定雜湊作為 `Idempotency-Key`。套件不自動重試；網路失敗、逾時、重新導向、5xx、429 或無法解析的成功回應會回傳 `OutcomeUnknown`，先核對原操作，或以完全相同的請求與 Stripe 原鍵安全重試。其他明確 HTTP 拒絕回傳 `Rejected(status)`，錯誤不包含金鑰或供應商原始回應。

HTTP 請求停用重新導向，逾時 15 秒，回應上限 1 MiB；觀察紀錄移除本文、授權標頭與原始業務操作鍵。`Client::api(method, path, parameters, key)` 提供通用伺服器 REST 入口：Stripe 使用 `/v1/...` 與巢狀表單編碼，GET 參數編入查詢，保留供應商分頁結構。此進階入口不執行本地客戶歸屬檢查或自動同步，應用必須自行驗證；使用者操作優先使用綁定身分的 `Billable`。

本套件不提供框架PHP Trait、Blade 元件或逐方法 PHP 相容層。前端使用供應商託管頁面／官方 JS SDK，PDF 使用供應商帳單連結。正式接入需要有效商戶、產品權限、價格目錄與可存取的通知地址。
