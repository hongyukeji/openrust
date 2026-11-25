# Webhook 发送 Job 扩展

## 固定基线与应用契约

固定 Spatie Server 的 `useJob` 要求 Job 属于该包，`CallWebhookJob` 提供状态判断、正文和响应访问、标签以及终态失败回调。Rust 用外部 crate 可实现的 `CallWebhookJob` trait 接入停止重试与终态失败观察。包原生任务仍执行网络、签名、许可、预算及持久状态流程。应用可通过 `WebhookJobAttempt` 显式借用请求原始正文、原始目标 URL、范围、ID、标签、元数据和当前尝试的有界原生响应；该上下文只在进程内使用。[固定 Builder](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php) · [固定 CallWebhookJob](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php)

只有命中现有重试条件且还有尝试余额的失败才进入停止重试钩子。最终失败先提交状态、再派发结果事件、最后执行 `failed`。失败钩子返回错误时记录脱敏警告，原有同步错误和原生 Worker 失败行为仍由 `throw_exception_on_failure` 决定。同步与 Worker 使用同一回调，调度器仅派发队列任务。过期扫描收尾和进程崩溃导致的 Unknown 补记没有当前尝试上下文，未补发钩子。

应用在所有消费者注册相同名称。持久快照仅保存自定义选择标志和 Job 名称，普通及以前保存的快照默认仍可解码。旧版进程读取带自定义标志的新快照时会拒绝未知字段，启用新 Job 前先部署生产者和消费者。恢复入队时核对已保存的钩子，Worker 领取时在同一事务中核对任务注册名与快照记录。缺少注册或队列任务与快照不匹配时撤销领取、保留预算及待发送状态。Hook 无请求构造或签名材料入口，不使用 PHP 类继承、反射注入和可修改的底层 HTTP 客户端。

## 验收

使用隔离基线叠加 Webhook 历史至 `e7598f4ab` 和本批文件，固定 Rust 1.94.0。SQLite、PostgreSQL 18.7、MySQL 8.4.13 实际运行 HTTP／Artisan／独立 Worker，验证外部 Job 的同步和队列接入、拦截 503 重试、允许重试后成功、原始请求与响应字节、两种终态回调、监听器与钩子错误脱敏、端点撤销、缺少注册后恢复及任务注册名不匹配。失配和缺失路径检查实际没有传输、没有消费尝试。SQLite 定时恢复、136 个真实接收请求及 83 次独立 Worker 回归通过。包发现、缓存、发布和两种签名协议回归通过。八个包测试、三语六组编译示例、全仓格式检查和双特性 workspace/all-targets 严格 Clippy 均通过。测试日志在 `target/tmp/webhooks-custom-job/`。

WH-04／WH-05 仍开放。接续出站重放、端点与订阅、TLS 扩展及全拓扑验收。
