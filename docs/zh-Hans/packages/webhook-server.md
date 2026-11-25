# Webhook 发送（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-server` 提供请求快照、目的地校验、具名签名器、退避策略及持久投递尝试。支持具名版本化端点。

## 启用与配置 {#configuration}

运行 `./artisan init --extensions=webhook-server --no-interaction` 启用此包并执行迁移。使用 `--extensions=webhook-client,webhook-server` 可同时安装接收和发送包，也可在同一列表中加入应用需要的其他扩展。已使用标准 `all` 配置初始化的应用，可再单独执行 Webhook 安装命令。

将 `webhook-server` 加入根应用 `[features].default`，保留已有 feature，包发现会注册 `WebhookServerServiceProvider`；其他应用可直接注册。默认配置合并不访问数据库或网络。用下面命令发布模板，在 `config/mod.rs` 增加 `mod webhook_server;` 与 `("webhook-server".to_owned(), webhook_server::load(env)?),`，再重新编译。重复发布保留定制，显式 `--force` 才覆盖。

```sh
./artisan vendor:publish --tag=webhook-server-config
```

需要在应用中维护迁移时，运行 `./artisan vendor:publish --tag=webhook-server-migrations`，再重新构建。保留发布时的文件名，以沿用现有迁移账本和执行顺序。已经执行的迁移以账本为准。普通重复发布会保留本地修改。

同一应用使用两个包时，在 HTTP 和 Artisan 共用的 Provider 注册投递策略及发送 Worker。让发送 Worker 监听配置的出站队列。即使应用加载了配置缓存，签名凭据也由该 Worker 在实际发送时解析。

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

pub fn schedule_webhook_recovery(schedule: &mut openrust::console::scheduling::Schedule) {
    schedule
        .command("webhook-server:recover", ["--scope=billing".into()])
        .every_minute();
}

struct StopOnGone;

impl openrust_webhook_server::CallWebhookJob for StopOnGone {
    fn should_be_removed_from_queue(
        &self,
        attempt: &openrust_webhook_server::WebhookJobAttempt<'_>,
    ) -> bool {
        attempt.get_response().is_some_and(|response| response.status().as_u16() == 410)
    }
}

pub fn register_delivery_job(
    container: &openrust::container::Container,
    service: &openrust_webhook_server::DeliveryService,
    queue: &openrust::queue::QueueManager,
) -> Result<()> {
    openrust_webhook_server::register_call_webhook_job(
        container, "stop-on-gone", std::sync::Arc::new(StopOnGone),
    )?;
    service.register_worker(queue, "stop-on-gone")
}

pub fn register_application_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn openrust_webhook_server::ReplayPolicy>,
) -> Result<()> {
    openrust_webhook_server::register_replay_policy(container, policy)
}

pub async fn prepare_endpoint_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PreparedWebhook> {
    let endpoint = store.endpoints().find("billing").await?
        .ok_or(openrust_webhook_server::Error::EndpointRevoked)?;
    invoice_call()?.use_endpoint(&endpoint)
        .prepare(&openrust_webhook_server::TargetPolicy::default())
}

pub fn configure_webhook_limits(container: &openrust::container::Container) -> Result<()> {
    use openrust_webhook_server::{DeliveryLimit, DeliveryLimits, register_delivery_limits};

    register_delivery_limits(container, DeliveryLimits {
        sends_per_scope: Some(DeliveryLimit::new(100, 60)?),
        sends_per_endpoint: Some(DeliveryLimit::new(20, 60)?),
        replays_per_scope: Some(DeliveryLimit::new(5, 60)?),
    })
}

pub async fn publish_invoice(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::PublishedMessage> {
    use openrust_webhook_server::{EventType, PublishOptions, TargetPolicy};

    store.publish(
        "invoice-paid-inv-123",
        &EventType::new("invoice.paid", 1)?,
        serde_json::json!({"invoice_id": "inv-123"}),
        &PublishOptions::default(),
        &TargetPolicy::default(),
    ).await
}

pub fn grant_invoice_tls(
    policy: openrust_webhook_server::TargetPolicy,
    ca: &[u8],
    client_chain: &[u8],
    client_key: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    openrust_webhook_server::DeliveryPermission::new(policy)
        .with_tls_ca(ca)?
        .with_mutual_tls(client_chain, client_key)
}


pub fn grant_invoice_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedSocksProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}


