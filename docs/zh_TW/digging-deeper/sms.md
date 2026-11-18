# 簡訊

統一簡訊是 OpenRust 借鑑 Laravel [檔案儲存](filesystem.md)機制提供的擴充。`Storage::disk` 依名稱選擇由已註冊驅動程式支援的磁碟，`Sms::driver` 同樣依名稱選擇由已註冊供應商驅動程式支援的簡訊設定。多個設定可共用同一個驅動程式，並各自指定參數。

## 內建供應商

在應用程式環境中透過 `SMS_DRIVER` 選擇預設設定，並為應用程式與 Worker 設定該供應商的憑證。內建和自訂供應商都使用同一個 `Sms::driver` 入口。

| 設定 | 支援的內容 | 環境變數 |
| --- | --- | --- |
| `log`（預設） | 純文字和範本。應用程式日誌記錄號碼末四碼和訊息類型。 | 無 |
| `vonage` | 純文字 | `VONAGE_KEY`、`VONAGE_SECRET`、`VONAGE_SMS_FROM` |
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

使用阿里雲時，設定 `SMS_DRIVER=aliyun`，填寫 `SMS_ALIYUN_ACCESS_KEY_ID` 和 `SMS_ALIYUN_ACCESS_KEY_SECRET`。內地範本需要 `SMS_ALIYUN_SIGN_NAME`，國際文字則依目標市場要求填寫 `SMS_ALIYUN_SENDER_ID`。省略 `SMS_ALIYUN_MARKET` 時，同一設定可向兩個市場傳送，驅動程式會依收件號碼選擇介面和端點。國際及港澳台號碼使用 `+` 和國家或地區區碼。

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

## 通知和 Worker

以 `ChannelManager::extend` 將 `SmsChannel::default()` 註冊為 `sms`，也可以用 `SmsChannel::using("aliyun")` 綁定具名設定。通知可用 `to_sms` 提供正文，或用 `to_sms_template` 提供供應商範本。要使用設定中的業務範本名稱時，實作 `to_sms_message` 並回傳 `SmsMessage::named_template`。收件號碼使用 `sms` 路由鍵下的 `NotificationRoute::Sms`。

佇列儲存選定的設定名稱和訊息。Worker 註冊相同的管道及設定名稱，從自身設定解析範本並讀取憑證。Redis 佇列可用 `./artisan queue:work redis` 啟動 Worker。重新命名前請先處理舊工作，或保留原名稱直到舊工作完成。變更設定後，若使用設定快取，請重新產生並重新啟動常駐 Worker。

## 註冊供應商 {#custom-sms-providers}

在應用程式服務提供者的 `register` 中解析 `SmsManager`，呼叫 `extend` 註冊工廠。工廠接收選定的設定，回傳實作 `SmsDriver` 的驅動程式。多個具名設定可以共用同一個驅動程式。內建的 `vonage` 通知管道繼續使用原有的 `VONAGE_*` 環境變數。
