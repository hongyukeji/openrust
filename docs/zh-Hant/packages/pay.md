# 支付（擴充）

`openrust-pay` 支援微信支付 API v3 一般直連商戶的公鑰模式，以及支付寶 RSA2 一般公鑰模式的一次性支付。沿用應用 HTTP 用戶端、容器與資料庫交易。訂閱計費、Stripe、Paddle、服務商、分帳、自動扣款、憑證模式及憑證自動下載不屬於本套件支援範圍。

Stripe 訂閱與 Paddle Billing 使用 [Cashier](billing.md) 和 [Cashier（Paddle）](cashier-paddle.md)。

## 啟用與設定 {#configuration}

根應用啟用 `pay` Cargo feature 後，套件發現登記 `PayServiceProvider`，其 boot 鉤子登記回呼收件匣遷移，登記過程不查詢資料庫。使用 `DatabaseNotifications` 前先遷移：

```sh
cargo run --features pay -- artisan migrate
```

套件本身提供預設設定，預設應用不包含 `config/pay.rs`。需要自訂時執行 `./artisan vendor:publish --tag=pay-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod pay;` 與 `("pay".to_owned(), pay::load(env)?),`，然後重新建置。發布預設保留既有檔案，傳入 `--force` 才覆寫；設定快取快照保留所選商戶設定。

`PAY_DRIVER` 設定 `pay.default`，預設 `wechat`。商戶設定放在 `pay.drivers.<name>`，`driver` 為 `wechat` 或 `alipay`。不同商戶或應用 ID 使用不同設定名稱。解析閘道前不存取網路或讀取金鑰檔案。

| 驅動 | 環境變數 |
| --- | --- |
| 微信支付 | `PAY_WECHAT_APP_ID`、`PAY_WECHAT_MCH_ID`、`PAY_WECHAT_SERIAL_NO`、`PAY_WECHAT_PRIVATE_KEY_PATH`、`PAY_WECHAT_API_V3_KEY`、`PAY_WECHAT_PUBLIC_KEY_ID`、`PAY_WECHAT_PUBLIC_KEY_PATH` |
| 支付寶 | `PAY_ALIPAY_APP_ID`、`PAY_ALIPAY_SELLER_ID`、`PAY_ALIPAY_PRIVATE_KEY_PATH`、`PAY_ALIPAY_PUBLIC_KEY_PATH` |

私鑰為 2048 位 RSA 的 PKCS#8 或 PKCS#1 PEM 檔案。平台公鑰為 SPKI 或 PKCS#1 PEM，透過商戶可信管理管道取得。微信 `serial_no` 是商戶簽章憑證序號，`public_key_id` 是以 `PUB_KEY_ID_` 開頭的平台驗章公鑰 ID，兩者職責不同。API v3 金鑰必須剛好 32 位元組。輪換時可在 `public_keys` 中增加其他可信公鑰 ID 與 PEM 內容。

設定也可用 `private_key`／`public_key` 內嵌 PEM 替代對應路徑，每把金鑰只能指定一種來源。相對路徑以程序工作目錄解析，也可使用絕對路徑。設定快取及金鑰檔案須由部署權限保護。可選 `endpoint` 必須使用 HTTPS，僅回環位址允許 HTTP 協定測試；預設指向官方微信 API v3 網域和支付寶閘道。支付寶沙箱須同時使用沙箱端點及其對應憑證、公鑰。

其他應用可依賴 `openrust-pay` 並登記 `PayServiceProvider`；它透過 `load_migrations_from` 登記 `PayServiceProvider::migrations()`。閘道註冊及直接支付操作不要求存取本機資料庫。

## 發起支付 {#create-payment}

金額使用人民幣整數分，也支援精確解析 `Amount::fromYuan("12.34")`；拒絕負數、零、指數及超過兩位小數。商戶訂單號在查詢和重試間保持不變，使用 1–32 位 ASCII 字母、數字、底線或連字號。回呼及可選返回位址須使用 HTTPS。

```rust
use openrust_pay::{Amount, Order, Scene, facades::Pay};

let order = Order::new(
    "order_001",
    "Coffee",
    Amount::fromFen(1234)?,
    "https://shop.example/pay/notify",
);
let payment = Pay::driver("wechat")?.pay(Scene::Scan, &order).await?;
```

`Payment` 是供前端使用的參數，不代表已經支付。依列舉處理：`QrCode` 用於繪製 QR 碼，`Redirect` 用於瀏覽器導向，`App` 為支付寶 App 訂單字串，`Jsapi` 為微信 JSAPI／小程式參數，`WechatApp` 為微信 App 參數。微信調起參數由商戶私鑰簽章。

