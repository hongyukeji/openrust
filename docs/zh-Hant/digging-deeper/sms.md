# 簡訊

統一簡訊是 OpenRust 借鑑框架[檔案儲存](filesystem.md)機制提供的擴充。`Storage::disk` 依名稱選擇由已註冊驅動程式支援的磁碟，`Sms::driver` 同樣依名稱選擇由已註冊供應商驅動程式支援的簡訊設定。多個設定可共用同一個驅動程式，並各自指定參數。

應用在 `config/sms.rs` 中提供預設的 `log` 設定。自訂設定載入器替換簡訊設定時，必須提供 `default` 及 `drivers` 下對應的具名設定；缺少設定會回傳錯誤。

## 驅動程式生命週期 {#sms-driver-lifecycle}

`SmsManager` 沿用 具名驅動程式模式。同一應用程式首次解析設定時建立驅動程式，後續呼叫重用該驅動程式及對應範本設定；不同設定、不同應用程式各自獨立。呼叫 `default_driver` 或預設傳送方法時讀取目前預設設定名稱。

更新設定或註冊替換工廠後，清除已解析驅動程式，再重新取得：

```rust
Sms::forgetDrivers()?;
let sender = Sms::driver("aliyun")?;
```

`Sms::getDefaultDriver()` 回傳目前預設設定名稱，不建立驅動程式。`forgetDrivers` 影響之後的解析，已持有的 `SmsSender` 保留原驅動程式和設定。驅動程式建立失敗後可以重試。常駐 Worker 修改設定後，視需要重新產生設定快取並重新啟動 Worker。

## 內建供應商

在應用程式環境中透過 `SMS_DRIVER` 選擇預設設定，並為應用程式與 Worker 設定該供應商的憑證。內建和自訂供應商都使用同一個 `Sms::driver` 入口。

| 設定 | 支援的內容 | 環境變數 |
| --- | --- | --- |
| `log`（預設） | 純文字和範本。應用程式日誌記錄號碼末四碼和訊息類型。 | 無 |
| `vonage` | 純文字 | `SMS_VONAGE_API_KEY`、`SMS_VONAGE_API_SECRET`、`SMS_VONAGE_FROM` |
| `twilio` | 純文字與廠商審核範本，範本使用具名變數 | `SMS_TWILIO_ACCOUNT_SID`、`SMS_TWILIO_API_KEY_SID`、`SMS_TWILIO_API_KEY_SECRET`，以及 `SMS_TWILIO_FROM` 或 `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | 中國內地審核範本，以及透過 SendMessageToGlobe 傳送國際純文字 | `SMS_ALIYUN_ACCESS_KEY_ID`, `SMS_ALIYUN_ACCESS_KEY_SECRET`. 內地範本用 `SMS_ALIYUN_SIGN_NAME`。國際文字可設定 `SMS_ALIYUN_SENDER_ID` 和 `SMS_ALIYUN_MESSAGE_TYPE`（`OTP`、`NOTIFY`、`MKT`）。 |
| `tencent` | 廠商審核範本，使用依序排列的變數 | `SMS_TENCENT_SECRET_ID`、`SMS_TENCENT_SECRET_KEY`、`SMS_TENCENT_SDK_APP_ID`、視需要設定 `SMS_TENCENT_SIGN_NAME`，以及 `SMS_TENCENT_REGION`（預設 `ap-guangzhou`） |
| `smsbao` | 內地和國際文字 | `SMS_SMSBAO_USERNAME`, `SMS_SMSBAO_API_KEY` |
| `yunpian` | 內地文字或具名範本，以及國際文字 | `SMS_YUNPIAN_API_KEY` |

## 傳送簡訊

日常傳送使用預設設定。需要指定供應商或帳號時，可以直接傳入設定名稱。

```rust
use openrust::support::facades::Sms;

Sms::send_text("+85255550123", "Your order has shipped.").await?;

Sms::driver("aliyun")?
    .send_template("+8613800138000", "login_code", [("code", "123456")])
    .await?;