pub fn grant_invoice_connect_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
) -> Result<openrust_webhook_server::DeliveryPermission> {
    let proxy = openrust::http::client::PinnedHttpProxy::new(address)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub fn grant_invoice_https_proxy(
    permission: openrust_webhook_server::DeliveryPermission,
    address: std::net::SocketAddr,
    server_name: &str,
    ca_pem: &[u8],
) -> Result<openrust_webhook_server::DeliveryPermission> {
    use openrust::http::client::{PinnedHttpProxy, TlsCertificate};
    let roots = TlsCertificate::from_pem_bundle(ca_pem)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    let proxy = PinnedHttpProxy::https(address, server_name, roots)
        .map_err(|_| openrust_webhook_server::Error::InvalidConfig("proxy"))?;
    permission.with_proxy("corp-egress", proxy)
}

pub async fn inspect_invoice_attempts(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
    after: u32,
) -> Result<openrust_webhook_server::DeliveryAttemptsPage> {
    store.inspect_attempts(id, after, 25).await
}


pub async fn preview_invoice_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::DeliveryPruneReport> {
    store.prune(openrust_webhook_server::DeliveryRetention::Payloads, 30, 100, true).await
}

pub async fn preview_shared_body_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::PublicationPruneReport> {
    store.prune_publication_bodies(30, 100, after, true).await
}

pub async fn hold_invoice_delivery(
    store: &openrust_webhook_server::DeliveryStore,
    id: &str,
) -> Result<bool> {
    store.set_retention_hold(openrust_webhook_server::RetentionRecord::Delivery, id, true).await
}

pub async fn preview_expiring_invoice_keys(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::IdempotencyPruneReport> {
    store.prune_idempotency_keys(openrust_webhook_server::IdempotencyKind::Delivery, 30, 100, true).await
}

pub async fn preview_invoice_history_pruning(
    store: &openrust_webhook_server::DeliveryStore,
    after: Option<&str>,
) -> Result<openrust_webhook_server::HistoryPruneReport> {
    store.prune_history(openrust_webhook_server::IdempotencyKind::Delivery, 90, 100, after, true).await
}

pub async fn preview_invoice_replay_audit_pruning(
    store: &openrust_webhook_server::DeliveryStore,
) -> Result<openrust_webhook_server::ReplayAuditPruneReport> {
    store.prune_replay_audit(90, 100, true).await
}

```


### 准备出站请求 {#prepare}

在已启动的应用中调用 `WebhookCall::create()` 读取包配置，或使用 `WebhookCall::new()` 获取内置默认值。设置目的地、载荷和凭据，再调用 `prepare(&TargetPolicy::default())`。快照保留稳定消息 ID、已序列化的载荷及调度配置。`headers_at` 按尝试时间戳为已保存的字节生成签名头。凭据由应用在执行时解析的场景可传入具名凭据引用。

POST 等正文方法使用已准备的字节。GET 将载荷编码为 RFC 3986 查询串，Spatie 签名继续使用已准备的 JSON。原始正文模式可以保留任意字节。快照限制正文为 16 MiB、Header 为 64 KiB。通过 `register_signer` 和 `register_backoff_strategy` 把外部实现注册到应用容器。

默认目的地策略要求 HTTPS，并复核解析出的全部 Socket 地址。内部端点使用应用可信配置提供固定来源、路径与地址。`PreparedWebhook::bind_target` 可验证单次尝试的地址列表。HTTP 客户端固定实际连接地址，保留 Host 与 SNI，禁止重定向，默认直连并校验 HTTPS 证书。

### 配置 TLS {#tls}

使用私有 CA 或双向 TLS 时，应用的 `DeliveryPolicy` 先授权范围和具体目的地，再通过 `DeliveryPermission::with_tls_ca(&pem_bundle)` 为该对端替换系统信任库，或通过 `with_mutual_tls(&certificate_chain, &private_key)` 提供客户端身份。Worker 从可信应用配置读取 PEM 材料，私钥由应用在运行时解密。队列、投递快照和事件均不包含 PEM 材料。连接保留目的地主机名用于 SNI 和证书校验。

隔离测试中，调用 `do_not_verify_ssl()` 且投递策略同时返回 `allow_unverified_tls()` 时，才关闭服务端证书及主机名校验。未得到策略许可、指定了自定义 CA 或附带客户端证书时，该请求会被拒绝。调用 `verify_ssl(true)` 可恢复校验。

### 使用具名出站代理 {#proxy}

在 `webhook-server.proxy` 中设置已批准的名称，例如 `corp-egress`，也可为单次投递调用 `use_proxy(Some("corp-egress".into()))`。调用 `use_proxy(None)` 可选择直连。应用策略授权投递范围和目的地后，用 `DeliveryPermission::with_proxy` 批准相同名称。可覆写 `DeliveryPolicy::authorize_transport` 读取所请求的名称。投递记录只保存名称。用可信配置中的数字 Socket 地址创建代理，并在运行时提供可选凭据。

代理配置也接受 `{"https":"corp-egress","no":[".internal.example"]}` 一类对象，按目标协议选择具名代理。单次投递可调用 `use_proxy_map(ProxyMap::new(...))`。普通域名也匹配子域，前导点仅匹配子域，列表还支持 IP 字面量和 `*`。命中排除项或协议未配置名称时，按同一目的地策略直连。固定地址代理传输要求目标使用经校验的 HTTPS。投递记录保存选中的名称，Worker 发送时重新解析并授权该名称。

可用 `PinnedSocksProxy` 进行本地解析的 SOCKS5 转发。使用 CONNECT 时，可用 `PinnedHttpProxy::https` 指定代理的数字地址、证书名称和可信 CA 证书。Worker 分别校验代理和收件端，收件端的 mTLS 身份只用于收件端。代理收到已校验的目标 IP 和端口，收件端 TLS 和 HTTP 使用原主机。使用 `PinnedHttpProxy::new` 建立明文 HTTP CONNECT 时，链路上的代理认证资料未加密，应部署在可信网络。缺少许可或代理连接错误时，投递不回退为直连。目前不支持 SOCKS5 远程 DNS 或向代理提供 TLS 客户端证书。

### 登记和投递 {#delivery}

启用包后执行 `./artisan migrate`。用应用数据库连接和投递范围构建 `DeliveryStore`。业务变更需要与投递登记原子提交时，使用同库的 `submit_in_transaction`。单独登记则调用 `submit`。同一键与相同快照返回已有投递 ID，同一键的不同快照返回冲突。需要重复登记时，先为快照指定稳定的消息 ID。投递 ID 和发布消息 ID 均以返回值为准。同一范围的键继续返回原结果，也适用于幂等映射迁移之前的记录。部署新写入路径前先执行迁移，并停止旧版本写入进程。

通过 `register_delivery_policy` 注册应用策略。每次尝试都按当前配置核对投递范围、URL 和凭据引用，在运行时解析密钥。保存的快照拒绝内联密钥与敏感请求头，队列载荷只含投递 ID。避免把敏感值放进 URL、正文和元数据。

队列发送需要在各 Worker 进程用 `DeliveryService::register_worker` 注册准备时选定的 Job 名称，统一具名数据库、投递范围和策略，并配置 `database` 等直接供 Worker 使用的队列连接。同一投递同一时刻只允许一个领取。HTTP 2xx 视为成功，其余状态按配置重试。请求超时或进程中断可能使远端结果未知，接收方可按稳定的消息 ID 去重。

Builder 的 `dispatch`、`dispatch_if` 和 `dispatch_unless` 登记投递并入队。对应的 `dispatch_sync` 系列登记后在当前进程尝试。条件方法传入 Rust 布尔值，跳过时直接返回 `None`。开启 `throw_exception_on_failure(true)` 后，同步投递最终失败会返回带投递 ID 的类型化错误，队列投递则交由原生 Worker 记录失败任务。

投递成功后重试同步调用时，沿用同一范围的幂等键和快照。即使重试由使用同一数据库的另一个应用实例处理，也会返回已有投递 ID，且不会再次发送请求。

在 `openrust_webhook_server::events` 下注册同步监听器。`DispatchingWebhookCallEvent` 在投递记录提交后、入队前触发。`WebhookCallSucceededEvent` 和 `WebhookCallFailedEvent` 在尝试结果提交后触发，最终失败随后还会派发 `FinalWebhookCallFailedEvent`。共享载荷提供消息与投递 ID、请求方法、URL 路径、正文和元数据的访问器，以及尝试状态。事件省略 URL 查询串、请求头的值和响应内容。入队前监听器失败时可从持久记录继续投递，结果监听器失败时以已提交的结果为准。可用 `transfer_stats()` 查看收到响应时的 HTTP 传输耗时，用 `transport_failure()` 区分超时、连接失败、响应超限等脱敏类别。自定义 Job 钩子也可读取同一失败类别。

```sh
./artisan webhook-server:send <delivery-id> --scope=billing
./artisan webhook-server:send <delivery-id> --scope=billing --queue
./artisan webhook-server:send --scope=billing --limit=100
```

### 定时恢复 {#recovery}

在应用调度器中注册 `webhook-server:recover`，并每分钟运行 `./artisan schedule:run`。命令按投递记录的 Worker 路由入队，报告候选、成功入队、过期或耗尽、跳过与派发失败的数量。`--pretend` 仅统计候选。投递存储使用具名连接时，在命令里指定 `--database`。

恢复进程在入队前预留 60 秒唤醒窗口。未入队或丢失的任务可在窗口到期后补派。Worker 领取或提交重试结果时会清除窗口，下一次尝试继续受投递租约与退避控制。单批选择不超过 `--limit` 条，派发预算为 30 秒。多个调度进程通过共享数据库协调，各节点需提供记录对应的 Worker 路由。调度进程在领取后退出时，另一节点可在窗口到期后的调度周期恢复该投递。

```sh
./artisan webhook-server:recover --scope=billing --limit=100
./artisan webhook-server:recover --scope=billing --pretend
```

### 自定义发送 Job {#custom-job}

实现 `CallWebhookJob`，在生产者、Scheduler 和 Worker 用 `register_call_webhook_job` 与 `DeliveryService::register_worker` 注册同一名称，再用 `.use_job("stop-on-gone")` 选用。`should_be_removed_from_queue` 可停止原投递预算允许的重试。`failed` 在一次尝试提交最终失败并派发结果事件后运行。两个钩子都应快速同步返回。

钩子可以读取原始正文、URL、标签、元数据及有大小上限的原生响应。传输、凭据解析、尝试记录及持久状态由包统一管理。已保存的自定义投递在运行时要求存在同名钩子，缺失时撤销领取且不扣减尝试额度。同步发送和队列发送在保存使用非默认 Job 的新投递前，都要求先注册对应的 `CallWebhookJob`。队列 Worker 路由和 Job 钩子需要分别注册。

### 人工重放投递 {#replay}

使用重放前，在应用中注册 `ReplayPolicy`。核实实际操作者身份及其对指定范围和端点的权限。应用可用 `ReplayPhase::Request` 实施操作限流，并在 `ReplayPhase::Attempt` 复核执行许可。

重放登记一条带审计记录及独立尝试额度和截止时间的投递。它沿用原消息 ID、正文与目的地，可按授权改用具名签名凭据。调度器通过同一 Worker 流程发送。默认只重放最终失败的投递。已成功的原投递须显式传入 `--include-succeeded`，并取得更高权限。命令报告两条投递的 ID 和新代次。

```sh
./artisan webhook-server:replay <delivery-id> --scope=billing --actor=operator-42 --reason="confirmed receiver recovery"
```

### 管理端点版本 {#endpoints}

在应用授权的操作中使用 `DeliveryStore::endpoints()`，按名称、URL 和可选凭据引用登记端点。使用 `.use_endpoint(&endpoint)` 绑定发送。`replace`、`pause`、`resume`、`delete` 需要传入预期版本，以拒绝过期的管理操作。

暂停期间待处理投递保留原截止时间，恢复后符合条件的投递继续。修改 URL 或凭据创建新版本，供新投递使用。旧版本及已删除端点的投递会在执行时取消。已发出的请求仍可能在对端完成，尝试历史保留实际响应。人工重放也检查端点当前许可。直接指定 URL 的发送沿用应用投递策略。Worker 缺少已批准修订的凭据时，会在发送前拒绝该尝试。

### 共享投递限额 {#limits}

在各节点的应用中注册 `DeliveryLimits`。范围发送限额覆盖直接 URL、队列投递、自动重试和人工重放发送。具名端点还有独立计数，同一端点各配置版本共用。人工重放登记可设独立的范围限额。窗口从首次保留额度开始，按数据库时钟计算。

发送达到限额时，投递等待窗口恢复，保留原尝试额度及截止时间。重放登记达到限额时返回带下次可申请 Unix 时间戳的错误。共享投递数据库的所有节点应使用一致的限额配置。

### 查询投递与尝试 {#inspect}

使用 `./artisan webhook-server:inspect <id> --scope=<scope>` 查询投递 ID。查询 `publish` 返回的内部消息 ID 时添加 `--message`。添加 `--attempts --limit=25` 读取一页尝试，下一页通过 `--after` 传入返回的 `next_after`。时间字段以 Unix 秒表示。使用 `--database` 选择具名数据库连接。

查询返回状态、时间、HTTP 状态码及有限的尝试结果。已发布消息的摘要还包含事件类型、schema 版本和首次投递 ID。包不注册诊断 HTTP 路由，应用授权范围后可调用 `DeliveryStore` 的只读方法。Artisan 允许选择数据库和范围，仅交给可信运维人员。诊断输出不包含请求 URL、正文、Header、签名材料或远端响应正文。

### 清理出站投递数据 {#prune}

先用 `./artisan webhook-server:prune --scope=<scope> --days=30 --pretend` 预演候选记录。移除 `--pretend` 后清理已保存的请求快照，保留投递 ID、状态和幂等摘要。从投递截止时间起算保留期。添加 `--attempts` 单独清理已完成的尝试，保留期按完成时间计算。每批最多 500 条，默认 100 条。使用 `--database` 选择具名连接。

启用清理前先执行迁移，并升级所有可登记重放的进程。仅清理终态投递。清理后，原始记录无法再登记新重放，已有重放仍可使用自身保存的请求。诊断结果以 Unix 秒返回 `payload_pruned_at`。

使用 `--messages` 回收达到发布留存期限的共享正文。首次投递及所有已登记重放的请求快照都已清理后，该正文才可回收。下一页通过 `--after` 传入返回的 `next_after`，下一轮从头检查此前未满足回收条件的消息。消息 ID、原始投递 ID 和幂等摘要继续保留。`webhook-server:inspect --message` 在清理后显示 `body_pruned_at`。

### 设置出站业务保留 {#retention-hold}

使用 `./artisan webhook-server:hold <id> --scope=<scope>` 保留一条投递的快照和尝试。添加 `--message` 可按发布消息 ID 保留共享正文，添加 `--release` 可解除保留。消息保留仅保护共享正文；投递保留也会保护其现存快照引用的共享正文。解除保留后仍须达到正常留存期限才会清理。

已设置保留的记录在诊断中显示 `retention_hold_at`。保留只保护仍存在的数据。使用保留前先执行迁移，并升级所有运行清理的进程。应用授权范围后可调用 `DeliveryStore::set_retention_hold`。Artisan 命令仅交给可信运维人员。

查看单次尝试时，运行 `./artisan webhook-server:inspect <delivery-id> --scope=billing --transport=1`，或调用 `DeliveryStore::inspect_attempt_transport`。结果返回以微秒计的实测网络耗时或脱敏传输失败类别。Worker 退出后由租约回收标记的尝试等记录，两项可以都是空值。

### 释放到期幂等键 {#idempotency-expiry}

使用 `./artisan webhook-server:prune --scope=<scope> --days=30 --idempotency=delivery --pretend` 预演直接投递键到期。发布消息使用 `--idempotency=publication`。期限从请求快照或共享正文清理时起算，业务保留会阻止到期。移除 `--pretend` 后释放符合条件的键。启用前先执行迁移，并升级所有写入及清理进程。

重用已释放的键会生成新记录和 ID，旧队列任务仍指向旧投递。历史记录保留原 ID，并在诊断中显示 `idempotency_expired_at`。若接收方应将续用键的直接投递视为新事件，应用需设置新的业务消息 ID。

### 清理历史元数据 {#history-retention}

使用 `./artisan webhook-server:prune --scope=<scope> --days=90 --history=delivery --pretend` 预演历史投递清理。发布回执使用 `--history=publication`。期限从幂等键到期起算，使用返回的 `next_after` 与 `--after` 继续分页。下一轮从头重新检查此前未满足清理条件的记录。

投递历史只有在尝试均已清理、重放审计和现存共享正文都未引用它时才可删除。发布回执需要先清理正文并释放幂等键。业务保留会阻止两类清理。历史元数据删除后，诊断无法再查询该 ID。

### 清理重放审计 {#replay-retention}

使用 `./artisan webhook-server:prune --scope=<scope> --days=90 --replay-audit --pretend` 预演审计清理，移除 `--pretend` 后执行。两条投递均须已清理请求快照，且各自的幂等键到期均已达到指定留存期限，同时两条记录均没有业务保留。每批最多清理 500 条审计。

该操作清理重放关系及操作者元数据。原投递和重放投递仍按各自的历史清理规则回收。原快照已清理时仍无法登记新的重放。

### 按订阅发布 {#subscriptions}

用 `DeliveryStore::subscribe` 为具名端点订阅确切事件名和版本。用 `unsubscribe` 取消之后的订阅。发布时提交 JSON 负载，正文包含 `type`、`schema_version` 和 `data`。消息、共享正文和当次订阅端点的全部投递在同一事务提交。`publish_in_transaction` 可将发布纳入同库业务事务。

单次发布支持至多 32 个匹配端点和 1 MiB 的消息正文。相同范围、键和输入重复提交时返回原收据，包括零投递的结果。同一键的输入发生变化时返回冲突。各端点和重放沿用同一共享正文与消息 ID。事务提交后由既有调度器恢复派发。

## 自定义签名与退避 {#customization}

外部 crate 可实现 `Signer`。`SigningContext` 提供不可变正文、URL、运行时凭据、稳定消息 ID 和单次尝试时间戳，这是上游签名参数的 Rust 适配。密钥以借用传入，上下文不支持序列化或 Debug 输出。`headers` 方法支持协议附加的认证头。不要把密钥加入请求元数据、日志或普通队列载荷。

`BackoffStrategy::wait_in_seconds_after_attempt` 返回秒数。默认 `ExponentialBackoffStrategy` 沿固定基线：尝试1至4分别等待10、100、1000、10000秒，超过4等待100000秒。底层契约的0返回1秒，首个实际尝试编号为1。


## 使用边界 {#scope}

网络和任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)与[队列](../digging-deeper/queues.md)。待重试的投递到期后，可再次唤醒队列任务或使用有界扫描。

### 单机与多副本 {#deployment}

多个副本使用同一个可写 PostgreSQL 或 MySQL 数据库保存投递状态和限额，各 Worker 使用同一套 database 或 Redis 队列。所有 Worker 都须能读取凭据引用和已授权的配置版本。多个副本上的调度器使用 `on_one_server()` 时，需要共享 database 或 Redis 缓存。单机上同一应用的调度进程可以使用同一个本地目录中的 file 缓存。SQLite 适用于单机。上述 database 与 Redis 组合已在一台主机的独立应用进程中验收，跨主机部署前须验证实际基础设施。

投递数据库不可达时，登记及发送均失败。Redis 恢复连接且补偿领取窗口到期后，定时恢复可继续处理待投递记录，任务仍使用原先配置的队列。

应用扩缩容时，保留节点的 Worker 继续访问原投递数据库并沿用原队列配置。新节点准备好配置、路由和 Job 注册后接入负载均衡。缩容时先将节点移出负载均衡，再停止进程。

执行兼容的增量迁移时，由一个发布进程迁移数据库，逐个将旧实例从负载均衡移出并替换。过渡期间两个版本使用同一队列。升级所有写入和清理进程后，再启用新增的幂等键到期及清理命令。
