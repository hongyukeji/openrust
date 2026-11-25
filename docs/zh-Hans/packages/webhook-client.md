# Webhook 接收（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-client` 提供具名配置、类型化实现工厂、显式回调路由、`WebhookController`、手动 `WebhookProcessor` 和按范围隔离的数据库接收记录。应用提供任务派发实现。数据库接收支持按已验签的事件 ID 去重，可显式启用。支持显式开启恢复扫描和私有 Storage 磁盘引用。保存记录及派发成功不代表业务完成，包不会自动安装公开回调路由。

## 启用与配置 {#configuration}

将 `webhook-client` 加入根应用 `[features].default`，保留已有 feature，包发现会注册 `WebhookClientServiceProvider`。其他应用也可直接依赖 crate 并注册该 Provider。注册与启动不查询数据库、不联系供应商。

默认配置名为 `default`，签名头为 `Signature`，凭据引用指向 `WEBHOOK_CLIENT_SECRET`。通过进程环境或受保护的应用 `.env` 提供密钥；包配置只保存环境变量名称，不保存解析后的密钥。自定义时发布模板，在 `config/mod.rs` 增加 `mod webhook_client;` 与 `("webhook-client".to_owned(), webhook_client::load(env)?),`，再重新编译。重复发布保留定制，显式 `--force` 才覆盖。

```sh
./artisan vendor:publish --tag=webhook-client-config
```

## 可用入口 {#available-entry-points}

### 验证原始字节 {#signatures}

读取 HTTP 流时先按配置限制正文大小，再创建 `WebhookRequest`。保留原始正文和请求头值。默认验证器使用 HMAC-SHA256，要求完全一致的小写十六进制签名；不签 URL 或时间戳，不提供抗重放时间窗。重复签名头会被拒绝。

正文默认上限为1 MiB，`with_max_body_bytes` 最多允许16 MiB。请求快照最多接受128个头值，头名和值合计不超过64 KiB。配置来源最多128个配置，名称与修订标签最多128字节，单个密钥最多8192字节。HTTP 传输层还可能有更低的限制。

自定义验证器若签名包含请求目标，使用 `WebhookRequest::with_uri` 附上原始 `Request::uri()`，保留查询串编码。

```rust
use openrust::{foundation::Application, http::{HeaderMap, Method}};
use openrust_webhook_client::{
    DefaultSignatureValidator, Error, SignatureValidator,
    WebhookConfigRepository, WebhookRequest,
};

pub async fn verify_signature(
    app: &Application,
    headers: HeaderMap,
    original_body: Vec<u8>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let configs = app.make::<WebhookConfigRepository>().await?;
    let config = configs.get_config("default")?
        .ok_or(Error::InvalidConfig("name"))?;
    let request = WebhookRequest::new(Method::POST, headers, original_body)?;
    Ok(DefaultSignatureValidator.is_valid(&request, &config)?)
}

pub async fn receive(
    request: &WebhookRequest,
    config: &openrust_webhook_client::WebhookConfig,
    connection: openrust::database::Connection,
    scope: &str,
    job: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
    events: std::sync::Arc<dyn openrust::contracts::events::Dispatcher>,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{DatabaseWebhookStore, WebhookProcessor, WebhookRuntime};
    let store = std::sync::Arc::new(DatabaseWebhookStore::new(connection, scope)?);
    let runtime = WebhookRuntime::new(store, job, events);
    WebhookProcessor::new(request, config, &runtime).process().await
}

pub fn register_routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    use openrust_webhook_client::{WebhookRouteOptions, WebhookRoutes};
    Route::webhooks("/callbacks/payments", WebhookRouteOptions::new().name("payments"))
}

pub fn register_dispatcher(
    container: &openrust::container::Container,
    dispatcher: std::sync::Arc<dyn openrust_webhook_client::WebhookJobDispatcher>,
) -> openrust_webhook_client::Result<()> {
    openrust_webhook_client::WebhookRuntime::register_job_dispatcher(
        container, "payments", move |_, _| Ok(dispatcher.clone()),
    )
}

pub async fn process_named(
    app: std::sync::Arc<Application>,
    config: std::sync::Arc<openrust_webhook_client::WebhookConfig>,
    request: &WebhookRequest,
) -> openrust_webhook_client::Result<openrust::http::Response> {
    use openrust_webhook_client::{WebhookProcessor, WebhookRuntime};
    let owner = app.clone();
    app.call(openrust::container::Callback::new(&[], move |_| async move {
        let runtime = WebhookRuntime::from_application(owner, config.clone()).await;
        Ok(match runtime {
            Ok(runtime) => WebhookProcessor::new(request, &config, &runtime).process().await,
            Err(error) => Err(error),
        })
    }), Default::default()).await?
}
```