```

預設的 `log` 驅動程式記錄訊息類型和號碼末四碼。需要實際傳送時，將 `SMS_DRIVER` 設為對應的供應商或具名設定。

## 設定

### 開發環境使用 log

省略 `SMS_DRIVER` 或設為 `log`。以下呼叫只記錄訊息類型和號碼末四碼，無需聯絡簡訊供應商。

```dotenv
SMS_DRIVER=log
```

```rust
Sms::send_text("+8613800138000", "本機簡訊測試").await?;
```

### 阿里雲同時傳送內地和國際簡訊

使用阿里雲時，設定 `SMS_DRIVER=aliyun`，填寫 `SMS_ALIYUN_ACCESS_KEY_ID` 和 `SMS_ALIYUN_ACCESS_KEY_SECRET`。內地範本需要 `SMS_ALIYUN_SIGN_NAME`，國際文字則依目標市場要求填寫 `SMS_ALIYUN_SENDER_ID`。省略 `SMS_ALIYUN_MARKET` 時，同一設定可向兩個市場傳送，驅動程式會依收件號碼選擇介面和端點。國際及港澳台號碼使用 `+` 和國家或地區區碼。

自動選擇市場時，內地號碼也要加上 `+` 和國家或地區區碼。只有將設定限制為 `mainland` 時，才接受 11 位內地本地手機號碼。

在設定的 `templates` 下將業務範本名稱對應到供應商範本 ID。騰訊雲等依序接收變數的介面，也可以透過 `id` 和 `parameter_order` 將具名變數轉換為廠商要求的順序。各市場獨有的參數放在 `markets.mainland` 或 `markets.international` 下，共用憑證填在設定外層。

下例為現有 `aliyun` 設定的補充欄位。使用自訂端點時才需設定 `SMS_ALIYUN_GLOBAL_ENDPOINT`。如果某個市場使用獨立帳號，可在對應的市場下覆寫兩個憑證欄位。

```rust
"templates": {
    "login_code": "SMS_123",
},
"markets": {
    "international": {
        "endpoint": get(env, "SMS_ALIYUN_GLOBAL_ENDPOINT", Value::Null)?,
    },
},
```

驅動程式會在發出請求前檢查訊息類型。阿里雲內地簡訊需要通過審核的簽名和範本，此處接入的國際介面接受純文字。業務已持有供應商範本 ID 時，可用 `SmsMessage::template` 建立訊息並呼叫驅動程式的 `send`。

同一個業務範本名稱可以在一個市場使用廠商範本 ID，在另一個市場使用正文。在 `markets.international` 的 `templates` 中加入同名對應即可覆寫共用設定。`{code}` 之類的預留位置來自呼叫時傳入的具名變數，缺少變數時會在發出請求前回報錯誤。

```rust
"templates": {
    "login_code": "SMS_123",
},
"markets": {
    "international": {
        "templates": {
            "login_code": {"text": "Your verification code is {code}."},
        },
    },
},
```

### 簡訊寶單帳號傳送內地和國際簡訊

在 `.env` 填寫使用者名稱和 API Key。省略 `SMS_SMSBAO_MARKET`，驅動程式會依收件號碼選擇內地或國際介面。

```dotenv
SMS_DRIVER=smsbao
SMS_SMSBAO_USERNAME=your-username
SMS_SMSBAO_API_KEY=your-api-key
```

傳入帶有已審核簽名的完整正文。傳送到中國內地以外的號碼時，帶上 `+` 和國家或地區區碼。

```rust
Sms::send_text("+8613800138000", "【範例商店】您的訂單已出貨。").await?;
Sms::send_text("+85255550123", "【範例商店】您的訂單已出貨。").await?;
```

### 雲片單帳號

在 `.env` 中設定預設配置和 API Key。

```dotenv
SMS_DRIVER=yunpian
SMS_YUNPIAN_API_KEY=your-api-key
```

傳送帶有已審核簽名的完整正文，驅動程式會依號碼選擇內地或國際市場。

```rust
Sms::send_text("+8613800138000", "〖範例商店〗您的訂單已出貨。").await?;
```

### 騰訊雲範本依序傳入變數

在 `.env` 中設定騰訊雲憑證、應用程式 ID 和已審核簽名。

```dotenv
SMS_DRIVER=tencent
SMS_TENCENT_SECRET_ID=your-secret-id
SMS_TENCENT_SECRET_KEY=your-secret-key
SMS_TENCENT_SDK_APP_ID=your-app-id
SMS_TENCENT_SIGN_NAME="Your Brand"
```

將以下欄位加入 `config/sms.rs` 的 `drivers.tencent`，並將 `id` 換成通過審核的範本 ID。

```rust
"templates": {
    "login_code": {
        "id": "123456",
        "parameter_order": ["code", "minutes"],
    },
},
```

業務程式碼傳入具名變數即可。即使呼叫時先寫 `minutes`，OpenRust 也會依設定先傳送 `code`。

```rust
Sms::send_template(
    "+8613800138000",
    "login_code",
    [("minutes", "5"), ("code", "246810")],
)
.await?;
```

### Twilio 同時傳送純文字和範本

在 `.env` 中填入帳號 SID、API Key 和寄件號碼。使用 Messaging Service 時，可用 `SMS_TWILIO_MESSAGING_SERVICE_SID` 指定服務。

```dotenv
SMS_DRIVER=twilio
SMS_TWILIO_ACCOUNT_SID=ACyour_account_sid
SMS_TWILIO_API_KEY_SID=SKyour_api_key_sid
SMS_TWILIO_API_KEY_SECRET=your-api-key-secret
SMS_TWILIO_FROM=+12025550100
```

將以下欄位加入 `config/sms.rs` 的 `drivers.twilio`，並將範例 ID 換成你的 Twilio Content SID。

```rust
"templates": {
    "order_shipped": "HXyour_content_sid",
},
```

兩次呼叫都使用同一個 Twilio 設定。範本變數的鍵依實際範本填寫。

```rust
let sms = Sms::driver("twilio")?;
sms.send_text("+12025550123", "您的訂單已出貨。").await?;
sms.send_template("+12025550123", "order_shipped", [("1", "A1024")])
    .await?;
