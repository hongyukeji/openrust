# 自定义发送 Job 的已保存设置

固定 Spatie 发送 Job 在业务钩子可读取该次投递的请求、签名和重试属性。原生 `WebhookJobAttempt::options` 向两个应用钩子提供已保存的 HTTP 方法、代理名称、尝试上限、重试状态码、Retry-After、抖动、超时、签名及退避名称、头名称、TLS 验证和队列连接。 `header_names` 提供准备阶段的头名称。 [固定 Spatie CallWebhookJob](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Jobs/CallWebhookJob.php)

新入口借用现有快照，复用同一次尝试中的正文、响应、元数据与标签。发送所需秘钥、代理认证、CA 及客户端身份仍由当前应用策略解析和应用。持久记录、队列载荷以及事件结构沿用原格式。

## 验收

SQLite、PostgreSQL 18.7、MySQL 8.4.13 实测 HTTP 提交、Artisan、同步投递和独立 Worker。两个自定义钩子读取同一快照，覆盖默认重试状态、显式 503、不同超时、启用抖动、同步抛错、队列路由、连接失联、撤销及终态。首轮夹具将规范化后的 timestamp 头名误写为首字母大写，按保存的 HeaderName 形状修正后复测通过。发送包及夹具严格 Clippy、包契约和三语浏览器检查通过。