`StandardSignatureValidator` 复用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。密钥为合法 base64 或带 `whsec_` 前缀的形式。支持五分钟时间窗与单个头内的多个签名，拒绝重复协议头。该库要求 UTF-8 正文；如需抑制同一已验签事件的重复投递，开启数据库事件去重。

### 注册回调路由与实现 {#routes}

导入 `WebhookRoutes` 后调用 `Route::webhooks(uri, options)` 或 `Route::webhooks_with(uri, options, configure)`。`WebhookRouteOptions::default()` 使用 `default` 配置及 `post` 方法，可用 `name`、`methods` Builder 指定其他配置和方法列表。注册参数只接受小写 `get`、`query`、`post`、`put`、`patch`、`delete`；GET 继承原生 HEAD 匹配，但不能把 `head` 作为注册选项。大写或不支持的方法使注册失败；原生 OPTIONS 协商及405／Allow行为保留。

在应用现有 API 或 web 路由登记中注册。导入 `WebhookRouter` 后，底层 `Router` 也提供 `webhooks` 扩展，应在应用路由登记期间调用。组前缀、名称和中间件保留，`webhooks_with` 可设置原生参数约束、默认值和路由中间件。包不会关闭 CSRF；供应商使用 web 路由时，仅在应用请求伪造防护配置中显式排除该回调路径，其他 web 路由继续受保护。

配置从路由器实际匹配的路由名选择，不跟随请求的自定义路由解析器，也不由 URL／正文里传入的账号选择。默认名称为 `webhook-client-{name}`，`add_unique_token_to_route_name=true` 追加八位随机后缀；这是命名机制，不是鉴权。与固定源码一致，唯一令牌模式按第一个点分隔名称，因此该模式下应使用不含点的配置名。

在应用 Provider 中调用 `WebhookRuntime::register_signature_validator`、`register_webhook_profile`、`register_webhook_response`、`register_webhook_store` 或 `register_job_dispatcher`。工厂接收不可变 `WebhookConfig` 和当前应用 `Container`，返回对应 trait 的 `Arc`。`WebhookRuntime::from_application` 需要 await；构造与处理使用指定应用的原生上下文，结束后恢复调用者。HTTP 控制器让请求依赖工厂与处理共用一次请求借用，请求句柄不能带出该借用。注册名区分大小写，最多128字节，只允许 ASCII 字母、数字及 `-`、`_`、`.`、`:`。未知或类型错误的绑定会失败，不回退到其他应用或账号。

配置字段依次为 `signature_validator`、`webhook_profile`、`webhook_response`、`webhook_model`、`process_webhook_job`。内置验证器为 `default`／`standard`，Profile 为 `process-everything`，响应为 `default`，存储为 `database`。任务派发器必须显式注册；目前 `process_webhook_job` 选择派发器，原生队列处理可用 `QueuedWebhookJob`。数据库配置还需要可信稳定 `scope`，可选 `connection` 为数据库连接名，`null` 使用应用默认连接。发布模板保留空 scope 和空任务名，未配置的接收端会报错，不会假确认。 必须显式选择 `signature_validator`、`webhook_profile`、`webhook_model`、`process_webhook_job`，缺失字段会被拒绝；只有 `webhook_response` 可省略或为 null，并使用默认响应。

例如具名 `payments` 配置可使用 `scope="billing/site-a"`、`signing_secret={"env":"PAYMENT_WEBHOOK_SECRET"}`、内置默认验证器／Profile／响应、database 存储及 `process_webhook_job="payments"`。将这些字段写入发布后的配置，并在 Provider 注册 payments 派发器。手动消费者也可通过 `WebhookConfig` 的 `with_signature_validator`、`with_webhook_profile`、`with_webhook_response`、`with_webhook_model`、`with_process_webhook_job`、`with_scope`、`with_connection` Builder 创建配置。

