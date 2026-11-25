# Webhook 错误契约与队列序列化

固定 Spatie 的 PHP 异常工厂映射为 Rust 公开 `Error` 变体及 `Result`。调用方可按配置字段或错误变体处理，接收端容器和路由错误同时保留 `source` 链。文案不复制 PHP 异常文本。旧版 `6d1a81924` 没有对应 Webhook 模块。[接收错误源码](https://github.com/spatie/laravel-webhook-client/tree/ff886d1bae33d35ad98b6c91403f280df158129d/src/Exceptions) · [发送错误源码](https://github.com/spatie/laravel-webhook-server/tree/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Exceptions)

## 接收错误

| 固定入口 | Rust 公开契约与判定点 |
| --- | --- |
| `InvalidConfig::couldNotFindConfig` | `ConfigNotFound { name }`，按匹配路由提取配置，未命中原样保留名称 |
| `invalidSignatureValidator`、`invalidWebhookProfile`、`invalidWebhookResponse` | `InvalidImplementation { field, source }`，按验证器、Profile、响应顺序解析。未配置可选响应时用默认响应。必填值缺失或字段类型错误为 `InvalidConfig(field)` |
| `invalidProcessWebhookJob` | `InvalidImplementation { field: "process_webhook_job", source }`，绑定无效在验签前拒绝。已绑定工厂在接收记录保存后构造 Job，构造失败写入接收异常 |
| `signingSecretNotSet` | 默认验证器先检查是否有签名，签名存在时空字符串或 `0` 返回 `InvalidConfig("signing_secret")` |
| `invalidPrunable` | `InvalidConfig("delete_after_days")`，原生按安全上限限制为 0 至 36,500 天，`null` 可停用清理 |
| `InvalidMethod::make` | `InvalidMethod { method }` 由公开路由注册的错误链转交，保留提交的原方法名 |
| `InvalidWebhookSignature::make` | 验证返回 false 先派发签名无效事件，再返回 `InvalidSignature`，验证器自身错误保留原因与原判定顺序 |

## 发送错误与持久边界

- `CouldNotCallWebhook::urlNotSet` 对应 `InvalidConfig("url")`。有目标时先校验 URL 和目标许可，签名开启且密钥未提供、为空或是 `0` 时返回 `InvalidConfig("signing_secret")`，发生在生成请求正文之前。使用具名凭据的投递在真正执行时解析密钥。
- `InvalidSigner` 和 `InvalidBackoffStrategy` 分别对应 `InvalidConfig("signer")`、`InvalidConfig("backoff_strategy")`。现有 Rust Builder 链保留无错误返回值，直接使用 Builder 时在 `prepare` 解析所需实现。禁用签名后不需要签名实现。PHP 类检查在配置构造及 Setter 执行，这一时机差异保留在对照表。
- `InvalidWebhookJob` 对应 `InvalidConfig("delivery.job")`。原先冻结持久快照只探测容器有没有名字，没有找到时可能把非默认 Job 当作默认任务执行。现在冻结前必须解析同名 `CallWebhookJob`，适用于 Builder 同步／队列派发、直接存储和订阅投递。队列路由还按原规则核对 Worker 注册。已保存快照的解码和历史标志保持原有含义，既有任务在消费者缺少已声明的钩子时依然保留待处理状态和尝试预算。
- `SerializesWebhookData::__serialize`／`__unserialize` 通过版本化 Rust 快照与 Queue 的公开消费者验收。PHP 将可变对象的字符串和数组属性编码后交给 Queue。Rust 仅将内部投递 ID 交给 Queue，Worker 按版本解码持久快照并恢复二进制正文与头值，凭据按引用解析。包不读写 PHP 对象序列化格式，应用无需实现该 trait 才能为 Rust 序列化出站 Job。[固定序列化 trait](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Concerns/SerializesWebhookData.php)

## 真实消费者验收

- 在 `1e133c883` 的隔离源码中叠加本批变更，使用 Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 分别运行发送端 HTTP／Artisan／独立 Worker。未注册队列路由、仅注册 Worker、同步缺少注册以及直接存储缺少注册均拒绝，未写入投递且对端未收到请求。已注册的自定义 Job 保持同步和队列回调行为。已保存的自定义投递由缺注册的 Worker 领取时保留状态与预算，恢复注册后再执行。
- 缓存前后用实际 Artisan 验证缺 URL、缺密钥、`0` 密钥、未知 Signer 和未知 Backoff。缺 URL／密钥用同时非法的正文作反例，确认报错时机先于正文处理。输出不泄漏夹具密钥或正文。真实 HTTP 与 Worker 再检查二进制请求往返、重试和队列只保存 ID。
- 接收 SQLite 回归执行 136 个真实 HTTP 请求和 83 个独立 Worker 回归场景，经过签名、路由配置错误、保留期配置、恢复及缓存入口。后续批次已接通[通用模型清理](retention.md#model-prune)。
- 两包共 42 项定向测试、三语六组示例编译、定向格式检查和两包测试目标与验收应用的严格 Clippy 均通过。接收包外部消费者对未注册 Profile、Response 及非法路由方法核对了可下转的原生错误来源。
