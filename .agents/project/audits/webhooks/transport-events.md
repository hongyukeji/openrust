# 出站事件中的传输统计与失败类别

固定 Spatie `CallWebhookJob` 使用 Guzzle `on_stats` 取得传输统计，并把统计和异常信息交给结果事件。在 OpenRust 中复用核心 HTTP Client 的 `TransferStats`。`WebhookCallEvent::transfer_stats()` 提供已经取得的 HTTP 传输耗时，与 `WebhookAttempt::elapsed` 表达的整次领域尝试耗时分别读取。HTTP 2xx 与 503 等实际响应均可带测量结果。未取得响应时，结果事件和自定义 Job 钩子都能通过 `transport_failure()` 取得脱敏类别。投递期限、HTTP 超时、连接、响应读取和正文超过预算可以分别处理。 [固定 Job](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/CallWebhookJob.php) · [固定基础事件](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Events/WebhookCallEvent.php)

入队前事件和本地策略拒绝没有传输统计或失败类别。分类在结果完成事务及租约校验后随原同步事件分发。观察字段只在进程内提供，持久记录、队列、重试次数与错误文本沿用原契约。完整响应继续通过自定义 Job 的受限 `get_response()` 提供。Guzzle 响应序列化、底层错误及细分网络阶段耗时没有直接对应。进程崩溃后，补记的 Unknown 没有在途响应或本次进程内统计。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 用真实 HTTP、Artisan 迁移与配置／路由缓存、独立 Worker 验证有响应的成功及失败均含耗时，断连、超限响应和超时分别分类，头先到而正文超限时仍按已有规则保存未知结果。自定义 Job 同时读取连接失败类别并停止重试，单次失败与终态事件依序收到同一分类。策略撤销无传输分类，超限响应测试标记和自定义 Job 可见的响应正文都未进入事件输出。SQLite 复测 IPv6 HTTPS CONNECT 及错误目标 CA。发送包原有契约、应用夹具严格 Clippy、格式检查和三语文档构建通过。真实外部对端和跨主机部署仍待验证。