验证器、Profile 和响应在处理前构造；仅在验签和 Profile 通过后解析存储，有效但忽略的事件不访问存储。任务工厂先检查注册，保存后、清除旧异常前才构造派发器。缺失配置和无效签名是普通错误，默认控制器按固定源码返回500；应用可用[异常处理](../the-basics/errors.md)自行映射。正文超限保留413。前面直接调用验证器的示例仍可自行决定响应策略。

控制器复用原生 `Request::body_bytes_with_limit`，保留原文，即使中间件已解析 JSON／form，也会检查已缓存正文是否超过端点上限；该方法不提高原生8 MiB读取上限。前置中间件可能已经按自己的上限读取正文，较小端点限制不能倒退减少此前的内存使用。

## 自定义实现与快照 {#customization}

外部 crate 可实现 `SignatureValidator`、`WebhookProfile`、`RespondsToWebhook`、`WebhookStore` 和 `WebhookJobDispatcher`。`ProcessEverythingWebhookProfile` 选择所有请求，`DefaultRespondsTo` 生成 `{"message":"ok"}`。`WebhookRuntime::new` 接受显式依赖；`WebhookRuntime::from_application` 按当前应用的类型化工厂解析选定名称，不执行类名字符串，也不使用全局注册表。

`WebhookConfigRepository::add_config` 按名称替换配置，已取得的 `Arc<WebhookConfig>` 保留原修订与密钥快照。未知名称返回 `Ok(None)`，解析错误仍为错误。Provider 将仓库绑定到当前 HTTP、Artisan 或 Worker 生命周期，运行时追加不会泄漏到下一生命周期。`WebhookConfig::new` 接受运行时注入的密钥，私有保存并对 Debug 脱敏；不要把密钥序列化到任务或日志。`with_verification_secrets` 最多增加七个旧密钥，同一配置总共最多八个。

配置缓存保存凭据引用，因此包默认值不会让 `config:show webhook-client` 输出解析后的密钥。各进程按自己的不可变环境快照解析配置；变更凭据后重启常驻进程。跨进程重启与排队任务所需的历史凭据保留尚未实现，`revision` 标签不能代替持久凭据修订库。

### 保存与派发接收记录 {#receipts}

数据库存储仅在验签及 Profile 通过后保存解析输入。用 `call.payload(&store).await?` 读取 JSON、表单及查询参数，同名字段以正文为准。升级前保存的原文记录返回 `None`。用 `call.get_attachments(&store).await?` 读取从持久原文重建的原生上传对象分组，数据库不保存服务器上传路径。`store_attachments=false` 关闭附件视图，原始 multipart 正文仍包含文件字节。

Provider 注册 `webhook_calls` 迁移，使用 `DatabaseWebhookStore` 前运行 `./artisan migrate`。传入最多128字节的稳定应用／租户范围，同一范围的各节点使用相同值。读取和异常更新按该范围隔离，摘要键避免数据库排序规则混同大小写不同的范围。这是存储隔离，不是调用者授权；范围必须从可信应用上下文取得。

`WebhookProcessor::process` 依次验签、Profile 筛选、保存、清除旧异常、派发任务、生成响应。无效签名先派发强类型 `InvalidWebhookSignatureEvent` 再返回错误；事件让应用内可信监听器读取请求和配置，不自动提供可序列化的诊断属性。验证器报错仍是错误。有效但忽略的请求只生成约定响应。存储或派发失败不会返回成功响应；派发错误保存有界的通用异常记录，不保存任意第三方错误文本或堆栈。

`models::WebhookCall` 的 `store_webhook`、`find`、`save_exception` 和 `clear_exception` 委托存储适配。记录保留原始正文、`with_uri` 提供的请求目标、方法、修订与选择的请求头，包括重复头和非 UTF-8 头值。存储记录本身不是验证令牌；`VerifiedWebhook` 仅由 Processor 在验证及 Profile 通过后生成。数据库对原始字节使用 base64，这不是加密；应用仍需控制数据库访问和保留期限。

