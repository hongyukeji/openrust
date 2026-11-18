# 簡訊

統一簡訊是 OpenRust 借鑑 Laravel [檔案儲存](filesystem.md)機制提供的擴充。`Storage::disk` 依名稱選擇由已註冊驅動程式支援的磁碟，`Sms::driver` 同樣依名稱選擇由已註冊供應商驅動程式支援的簡訊設定。多個設定可共用同一個驅動程式，並各自指定參數。

## 內建供應商

在應用程式環境中透過 `SMS_DRIVER` 選擇預設設定，並為應用程式與 Worker 設定該供應商的憑證。內建和自訂供應商都使用同一個 `Sms::driver` 入口。

| 設定 | 支援的內容 | 環境變數 |
| --- | --- | --- |
| `vonage`（預設） | 純文字 | `VONAGE_KEY`、`VONAGE_SECRET`、`VONAGE_SMS_FROM` |
| `twilio` | 純文字與廠商審核範本，範本使用具名變數 | `SMS_TWILIO_ACCOUNT_SID`、`SMS_TWILIO_API_KEY_SID`、`SMS_TWILIO_API_KEY_SECRET`，以及 `SMS_TWILIO_FROM` 或 `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | 中國內地審核範本，使用具名變數 | `SMS_ALIYUN_ACCESS_KEY_ID`、`SMS_ALIYUN_ACCESS_KEY_SECRET`、`SMS_ALIYUN_SIGN_NAME` |
| `aliyun_international` | 國際及港澳台自由文字，使用 SendMessageToGlobe | `SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID`、`SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET`。可選 `SMS_ALIYUN_INTERNATIONAL_SENDER_ID` 和 `SMS_ALIYUN_INTERNATIONAL_MESSAGE_TYPE`（`OTP`、`NOTIFY`、`MKT`） |
| `tencent` | 廠商審核範本，使用依序排列的變數 | `SMS_TENCENT_SECRET_ID`、`SMS_TENCENT_SECRET_KEY`、`SMS_TENCENT_SDK_APP_ID`、視需要設定 `SMS_TENCENT_SIGN_NAME`，以及 `SMS_TENCENT_REGION`（預設 `ap-guangzhou`） |

國際及港澳台號碼請使用帶 `+` 和國家或地區區碼的格式。`aliyun` 設定用於中國內地審核範本，接受 11 位內地手機號碼並補上 `+86`。`aliyun_international` 設定用於國際及港澳台自由文字，使用獨立憑證，預設連線至阿里雲新加坡端點。騰訊雲和 Twilio 可用各自的 `SMS_*_MARKET` 環境變數選擇 `mainland` 或 `international`。請為目標市場及設定選擇正確的寄件者和審核範本。供應商受理與手機實際收件是兩個狀態。

## 純文字與範本

用 `SmsMessage::new` 建立純文字，用 `SmsMessage::template` 傳入廠商審核範本。通知可以由 `to_sms` 傳回純文字，或由 `to_sms_template` 傳回範本。遇到不支援的訊息類型或變數格式時，驅動程式會在提交前回報錯誤。

```rust
use openrust::{
    notifications::sms::{SmsMessage, SmsTemplate},
    support::facades::Sms,
};

let template = SmsTemplate::named("SMS_123", [("code".into(), "123456".into())].into());
Sms::driver(Some("aliyun"))?
    .send(SmsMessage::template("+8613800138000", template)?)
    .await?;

let template = SmsTemplate::positional("1234", vec!["123456".into(), "5".into()]);
Sms::driver(Some("tencent"))?
    .send(SmsMessage::template("+85255550123", template)?)
    .await?;

Sms::driver(Some("aliyun_international"))?
    .send(SmsMessage::new("+85255550123", "Your order is ready.")?)
    .await?;

Sms::driver(Some("twilio"))?
    .send(SmsMessage::new("+12025550123", "Your order is ready.")?)
    .await?;
```

## 註冊供應商 {#custom-sms-providers}

透過 `Sms::driver(None)` 選擇預設設定，或用 `Sms::driver(Some("backup"))` 明確指定。直接傳送時呼叫 `driver.send(SmsMessage::new(number, text)?).await?`。在應用程式服務提供者的 `register` 中解析 `SmsManager`，以 `extend` 註冊每個驅動程式。工廠接收選定的設定，並傳回實作 `SmsDriver` 的驅動程式。`sms.drivers` 下的設定名稱可以與註冊的驅動程式名稱不同。

用 `ChannelManager::extend` 將 `SmsChannel::default()` 註冊為 `sms`，用 `SmsChannel::using("backup")` 註冊固定使用備用設定的管道。在 `Notification::via` 中選擇已註冊的管道，由 `to_sms` 傳回簡訊正文，並在 `sms` 路由鍵下以 `NotificationRoute::Sms` 指定號碼。派送程序和 Worker 使用相同的服務提供者和管道註冊。現有 `vonage` 管道仍可使用。

## 設定

以下設定預設選擇 `primary`。權杖透過環境變數傳入，應用程式執行時會將其載入對應設定。

```json
{
  "sms": {
    "default": "primary",
    "drivers": {
      "primary": {"driver": "demo-alpha", "endpoint": "http://127.0.0.1:8767/alpha"},
      "backup": {"driver": "demo-beta", "endpoint": "http://127.0.0.1:8767/beta"}
    }
  }
}
```

## 執行範例

從儲存庫根目錄執行完整範例。先在一個終端機啟動模擬端點。

```sh
python3 -B examples/sms/mock.py
```

再在另一個終端機啟動獨立 Redis，並執行範例應用程式的 Artisan 命令。

```sh
cargo build --locked --example sms
docker run -d --rm --name openrust-sms-demo -p 127.0.0.1:16379:6379 \
  --tmpfs /data redis:8 redis-server --save '' --appendonly no

export APP_BASE_PATH="$(mktemp -d)"
export APP_ENV=local CACHE_STORE=array QUEUE_CONNECTION=redis QUEUE_FAILED_DRIVER=null
export REDIS_HOST=127.0.0.1 REDIS_PORT=16379 REDIS_QUEUE=sms-demo
export SMS_ALPHA_URL=http://127.0.0.1:8767/alpha SMS_ALPHA_TOKEN=demo-alpha-token
export SMS_BETA_URL=http://127.0.0.1:8767/beta SMS_BETA_TOKEN=demo-beta-token

SMS_DRIVER=primary target/debug/examples/sms artisan sms:demo +12025550123 "First"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Second"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Third" --profile primary
SMS_DRIVER=backup target/debug/examples/sms artisan queue:work redis --stop-when-empty --tries=1
```

模擬端點會依序印出 `/alpha`、`/beta` 和 `/alpha`。為 `sms:demo` 加上 `--now` 可試用 Facade 的直接傳送。佇列儲存設定名稱、號碼和正文。Worker 從自己的設定讀取端點及憑證。派送後修改 `sms.default`，已入列任務仍使用派送時選定的設定，請保留該設定直到任務完成。

用 Ctrl+C 停止模擬服務，再執行 `docker stop openrust-sms-demo`。模擬端點只接收本機 HTTP 請求，不會傳送真實簡訊。