```

### Vonage 傳送純文字

設定 `SMS_DRIVER=vonage` 並填寫 Vonage 憑證及傳送身分。每個具名設定在 `config/sms.rs` 中獨立設定 `api_key`、`api_secret`、`from` 和可選的 `endpoint`。驅動自動選擇 Unicode 編碼。需要業務追蹤識別碼或送達回執位址時，在設定中指定 `client_reference` 和 `status_callback`。

```dotenv
SMS_DRIVER=vonage
SMS_VONAGE_API_KEY=your-key
SMS_VONAGE_API_SECRET=your-secret
SMS_VONAGE_FROM=YourBrand
```

```rust
Sms::send_text("+12025550123", "您的訂單已出貨。").await?;
```

## 通知和 Worker

`sms` 通知管道已預設註冊。需要固定具名設定時，可用 `ChannelManager::extend` 註冊 `SmsChannel::using("aliyun")`。實作 `to_sms` 並回傳 `SmsMessage`，統一提供純文字、供應商審核範本或設定中的業務範本。管道從 `sms` 路由鍵下的 `NotificationRoute::Sms` 取得號碼，並傳給該方法。

```rust
impl Notification for LoginCode {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["sms".into()])
    }

    fn to_sms(
        &self,
        _: &dyn Notifiable,
        to: String,
    ) -> NotificationResult<Option<SmsMessage>> {
        SmsMessage::named_template(to, "login_code", [("code", self.code.as_str())])
            .map(Some)
    }
}
```

佇列儲存選定的設定名稱和訊息。Worker 註冊相同的管道及設定名稱，從自身設定解析範本並讀取憑證。Redis 佇列可用 `./artisan queue:work redis` 啟動 Worker。重新命名前請先處理舊工作，或保留原名稱直到舊工作完成。變更設定後，若使用設定快取，請重新產生並重新啟動常駐 Worker。

傳送驗證碼等敏感內容時，可以覆寫 `Notification::should_encrypt`，為簡訊管道傳回 `true`。派送端和 Worker 須使用相同且有效的應用程式加密金鑰。佇列和失敗工作會儲存密文，供應商憑證仍由 Worker 從自己的設定讀取。

## 提交結果與送達

`send_text`、`send_template` 和 `SmsSender::submit` 回傳 `SmsSubmission`，內含供應商、提交狀態和廠商訊息 ID（如有）。`Logged` 表示已記錄在本機，`Accepted` 表示廠商已受理。業務可以儲存這筆結果，在驗證廠商回執或主動查詢後，用供應商和訊息 ID 呼叫 `record_delivery` 更新送達狀態。底層 `send` 仍回傳廠商原始回應。

明確的 HTTP 429 可以依工作策略重試。網路中斷或回應異常導致受理結果未知時，Worker 會停止自動重試。補發前請先核對廠商記錄。簡訊已提交後，`NotificationSent` 監聽器的錯誤會單獨記錄。

### Worker 當機後復原

啟動正式環境的 Worker 前執行 `./artisan migrate --force`。排入佇列的簡訊使用應用程式預設 SQL 連線的 `sms_deliveries` 資料表，即使佇列本身使用 Redis。所有 Worker 須連線至同一個記錄資料庫。

Worker 在呼叫廠商前依佇列工作的 UUID 登記，並在觸發 `NotificationSent` 之前儲存提交結果。接手的 Worker 遇到已受理的工作會直接確認佇列。如果上一次仍為 `submitting` 或結果未知，工作會列入失敗記錄以供核對。先查詢廠商記錄，再決定是否建立新工作補送。

佇列仍可能重新投遞或重試時，保留對應的傳送記錄。升級前先停止舊 Worker，核對傳送途中被中斷的歷史工作。資料表記錄升級後執行的傳送嘗試。

## 註冊供應商 {#custom-sms-providers}

在應用程式服務提供者的 `register` 中解析 `SmsManager`，呼叫 `extend` 註冊工廠。工廠接收選定的設定，回傳實作 `SmsDriver` 的驅動程式。多個具名設定可以共用同一個驅動程式。