默认不保存请求头。`with_store_headers(&["X-Source"])` 不区分头名大小写，`with_store_headers(&["*"])` 明确保存全部头，可能包含敏感值。配置文件的全选值使用字符串 `"*"`，不能写成包含 `"*"` 的数组。异常 JSON 最多8192字节，请求目标最多8192字节，持久方法名最多32字节；手动传入的异常详情须由应用脱敏。

#### 将 multipart 附件保存在私有 Storage 磁盘 {#attachment-disk}

将 `attachment_disk` 设为已配置的私有磁盘名，并运行 `./artisan migrate`。手动构造 profile 时可调用 `with_attachment_disk("local")`。开启附件保存后，接受的含文件 multipart 请求会作为一个私有对象写入。接收记录保存磁盘、长度和 SHA-256 摘要。对象名由可信范围和接收 ID 生成。Worker 核对长度和摘要后读取原始正文并还原原生上传文件。各 Worker 必须能通过已保存的磁盘名读取同一存储。

使用共享存储时，为各节点的 `attachment_disk` 配置同名 S3 磁盘。使用私有桶，并确保后端支持设置和验证私有对象 ACL。启用 ACL 的 SeaweedFS 4.53 已通过双节点验收。存储失联时，新的附件接收在确认前失败，恢复后可重试并保留原始字节。

已有内联记录和配置了 `store_attachments: false` 的请求继续使用数据库存储。数据库明确回滚时，接收器尝试删除暂存对象。提交前崩溃或存储结果不确定时，可能留下待清理的暂存登记。清理过期接收记录时，会在同一数据库事务中删除记录，并登记解除引用的对象。

定期运行 `./artisan webhook-client:prune --scope=payments --attachment-staging`，清理已解除引用的对象及暂存至少一小时的对象。用 `--pretend` 查看候选数量。删除失败时保留登记，租约到期后可重试，命令以错误状态退出。接收器、Worker 和清理进程应将已保存的磁盘名映射到同一私有存储。即使停用了接收记录保留期清理，也可以执行附件清理。

回滚附件迁移前先停止 Webhook 写入。存在有效引用或暂存登记时，迁移拒绝回滚。

### 在 Worker 中处理接收记录 {#processing-jobs}

实现 `jobs::ProcessWebhookJob` 处理已保存的接收记录。在 HTTP 和 Worker 共用的应用 Provider 中，调用 `QueuedWebhookJob::register`，传入原生 `QueueManager`、稳定名称、可信范围的存储及处理器。再按 `process_webhook_job` 选择的名称注册 `QueuedWebhookJob` 派发器。不同存储范围使用各自的注册名。Worker 对照原记录核对 ID、范围、配置名和修订号后才调用处理器。

派发器可指定队列连接和名称，原生 `JobOptions` 控制尝试次数、退避及超时。业务策略由处理器决定。队列载荷保存接收记录标识，Worker 直接读取已验收的记录，无需接收方的验签密钥。处理器失败时记录固定异常，下次尝试清除上次诊断。请选择由 Worker 消费的连接。派发器拒绝内置 sync、null、deferred 和 background 连接，并要求队列返回已入队结果。

### 同事务保存接收记录与数据库队列任务 {#atomic-database-queue}

在配置中设置 `atomic_database_queue: true`，或在运行时调用 `with_atomic_database_queue(true)`。让 `DatabaseWebhookStore` 和 `QueuedWebhookJob` 使用同一应用数据库连接。接收记录和队列任务一起提交后才返回 HTTP 确认，队列写入失败时一并回滚。其他驱动或分开的连接实例会被拒绝，即使指向同一个数据库。该选项默认关闭，开启后覆盖入队前的进程退出窗口。签名协议提供已认证事件 ID 时，可启用事件去重。

### 接收记录提交后恢复派发 {#dispatch-recovery}

在 profile 中设置 `recover_dispatch: true`，或在 Rust 中调用 `with_dispatch_recovery(true)`。运行 `./artisan migrate`。此模式在同一事务中提交已验签接收记录和待派发意图，再返回配置的 HTTP 确认。该 profile 的 `atomic_database_queue` 须关闭。

