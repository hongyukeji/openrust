# 06-03 Beanstalkd

按固定 Laravel BeanstalkdQueue、BeanstalkdJob 与连接器核对 tube、默认优先级、延时、TTR、阻塞预约、尝试计数和失败释放。旧版 `6d1a8192` 未找到 Beanstalkd 驱动，沿用既有 Rust JSON 载荷、ReservedJob 和 Worker。 [Laravel BeanstalkdQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BeanstalkdQueue.php) · [Laravel BeanstalkdJob](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/BeanstalkdJob.php)

- TCP 会话随预约持有，释放使用原会话。删除前核验预约所有权，旧执行者不能确认被后继会话取得的预约。状态统计提供 ready、delayed 与 reserved 计数。
- macOS ARM64、Rust 1.94.0，使用源码编译的 Beanstalkd v1.13 独立回环实例。真实 `queue:work beanstalkd` 在 SQLite 和 MySQL 8.4.13 失败存储下通过，覆盖延迟、阻塞唤醒、并发预约排他、释放、TTR 到期重投递、过期确认拒绝、失败记录与回调、异常后重试。
- 任务链沿用至少一次执行，Beanstalkd 的后继发布与当前任务确认分两步。固定 Laravel Beanstalkd 同样未实现清空接口。SQS 单独验收。
- Rust 格式与专项严格 Clippy、三语站点构建和 12 项浏览器检查通过。

## 跨连接任务链

固定 Laravel 在确认当前任务前派发下一项，并沿用后继任务的队列与延迟。Rust 继续使用可重新投递的转发任务。目标发布失败时保留转发任务，保留同步处理器自身失败的原有结算。旧版 `v0.x` 没有对应的 Beanstalkd／SQS 转发实现。 [Laravel CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) · [Laravel Queueable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Queueable.php)

- 真实 HTTP 派发 `Bus::chain`。数据库 Worker 向 Beanstalkd 1.13 转发，核对目标 tube、延迟期间不可预约、到期后独立 Artisan 消费和队列清空。SQLite、MySQL 8.4.13 均通过。
- ElasticMQ 1.6.15 与 MySQL 8.4.13 验证 SQS 正常转发及目标队列不存在时的失败。第一次转发失败后，数据库保留转发任务。创建队列后重新运行 Worker，转发成功，SQS Worker 完成后继任务，失败记录未增加。
- SQLite、MySQL 和 Redis 的既有任务链／批次回归、专项严格 Clippy、格式、三语站点和 12 项浏览器检查通过。发布成功但来源确认中断的恢复，以及托管 AWS 验收仍待收尾。

Beanstalkd 批次、追加和链中批次已用独立 Worker 验收。 [外部队列批次验收](external-batches.md)
