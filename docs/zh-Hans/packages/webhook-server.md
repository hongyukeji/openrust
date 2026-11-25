# Webhook 发送（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-server` 提供请求快照、目的地校验、具名签名器、退避策略及持久投递尝试。自动调度、重放与端点管理仍在开发。

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

fn invoice_call() -> Result<openrust_webhook_server::WebhookCall> {
    use openrust_webhook_server::WebhookCall;

    Ok(WebhookCall::new()?
        .url("https://billing.example.com/callback")
        .uuid("invoice-issued-inv-123")
        .payload(serde_json::json!({"invoice_id": "inv-123"}))
        .use_secret_reference("billing-v1")
        .on_connection(Some("database".into())))
}

pub fn prepare_invoice() -> Result<openrust_webhook_server::PreparedWebhook> {
    invoice_call()?.prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub async fn submit_invoice(store: &openrust_webhook_server::DeliveryStore) -> Result<String> {
    store.submit("invoice-issued-inv-123", &prepare_invoice()?, 3600).await
}

pub async fn queue_invoice(
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<openrust_webhook_server::QueuedDelivery> {
    invoice_call()?.dispatch(
        service, queue, &openrust_webhook_server::TargetPolicy::default(),
        "invoice-issued-inv-123", 3600,
    ).await
}
```


### 准备出站请求 {#prepare}

在已启动的应用中调用 `WebhookCall::create()` 读取包配置，或使用 `WebhookCall::new()` 获取内置默认值。设置目的地、载荷和凭据，再调用 `prepare(&TargetPolicy::default())`。快照保留稳定消息 ID、已序列化的载荷及调度配置。`headers_at` 按尝试时间戳为已保存的字节生成签名头。凭据由应用在执行时解析的场景可传入具名凭据引用。

POST 等正文方法使用已准备的字节。GET 将载荷编码为 RFC 3986 查询串，Spatie 签名继续使用已准备的 JSON。原始正文模式可以保留任意字节。快照限制正文为 16 MiB、Header 为 64 KiB。通过 `register_signer` 和 `register_backoff_strategy` 把外部实现注册到应用容器。

默认目的地策略要求 HTTPS，并复核解析出的全部 Socket 地址。内部端点使用应用可信配置提供固定来源、路径与地址。`PreparedWebhook::bind_target` 可验证单次尝试的地址列表。HTTP 客户端固定实际连接地址，保留 Host 与 SNI，禁止重定向和代理转发，HTTPS 默认证书校验。

### 登记和投递 {#delivery}

启用包后执行 `./artisan migrate`。用应用数据库连接和投递范围构建 `DeliveryStore`。业务变更需要与投递登记原子提交时，使用同库的 `submit_in_transaction`。单独登记则调用 `submit`。同一键与相同快照返回已有投递 ID，同一键的不同快照返回冲突。需要重复登记时，先为快照指定稳定的消息 ID。

通过 `register_delivery_policy` 注册应用策略。每次尝试都按当前配置核对投递范围、URL 和凭据引用，在运行时解析密钥。保存的快照拒绝内联密钥与敏感请求头，队列载荷只含投递 ID。避免把敏感值放进 URL、正文和元数据。

队列发送需要在各 Worker 进程用 `DeliveryService::register_worker` 注册准备时选定的 Job 名称，统一具名数据库、投递范围和策略，并配置 `database` 等直接供 Worker 使用的队列连接。同一投递同一时刻只允许一个领取。HTTP 2xx 视为成功，其余状态按配置重试。请求超时或进程中断可能使远端结果未知，接收方可按稳定的消息 ID 去重。

Builder 的 `dispatch`、`dispatch_if` 和 `dispatch_unless` 登记投递并入队。对应的 `dispatch_sync` 系列登记后在当前进程尝试。条件方法传入 Rust 布尔值，跳过时直接返回 `None`。开启 `throw_exception_on_failure(true)` 后，同步投递最终失败会返回带投递 ID 的类型化错误，队列投递则交由原生 Worker 记录失败任务。

在 `openrust_webhook_server::events` 下注册同步监听器。`DispatchingWebhookCallEvent` 在投递记录提交后、入队前触发。`WebhookCallSucceededEvent` 和 `WebhookCallFailedEvent` 在尝试结果提交后触发，最终失败随后还会派发 `FinalWebhookCallFailedEvent`。共享载荷提供消息与投递 ID、请求方法、URL 路径、正文和元数据的访问器，以及尝试状态。事件省略 URL 查询串、请求头的值和响应内容。入队前监听器失败时可从持久记录继续投递，结果监听器失败时以已提交的结果为准。

```sh
./artisan webhook-server:send <delivery-id> --scope=billing
./artisan webhook-server:send <delivery-id> --scope=billing --queue
./artisan webhook-server:send --scope=billing --limit=100
```

## 自定义签名与退避 {#customization}

外部 crate 可实现 `Signer`。`SigningContext` 提供不可变正文、URL、运行时凭据、稳定消息 ID 和单次尝试时间戳，这是上游签名参数的 Rust 适配。密钥以借用传入，上下文不支持序列化或 Debug 输出。`headers` 方法支持协议附加的认证头。不要把密钥加入请求元数据、日志或普通队列载荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 返回秒数。默认 `ExponentialBackoffStrategy` 沿固定基线：尝试1至4分别等待10、100、1000、10000秒，超过4等待100000秒。底层契约的0返回1秒，首个实际尝试编号为1。


## 使用边界 {#scope}

网络和任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)与[队列](../digging-deeper/queues.md)。待重试的投递到期后，可再次唤醒队列任务或使用有界扫描。