在接收器、恢复进程和 Worker 中调用 `QueuedWebhookJob::register_with_database_recovery` 注册带租约的处理器，使用一致的处理器名称、接收数据库及范围。通过派发器的 `on_connection` 和 `on_queue` 明确指定持久队列连接和队列名。任务超时必须为正值，且比 Worker 租约至少短两秒。内建进程内连接和 failover 连接会被拒绝。Redis 等持久直连队列可以与接收数据库分开运行。

为每个接收范围调度有界命令，并将已保存的队列连接名始终映射到同一队列后端。使用 `--database` 选择具名接收连接。

```sh
./artisan webhook-client:recover --scope=billing/site-a --limit=100 --pretend
./artisan webhook-client:recover --scope=billing/site-a --limit=100
```

派发失败时保留意图，命令以失败状态退出。60 秒后可再次领取。队列已接受任务、恢复进程却在清除意图前退出时，重试可能产生重复队列任务。各 Worker 共享接收记录的领取与完成状态。业务副作用须实现幂等，以应对处理过程中的中断。

#### 补派已入列但逾期未完成的记录 {#processing-recovery}

对已启用 `recover_dispatch: true` 的 profile，设置 `"recover_processing": {"after_seconds": 300, "max_requeues": 2}`，或调用 `with_processing_recovery(300, 2)?`。运行 `./artisan migrate`。等待时间可设为 60–86,400 秒，补派预算为 1–10 次，两者随接收记录保存。等待时间应覆盖正常队列积压、重试和退避。

同一个定时命令按已保存的队列路由补派逾期且未完成的记录，跳过仍有业务租约或尚未到预定处理时间的记录。每次领取补派占用一次预算，包括队列确认前中断的尝试。预算耗尽后停止自动补派，已入列任务仍可执行。记录完成或终止后，下次恢复命令移除派发跟踪行。`--pretend` 只预览可派发的记录。Worker 在产生副作用后退出时仍可能重复处理，业务处理器须实现幂等。

### 多 Worker 领取接收记录 {#processing-lease}

先运行 `./artisan migrate`，再用 `QueuedWebhookJob::register_with_database_lease(&queue, name, store, handler, lease_seconds)` 注册处理器。存储参数为 `Arc<DatabaseWebhookStore>`。协调重启消费该队列的各个 Worker，使它们使用同一注册。例如任务超时 20 秒、租约 30 秒、数据库队列 `retry_after` 60 秒。租约范围为 3–86,400 秒，任务必须显式设置正数超时且至少比租约短两秒。应为数据库操作留出余量，并让队列保留时间或可见性超时长于租约。竞争中的任务通过原生队列释放，可能消耗一次队列尝试预算。

租约有效时，仅领取者进入业务处理器。已完成记录被重复投递时直接确认，处理器也可以主动释放任务以便再次执行。进程退出后，队列保留和记录租约都过期时可重新领取。业务端应按接收记录 ID 做外部操作幂等，因为进程可能在外部操作生效后、记录完成前退出。

数据库按接收记录和代次保存每次领取。完成、释放、延期、失败或停止会与相应的接收状态变更一起记录。崩溃后由另一 Worker 重新领取时，前一次领取标记为过期。尝试记录保存时间和状态，随接收记录保留。

### 按已验签事件去重 {#verified-event-deduplication}

在 profile 中选择 `atomic_database_queue: true` 或 `recover_dispatch: true`，再启用 `deduplicate_verified_events: true`，并运行 `./artisan migrate`。运行时配置可结合所选派发选项使用 `with_verified_event_deduplication(true)`。`standard` 验证器提供签名覆盖的 `webhook-id`，该模式要求 ID 不含半角句点。自定义验证器可在确认签名覆盖稳定事件 ID 后，通过 `SignatureValidator::verified_event_id` 提供该 ID。默认 HMAC 仅覆盖正文，无法提供事件 ID。启用去重时，验证器不能提供有效 ID 则拒绝接收。为每个来源账号配置独立的 scope 或 profile。

接收记录、去重键和原生队列任务在同一事务提交。相同 ID 和请求内容的重试返回配置的确认响应，只保存一条接收记录及一条任务。同一 scope 和 profile 内，已使用的 ID 携带不同的 HTTP 方法、原始路径和查询串、正文或 Content-Type 时返回 HTTP 409。签名时间戳、签名和凭据修订允许在重试时变化。去重键随接收记录保留，接收记录的保留期应覆盖供应商的重投递窗口。默认关闭。

