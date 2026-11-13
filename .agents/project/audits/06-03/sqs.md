# 06-03 Amazon SQS

对照固定 Laravel 的 SqsQueue、SqsJob 与 SqsConnector。旧版 `v0.x` 未找到 SQS 队列实现。本批复用当前的 JSON 载荷、Worker、失败记录、AWS 凭据链和 SDK 装配方式。SQS SDK 锁定为支持项目 Rust 1.94.0 的 1.101.0。 [Laravel SqsQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SqsQueue.php) · [Laravel SqsJob](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/SqsJob.php)

- 支持完整队列 URL，或前缀、名称与后缀组合。FIFO 后缀在 `.fifo` 前拼接。标准队列支持逐消息延迟，FIFO 支持分组、显式去重标识和内容去重配置。消息确认和释放使用收到的 receipt handle，尝试次数来自服务端。
- macOS ARM64、Rust 1.94.0。独立 ElasticMQ 1.6.15 实例通过标准队列延迟、立即释放、可见期到期重领、FIFO 同组顺序与重复键抑制、清空队列。实际 `queue:work sqs` 处理失败和后续成功任务，失败回调与 MySQL 8.4.13 失败记录通过。
- AWS 托管服务的 IAM、短期凭据与近似指标仍待实机验收。SQS 大消息溢出存储、批次发布和链路的跨操作恢复继续跟踪。FIFO 去重窗口由服务端控制。
- Rust 格式检查、专项严格 Clippy、三语站点构建和 12 项浏览器检查通过。
