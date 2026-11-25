# Webhook 接收（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-client` 提供具名配置、签名验证器、手动 `WebhookProcessor` 和按范围隔离的数据库接收记录。应用提供任务派发实现。自动回调路由、解析后的载荷／附件、去重和恢复尚未提供。保存记录及派发成功不代表业务完成，包不会自动安装公开回调路由。

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
```

`StandardSignatureValidator` 复用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。密钥为合法 base64 或带 `whsec_` 前缀的形式。支持五分钟时间窗与单个头内的多个签名，拒绝重复协议头。该库要求 UTF-8 正文；验签不替代持久事件去重。

## 自定义实现与快照 {#customization}

外部 crate 可实现 `SignatureValidator`、`WebhookProfile`、`RespondsToWebhook`、`WebhookStore` 和 `WebhookJobDispatcher`。`ProcessEverythingWebhookProfile` 选择所有请求，`DefaultRespondsTo` 生成 `{"message":"ok"}`。通过应用的存储、任务派发器与事件派发器构造 `WebhookRuntime`，用其 Builder 方法选择验证器、Profile 与响应；配置中的类名字段尚不会自动解析这些实现。

`WebhookConfigRepository::add_config` 按名称替换配置，已取得的 `Arc<WebhookConfig>` 保留原修订与密钥快照。未知名称返回 `Ok(None)`，解析错误仍为错误。Provider 将仓库绑定到当前 HTTP、Artisan 或 Worker 生命周期，运行时追加不会泄漏到下一生命周期。`WebhookConfig::new` 接受运行时注入的密钥，私有保存并对 Debug 脱敏；不要把密钥序列化到任务或日志。`with_verification_secrets` 最多增加七个旧密钥，同一配置总共最多八个。

配置缓存保存凭据引用，因此包默认值不会让 `config:show webhook-client` 输出解析后的密钥。各进程按自己的不可变环境快照解析配置；变更凭据后重启常驻进程。跨进程重启与排队任务所需的历史凭据保留尚未实现，`revision` 标签不能代替持久凭据修订库。

### 保存与派发接收记录 {#receipts}

Provider 注册 `webhook_calls` 迁移，使用 `DatabaseWebhookStore` 前运行 `./artisan migrate`。传入最多128字节的稳定应用／租户范围，同一范围的各节点使用相同值。读取和异常更新按该范围隔离，摘要键避免数据库排序规则混同大小写不同的范围。这是存储隔离，不是调用者授权；范围必须从可信应用上下文取得。

`WebhookProcessor::process` 依次验签、Profile 筛选、保存、清除旧异常、派发任务、生成响应。无效签名先派发强类型 `InvalidWebhookSignatureEvent` 再返回错误；事件让应用内可信监听器读取请求和配置，不自动提供可序列化的诊断属性。验证器报错仍是错误。有效但忽略的请求只生成约定响应。存储或派发失败不会返回成功响应；派发错误保存有界的通用异常记录，不保存任意第三方错误文本或堆栈。

`models::WebhookCall` 的 `store_webhook`、`find`、`save_exception` 和 `clear_exception` 委托存储适配。记录保留原始正文、`with_uri` 提供的请求目标、方法、修订与选择的请求头，包括重复头和非 UTF-8 头值。存储记录本身不是验证令牌；`VerifiedWebhook` 仅由 Processor 在验证及 Profile 通过后生成。数据库对原始字节使用 base64，这不是加密；应用仍需控制数据库访问和保留期限。

默认不保存请求头。`with_store_headers(&["X-Source"])` 不区分头名大小写，`with_store_headers(&["*"])` 明确保存全部头，可能包含敏感值。配置文件的全选值使用字符串 `"*"`，不能写成包含 `"*"` 的数组。异常 JSON 最多8192字节，请求目标最多8192字节，持久方法名最多32字节；手动传入的异常详情须由应用脱敏。

应用实现的 `WebhookJobDispatcher` 应仅把记录 ID 和可信范围入队，在 Worker 中重新读取记录，不序列化密钥或整个模型。已接收的排队记录不应因签名时间窗过期而重新验签。本入口尚未补齐提交后派发崩溃窗口、请求去重、处理租约和自动注册的业务 Job，使用时须保留这些限制。

## 使用边界 {#scope}

应用网络与任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)和[队列](../digging-deeper/queues.md)。持久恢复、form／附件解析、公开路由／控制器及完整投递状态机仍未实现。