| 場景 | 微信支付 | 支付寶 |
| --- | --- | --- |
| `Scene::Scan` | Native 掃碼 | 預建立掃碼 |
| `Scene::Jsapi` | JSAPI／小程式；設定屬於該應用的 `order.openid` | 不支援 |
| `Scene::Wap` | H5；從可信請求或代理取得 `order.client_ip` | 手機網頁 |
| `Scene::Web` | 不支援 | 電腦網頁 |
| `Scene::App` | App | App |

支付寶網頁支付可設定 `order.return_url`。付款人的 OpenID 須透過對應應用的認證流程取得，其他應用的第三方登入識別碼不能直接替代。

## 查單、關單與退款 {#query-and-refund}

使用 `gateway.find(out_trade_no)`、`gateway.close(out_trade_no)`、`gateway.refund(&Refund)` 和 `gateway.findRefund(out_trade_no, out_refund_no)`。回傳經過簽章驗證的廠商 JSON；須檢查交易或退款狀態，不能把請求成功直接當作資金處理完成。`Refund` 包含原訂單號、穩定的商戶退款單號、原訂單 `total`、本次退款 `amount` 及可選 `reason`。退款金額不能超過原訂單金額，支援部分退款。

請求不會自動重試。`Error::OutcomeUnknown` 表示無法確定廠商受理結果，包括回應遺失、回應簽章無效等情況。先用原訂單號或退款單號查詢，再決定是否重試，不要為不確定請求產生新單號。經過驗章的廠商拒絕回傳 `Error::Rejected`；錯誤與 HTTP 觀察事件不包含原始廠商訊息和內容。請求／回應本文與授權標頭會遮蔽，但回傳給應用的成功 JSON 仍包含業務資料，記錄日誌或序列化時應明確選擇欄位。

## 驗證與處理回呼 {#callbacks}

將原始請求標頭與原始位元組交給 `gateway.callback`。微信先檢查可信公鑰 ID、RSA 簽章與五分鐘簽章時間窗口，再進行 AES-GCM 解密；支付寶僅解碼表單一次，拒絕重複鍵及非法編碼，並驗證 RSA2。兩者均核對商戶和應用身分。僅接受支付成功事件；退款通知及其他事件不能交給此處理器。退款流程使用退款查詢確認結果。

從可信業務訂單讀取 `ExpectedPayment`，核對不可變的原始金額和訂單號。支付套件不負責建立業務訂單、庫存決策、出貨或授予會員。

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
            .r#where("number", "=", notice.outTradeNo())
            .r#where("status", "=", "pending")
            .update([("status".into(), "paid".into())].into_iter().collect())
            .await?;
        Ok(())
    }))
    .await?;
let response = notification.success();
```

`payment_notifications` 收件匣只存事件身分雜湊、正規化指紋和處理狀態，不存原始回呼及憑證。依商戶／應用身分和廠商交易號去重，支付寶重複的成功／完成通知不會重複執行處理器。同一身分的內容變化回傳 `NotificationConflict`。`Processed` 與 `Duplicate` 均可在提交後確認；處理失敗或取消時，業務更新與收件匣標記一起回滾。

處理器須使用傳入的 `Transaction`，核對目前業務狀態，並僅執行交易內 SQL。原始預期訂單必須來自可信來源；若金額欄位允許修改，須在處理器內加鎖並重新核對。傳送訊息或呼叫外部履約服務應透過應用交易寄件匣在提交後處理，此輔助類別不保證外部副作用原子性。不同應用僅在共享業務訂單歸屬時才能共用收件匣。

不使用輔助類別時，呼叫 `notification.validate(&expected)`，並在業務交易中實作同等的持久去重。前端返回頁或僅驗章成功都不能直接標記履約。業務交易提交後再回傳成功確認；驗證或處理失敗應回傳錯誤回應，由廠商依協定重試。

## 閘道生命週期 {#gateway-lifecycle}

`PayManager::driver(name)` 懶解析並重用商戶設定和金鑰快照。`get_default_driver` 讀取目前預設名稱，`Pay::defaultDriver()` 解析它。`forgetDrivers` 讓後續解析使用新設定，已持有的 `Gateway` 複本保留原快照。管理器依應用隔離。輪換金鑰時重新整理相關實例，不從未驗證回呼取得驗章公鑰。

真實商戶進件、產品權限開通、公開 HTTPS 回呼及廠商端到端驗證由部署方完成。協定測試通過不代表商戶帳號已經可以實際收付款。