### 无稳定事件 ID 时按请求摘要去重 {#body-deduplication}

在启用 `atomic_database_queue: true` 或 `recover_dispatch: true` 的 profile 中将 `deduplicate_body_seconds` 设为 1–3,600。运行时可结合所选派发选项调用 `with_body_deduplication(seconds)`。每个 profile 选择已验签事件 ID 去重或摘要窗口去重。接收方在通过验签和 Profile 筛选后应用窗口。

窗口以首次接受请求时的数据库时间起算。摘要涵盖 HTTP 方法、原始路径和查询串、原始正文及 Content-Type。窗口内完全相同的请求共用一条接收记录及任务，过期后可创建新记录，早先的接收记录仍可读取。只有在业务允许把窗口内完全相同的合法请求视为重试时，才启用此模式。

### 延期处理尚未关联的通知 {#deferred-processing}

通知可能早于业务记录到达时，用 `QueuedWebhookJob::register_with_database_deferred_processing` 注册 Worker。原生数据库队列须与接收存储共用同一数据库连接。设置 `DeferredProcessingPolicy { max_deferrals, max_age_seconds }`，并沿用上文的租约、任务超时和队列保留时间。最多可配置 1–1,000 次延期，有效期为 2 秒至 30 天，从首次延期开始计时。

覆盖 `ProcessWebhookJob::handle_outcome`，在业务记录尚未就绪时返回 `WebhookProcessingOutcome::Deferred { delay_seconds }`。延迟必须为正数并处于剩余期限内。业务处理成功返回 `Completed`，按原生队列策略重试返回 `RetryableFailure`，立即停止返回 `PermanentFailure`。延期时在同一事务保存下次执行时间并入队，新任务使用独立的队列尝试预算。记录保持待处理，直至 Worker 完成处理或延期预算耗尽。返回 `Deferred` 时不要同时调用 `JobContext::release`。

业务记录就绪并完成关联后，应用可调用 `QueuedWebhookJob::wake_deferred(&store, queue, receipt_id)` 提前唤醒。传入可信范围对应的存储及应用的 `Arc<QueueManager>`。方法沿用延期时保存的处理器和队列地址。成功安排提前唤醒返回 `true`，记录已到期可领取、已终结或不属于该范围时返回 `false`。

使用 `recover_dispatch: true` 和独立队列后端时，调用 `QueuedWebhookJob::register_with_database_recovery_and_deferred_processing` 注册 Worker。接收记录事务一并保存延期状态和下次派发意图，定时运行的 `webhook-client:recover` 到期后派发。队列后端离线时仍可登记提前唤醒。即使已确认入列的唤醒任务丢失且未启用丢失补派，恢复命令仍能在总期限到期后直接终止记录。到期或提前唤醒接续的队列任务有独立的尝试次数，保留接收记录的延期预算及已配置的丢失补派预算。

### 查询并重放已耗尽的接收记录 {#replay-exhausted}

先运行 `./artisan migrate`，并升级所有 Webhook Worker，再启用重放。`webhook-client:exhausted` 按可信范围列出接收标识和元数据，每次限定 1–500 条。记录须已确认入列、自动补派预算耗尽且等待窗口已过，同时没有活跃租约、计划中的延期或终态。

在命令、派发器及 Worker 共用的 Provider 中注册应用的 `ReplayPolicy`。策略根据可信应用上下文验证操作人，核对接收范围和配置或凭据修订的当前许可，并给出 1–10 个重放代次和 60–86,400 秒有效期。`--actor` 和 `--reason` 是策略收到的审计输入。

```rust
use openrust_webhook_client::replay::{self, ReplayPolicy};

pub fn register_replay_policy(
    container: &openrust::container::Container,
    policy: std::sync::Arc<dyn ReplayPolicy>,
) -> openrust_webhook_client::Result<()> {
    replay::register_policy(container, policy)
}
```

```sh
./artisan webhook-client:exhausted --scope=billing/site-a --limit=20
./artisan webhook-client:replay <receipt-id> --scope=billing/site-a --actor=operator-id --reason="Receipt reviewed"
./artisan webhook-client:recover --scope=billing/site-a
```

