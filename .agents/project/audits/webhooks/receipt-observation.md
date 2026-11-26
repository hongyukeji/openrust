# 接收结果与耗时的观测事件

固定 Spatie 接收流程已有无效签名事件，沿用原事件载荷及失败行为。按 Webhook 运维计划补充可选的 `WebhookReceiptObserved`，复用应用的原生 Dispatcher、Listener 和 Event Fake。具名配置启用 `observe_receipts`，手动构建也可调用 `WebhookConfig::with_receipt_observation()`。 [固定 Spatie 接收流程](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookProcessor.php)

每次启用观测且进入 Processor 的请求最多产生一次事件。数据仅含固定结果分类和单次执行耗时，不含接收 ID、租户、配置名、URL、正文、请求头或底层错误。耗时以本进程的单调时钟计算，排除该事件监听器自身耗时。监听器返回异常只产生固定摘要，接收结果和已提交状态保持一致。路由、请求流读取及运行时装配阶段提前返回时不产生该事件。

现有两种带派发意图的存储模式通过同一事务区分新接收与重复，包含并发竞争后的重复结果。新增的存储结果入口默认委托原方法，接收成功时按 `accepted` 上报。不改变自定义存储必须实现的方法。接收事件属于进程内观测，无法捕获进程强制终止前未发送的事件，持久状态和恢复继续由数据库承担。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 复验真实 HTTP、Artisan 迁移／缓存／诊断和独立 Worker。两个 HTTP 节点实际竞争同一签名事件，验出一个首次结果及一个重复。并覆盖内容冲突、无有效事件 ID、入列事务回滚、无效签名、Profile 忽略、无签名提示、共享限流和故障监听器。确认有意失败的监听器无法改变 HTTP 响应、重复结果、数据库记录数或任务数，日志中没有原始监听器错误。三语页面、默认关闭、原生 Event Fake、包与应用严格 Clippy 和格式检查通过。

PostgreSQL 和 MySQL 双节点另以 Redis 8.10.3 共享接收配额复验，分类及总次数一致。
