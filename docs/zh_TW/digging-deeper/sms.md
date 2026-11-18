# 簡訊

統一簡訊是 OpenRust 借鑑 Laravel [檔案儲存](filesystem.md)機制提供的擴充。`Storage::disk` 依名稱選擇由已註冊驅動程式支援的磁碟，`Sms::driver` 同樣依名稱選擇由已註冊供應商驅動程式支援的簡訊設定。多個設定可共用同一個驅動程式，並各自指定參數。

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
