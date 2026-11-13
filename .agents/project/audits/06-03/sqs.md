# 06-03 Amazon SQS

对照固定 Laravel 的 SqsQueue、SqsJob 与 SqsConnector。旧版 `v0.x` 未找到 SQS 队列实现。本批复用当前的 JSON 载荷、Worker、失败记录、AWS 凭据链和 SDK 装配方式。SQS SDK 锁定为支持项目 Rust 1.94.0 的 1.101.0。 [Laravel SqsQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SqsQueue.php) · [Laravel SqsJob](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/SqsJob.php)

- 支持完整队列 URL，或前缀、名称与后缀组合。FIFO 后缀在 `.fifo` 前拼接。标准队列支持逐消息延迟，FIFO 支持分组、显式去重标识和内容去重配置。消息确认和释放使用收到的 receipt handle，尝试次数来自服务端。
- macOS ARM64、Rust 1.94.0。独立 ElasticMQ 1.6.15 实例通过标准队列延迟、立即释放、可见期到期重领、FIFO 同组顺序与重复键抑制、清空队列。实际 `queue:work sqs` 处理失败和后续成功任务，失败回调与 MySQL 8.4.13 失败记录通过。
- AWS 托管服务的 IAM、短期凭据与近似指标仍待实机验收。SQS 批次发布和链路的跨操作恢复继续跟踪。FIFO 去重窗口由服务端控制。
- Rust 格式检查、专项严格 Clippy、三语站点构建和 12 项浏览器检查通过。

## 溢出载荷存储

- 对照固定 Laravel 的发送阈值、缓存指针、释放保留、确认后清理及可选清空。Rust 为每次存入缓存生成独立指针，避免重复发送同一载荷时互相覆盖。FIFO 启用溢出且请求内容去重时，使用原始 JSON 的 SHA-256，保持内容去重行为。可选 TTL 用于回收未消费的缓存项。
- macOS ARM64、Rust 1.94.0，使用独立 ElasticMQ 1.6.15、Redis 8.10.3 实例及 MySQL 8.4.13 独立库。真实 `queue:work sqs` 验证关闭时拒绝超限载荷、阈值模式的小消息直传、始终溢出、大消息重投递、成功确认后默认删除、可选保留及其 TTL，以及清空队列时清空专用缓存。原有 FIFO 顺序、去重、失败记录及后续消费同时通过。
- 发送结果不明和 FIFO 去重可能留下缓存项，由 TTL 或专用缓存清空回收。AWS 托管服务与跨操作恢复仍待验收。
- 根应用二进制使用环境变量配置溢出存储及 TTL，实际 `artisan queue:work sqs --once` 正常退出。格式检查、根应用及 Worker 验收目标的严格 Clippy、三语构建和 12 项浏览器检查通过。

从数据库转发至 SQS 的延迟和目标队列恢复已通过实际 Worker。 [跨连接任务链验收](beanstalkd.md#跨连接任务链)
