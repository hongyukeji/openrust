# 06-03／06-04 Beanstalkd 与 SQS 批次

固定 Laravel 在 `Batch::add` 中更新批次计数，并调用目标连接的 `bulk`。基础实现逐项发布，SQS 驱动使用 `SendMessageBatch`。OpenRust 的 SQS 待发布账本使用原生批量发送，并根据逐项响应保留未确认条目。OpenRust 延续连接选择、批次计数、追加和续接，使用现有 SQL 账本先提交待发布记录。旧版 `v0.x` 无 Beanstalkd 或 SQS 批次实现。Goravel 固定版用于参照应用级连接解析与 Worker 装配。 [Laravel Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) · [Laravel Queue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Queue.php) · [Goravel Queue Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/application.go)

- macOS ARM64、Rust 1.94.0。使用 `cargo +1.94.0 test --locked --offline --test job_batches` 跑真实 HTTP 和独立 Artisan。Beanstalkd 1.13 搭配 SQLite、MySQL 8.4.13，覆盖初次发布、追加、批次完成回调和链中批次的成功、取消及失败分支。
- ElasticMQ 1.6.15 搭配 MySQL 8.4.13。代理只拒绝第二个初始任务的首次 `SendMessage`，其余请求进入真实队列。以 `OPENRUST_TEST_SQS_BATCH_REJECT_SECOND=1` 启用断言。核对队列已收到首项、账本只保留余下两项、追加等待初次发布、并发 `queue:publish-batches` 恢复及 `queue:work sqs` 各消费一次。原有 SQLite、MySQL、Redis 和跨 MySQL 库回归同时通过。
- 格式、专项严格 Clippy、三语站点构建及 12 项浏览器检查通过。进程在 broker 接收后、SQL 确认前退出时仍可能重发。标准 SQS、Beanstalkd 仍使用至少一次投递。托管 AWS 继续跟踪。

## SQS FIFO 确认失败后的恢复

- 固定 Laravel `SqsQueue::getQueueableOptions` 区分显式去重、内容去重和默认 ID。OpenRust 对可重放的批次和任务链根据原始序列化载荷生成稳定的默认 ID，保留显式选项。普通发送仍使用独立的默认 ID。 [Laravel SqsQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SqsQueue.php) · [AWS FIFO 去重窗口](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/using-messagededuplicationid-property.html)
- ElasticMQ 1.6.15 接收第三项后，代理一次性替换成功响应。MySQL 8.4.13 的外键随后拒绝删除待发布记录，独立 `queue:publish-batches` 返回 1。解除外键后并发重跑，账本清空，FIFO 仍只有三项。独立 Worker 按序各消费一次，完成回调执行一次。
- 五分钟窗口结束后，FIFO 也可能重新接收已发布的任务。该实测覆盖错误响应和 SQL 确认失败，托管 AWS 仍待验收。

## SQS 原生批量发送

- 按固定 Laravel 的 `SqsQueue::chunkBatchEntries` 和 AWS API 限制，以最多 10 项、消息体共 1 MiB 分块。复用单条发送的载荷溢出和 FIFO 选项。遇到响应中的失败时停止发送后续分块。SQL 待发布记录保留失败及未发送的条目，即使成功项位于失败项之后。 [Laravel SqsQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SqsQueue.php) · [AWS SendMessageBatch](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/APIReference/API_SendMessageBatch.html)
- ElasticMQ 1.6.15 和 MySQL 8.4.13。代理将首项与第三项交给真实队列，返回 HTTP 200 并将第二项标记失败。并发恢复只重发第二项，追加任务随后发布。另以 16 项验证实际请求分块为 10、5、1，独立 Artisan Worker 消费完毕。FIFO 接收成功但响应报错、SQL 确认失败和重放去重回归同时通过。
- 1 MiB 分块用例使用 ElasticMQ 1.6.15 的 relaxed 模式，代理逐请求断言条数和字节上限。格式、两项测试目标的严格 Clippy、三语站点构建和 12 项浏览器检查通过。
