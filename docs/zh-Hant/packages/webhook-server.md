# Webhook 發送（擴展）

## 支援狀態 {#support-status}

可選套件 `openrust-webhook-server` 提供簽名器、退避契約與套件設定，尚未提供完整發送 Builder、持久投遞、重試或端點管理。產生簽名頭不等於發起網路投遞。

## 啟用與設定 {#configuration}

將 `webhook-server` 加入根應用 `[features].default`，保留既有 feature，套件發現會註冊 `WebhookServerServiceProvider`；其他應用可直接註冊。預設設定合併不存取資料庫或網路。用以下命令發布範本，在 `config/mod.rs` 增加 `mod webhook_server;` 與 `("webhook-server".to_owned(), webhook_server::load(env)?),`，再重新編譯。重複發布保留自訂，明確指定 `--force` 才覆寫。

```sh
./artisan vendor:publish --tag=webhook-server-config
```

## 可用入口 {#available-entry-points}

### 簽名原始位元組 {#signatures}

`DefaultSigner` 使用 HMAC-SHA256，以小寫十六進位寫入 `Signature`。預設演算法只涵蓋本文，修改 URL、訊息 ID 或時間戳不會改變該簽名。`DefaultSigner::new` 可更換簽名頭。`StandardWebhookSigner` 重用官方 `standardwebhooks 1.3.0`，回傳協定要求的三個頭；金鑰須為合法 base64，可帶 `whsec_` 前綴，本文須為 UTF-8 位元組。

`StandardWebhookSigner::headers` 要求非空 ASCII 訊息 ID，最多256位元組，不得包含控制字元或首尾空白，以確保產生的頭經過 HTTP 解析後保持一致。

```rust
use openrust::http::HeaderMap;
use openrust_webhook_server::{
    DefaultSigner, Result, Signer, SigningContext, StandardWebhookSigner,
};

pub fn spatie_signature(body: &[u8], secret: &str) -> Result<String> {
    DefaultSigner::default().calculate_signature(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id: "message-1", timestamp: 0,
    })
}

pub fn standard_headers(
    body: &[u8], secret: &str, message_id: &str, timestamp: i64,
) -> Result<HeaderMap> {
    StandardWebhookSigner.headers(&SigningContext {
        webhook_url: "https://example.com/webhook",
        body, secret, message_id, timestamp,
    })
}
```

## 自訂簽名與退避 {#customization}

外部 crate 可實作 `Signer`。`SigningContext` 提供不可變本文、URL、執行時憑據、穩定訊息 ID 和單次嘗試時間戳，這是上游簽名參數的 Rust 適配。金鑰以借用傳入，上下文不支援序列化或 Debug 輸出。`headers` 方法支援協定附加的認證頭。不要把金鑰加入請求中繼資料、日誌或一般佇列載荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 回傳秒數。預設 `ExponentialBackoffStrategy` 沿固定基線：嘗試1至4分別等待10、100、1000、10000秒，超過4等待100000秒。底層契約的0回傳1秒，首個實際嘗試編號為1。

這些工具本身不提供 SSRF 保護、持久領取、去重或 HTTP 重試迴圈，只能在已提供相應保證的應用流程內使用；套件尚未開放生產投遞服務。

## 使用邊界 {#scope}

應用網路與任務執行重用既有 [HTTP 用戶端](../digging-deeper/http-client.md)和[佇列](../digging-deeper/queues.md)。這些套件基礎契約不提供可靠業務處理或投遞狀態機。
