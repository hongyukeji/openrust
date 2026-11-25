# Webhook 发送（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-server` 提供签名器、退避契约与包配置，尚未提供完整发送 Builder、持久投递、重试或端点管理。生成签名头不等于发起网络投递。

## 启用与配置 {#configuration}

将 `webhook-server` 加入根应用 `[features].default`，保留已有 feature，包发现会注册 `WebhookServerServiceProvider`；其他应用可直接注册。默认配置合并不访问数据库或网络。用下面命令发布模板，在 `config/mod.rs` 增加 `mod webhook_server;` 与 `("webhook-server".to_owned(), webhook_server::load(env)?),`，再重新编译。重复发布保留定制，显式 `--force` 才覆盖。

```sh
./artisan vendor:publish --tag=webhook-server-config
```

## 可用入口 {#available-entry-points}

### 签名原始字节 {#signatures}

`DefaultSigner` 使用 HMAC-SHA256，以小写十六进制写入 `Signature`。默认算法只覆盖正文，修改 URL、消息 ID 或时间戳不会改变该签名。`DefaultSigner::new` 可更换签名头。`StandardWebhookSigner` 复用官方 `standardwebhooks 1.3.0`，返回协议要求的三个头；密钥须为合法 base64，可带 `whsec_` 前缀，正文须为 UTF-8 字节。

`StandardWebhookSigner::headers` 要求非空 ASCII 消息 ID，最多256字节，不得包含控制字符或首尾空白，以保证生成的头经过 HTTP 解析后保持一致。

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

## 自定义签名与退避 {#customization}

外部 crate 可实现 `Signer`。`SigningContext` 提供不可变正文、URL、运行时凭据、稳定消息 ID 和单次尝试时间戳，这是上游签名参数的 Rust 适配。密钥以借用传入，上下文不支持序列化或 Debug 输出。`headers` 方法支持协议附加的认证头。不要把密钥加入请求元数据、日志或普通队列载荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 返回秒数。默认 `ExponentialBackoffStrategy` 沿固定基线：尝试1至4分别等待10、100、1000、10000秒，超过4等待100000秒。底层契约的0返回1秒，首个实际尝试编号为1。

这些工具本身不提供 SSRF 保护、持久领取、去重或 HTTP 重试循环，只能在已提供相应保证的应用流程内使用；包尚未开放生产投递服务。

## 使用边界 {#scope}

应用网络与任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)和[队列](../digging-deeper/queues.md)。这些包基础契约不提供可靠业务处理或投递状态机。