申请成功后会登记一个新代次，并交由 `webhook-client:recover` 派发。恢复命令和 Worker 都会再次核对应用策略。旧代次的队列任务无法领取该记录。重放沿用原接收 ID、验签历史和原始载荷。审计记录随接收记录保留，业务副作用仍需由消费方保证幂等。

### 清理过期接收记录 {#retention}

包级 `delete_after_days` 默认 30 天，设为 `null` 可停用保留期清理。应用可通过调度器定时运行 Artisan 命令。传入接收记录的可信 scope，可用 `--database` 选择具名连接。

```sh
./artisan webhook-client:prune --scope=billing/site-a --pretend
./artisan webhook-client:prune --scope=billing/site-a --limit=100
```

数据库存储只清理创建时间和终态时间都超过保留期的记录。待处理记录、仍持有租约的记录及缺少终态标记的记录会保留。删除接收记录会一并删除关联去重键和处理尝试。每条命令最多处理 500 条。`WebhookCall::prunable` 通过存储适配器预览候选记录 ID。

### 无签名回调的查单提示 {#unverified-hints}

供应商无法提供签名时，可在 profile 中将 `unverified_hint_receiver` 指向通过 `WebhookRuntime::register_unverified_hint_receiver` 注册的实现。接收器拿到 `UnverifiedHint`。默认验证器仅在签名头缺失时进入此路径。Standard 验证器要求三个协议头全部缺失。携带签名字段的请求继续走常规验签与响应。自定义验证器若使用其他签名字段，可覆盖 `SignatureValidator::has_signature`。内置接收器要求存储范围与 profile 配置的 `scope` 一致。

`QueuedWebhookHint` 使用与提示存储共用连接的数据库队列。通过 `WebhookHintPolicy` 只关联可信范围内已登记的本地任务，再由独立 `ProcessWebhookHint` 在查单前复核任务。各部署节点共用限流缓存。内置适配器按范围和 profile 实施每分钟 1–600 次的配置限额。已接受的提示与未知任务均返回相同的 HTTP 202。提示记录保存本地任务引用，五分钟后失去查单资格，每条提示只可领取一次，任务超时为 15 秒。

查单处理器须按业务契约独立验证供应商响应。原始通知始终保留为无签名提示，提示任务只携带提示 ID 和修订。可信接收记录和队列继续处理已验签回调。

### 保留验签判定依据 {#verification-audit}

在设有可信 `scope` 的 profile 中启用 `verification_audit: true`，并运行 `./artisan migrate`。审计使用 profile 的 `connection`。账本保存验证器和策略名称、profile 修订、UTC 分钟以及分类和固定原因。修订是配置中的凭据集合标签，凭据集合变化时应更新。

每个范围、profile、修订、验证器、策略和原因在一分钟内只保留一条观测。审计记录业务处理前的可信度判定，区分已验签、无签名提示和拒绝。审计只保存上述元数据。启用审计后，写入失败会返回错误并中止后续业务处理。

```sh
./artisan webhook-client:prune --scope=billing/site-a --verification-audit --pretend
./artisan webhook-client:prune --scope=billing/site-a --verification-audit
```

该命令按包级 `delete_after_days` 清理验签判定记录。

## 使用边界 {#scope}

应用网络与任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)和[队列](../digging-deeper/queues.md)。持久恢复覆盖已提交的接收记录派发。各队列后端和部署拓扑需分别验收。

### 单机与多副本 {#deployment}

为所有副本配置同一套可写 PostgreSQL 或 MySQL 数据库。同库原子接收和入队使用 database 队列，任务投递到 Redis 时可配置持久派发恢复。HTTP 节点与 Worker 使用一致的队列名称、应用密钥及已授权的配置版本。持久附件使用所有 Worker 都能访问的私有存储。本地验收在同一主机使用独立节点目录和共享目录。跨主机部署前须验证实际共享存储和部署环境。

配置了持久派发恢复的接收记录，在队列失联期间仍可于数据库提交后确认。为所选队列连接安排恢复任务。接收数据库不可达时，请求失败，发送方可重试。
