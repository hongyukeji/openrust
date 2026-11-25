# Webhook 接收（扩展）

## 支持状态 {#support-status}

可选包 `openrust-webhook-client` 提供具名配置、签名验证器、筛选与响应契约。持久接收记录、业务任务与恢复尚未提供。验签成功不代表已可靠接收或完成业务处理，包不会自动安装公开回调路由。

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
```

`StandardSignatureValidator` 复用官方 `standardwebhooks 1.3.0`，使用 `webhook-id`、`webhook-timestamp`、`webhook-signature`。密钥为合法 base64 或带 `whsec_` 前缀的形式。支持五分钟时间窗与单个头内的多个签名，拒绝重复协议头。该库要求 UTF-8 正文；验签不替代持久事件去重。

## 自定义实现与快照 {#customization}

外部 crate 可实现 `SignatureValidator`、`WebhookProfile`、`RespondsToWebhook`。`ProcessEverythingWebhookProfile` 选择所有请求，`DefaultRespondsTo` 生成 `{"message":"ok"}`；应用应先完成自己要求的持久接收步骤，再使用响应契约。包尚无完整 Processor 或业务 Worker。

`WebhookConfigRepository::add_config` 按名称替换配置，已取得的 `Arc<WebhookConfig>` 保留原修订与密钥快照。未知名称返回 `Ok(None)`，解析错误仍为错误。Provider 将仓库绑定到当前 HTTP、Artisan 或 Worker 生命周期，运行时追加不会泄漏到下一生命周期。`WebhookConfig::new` 接受运行时注入的密钥，私有保存并对 Debug 脱敏；不要把密钥序列化到任务或日志。`with_verification_secrets` 最多增加七个旧密钥，同一配置总共最多八个。

配置缓存保存凭据引用，因此包默认值不会让 `config:show webhook-client` 输出解析后的密钥。各进程按自己的不可变环境快照解析配置；变更凭据后重启常驻进程。跨进程重启与排队任务所需的历史凭据保留尚未实现，`revision` 标签不能代替持久凭据修订库。

## 使用边界 {#scope}

应用网络与任务执行复用现有 [HTTP 客户端](../digging-deeper/http-client.md)和[队列](../digging-deeper/queues.md)。这些包基础契约不提供可靠业务处理或投递状态机。
