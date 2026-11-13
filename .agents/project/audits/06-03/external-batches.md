# 06-03／06-04 Beanstalkd 与 SQS 批次

固定 Laravel 在 `Batch::add` 中更新批次计数，并由连接的 `bulk` 逐项发布。OpenRust 延续连接选择、批次计数、追加和续接，使用现有 SQL 账本先提交待发布记录。旧版 `v0.x` 无 Beanstalkd 或 SQS 批次实现。Goravel 固定版用于参照应用级连接解析与 Worker 装配。 [Laravel Batch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php) · [Laravel Queue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Queue.php) · [Goravel Queue Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/queue/application.go)

- macOS ARM64、Rust 1.94.0。使用 `cargo +1.94.0 test --locked --offline --test job_batches` 跑真实 HTTP 和独立 Artisan。Beanstalkd 1.13 搭配 SQLite、MySQL 8.4.13，覆盖初次发布、追加、批次完成回调和链中批次的成功、取消及失败分支。
- ElasticMQ 1.6.15 搭配 MySQL 8.4.13。代理只拒绝第二个初始任务的首次 `SendMessage`，其余请求进入真实队列。以 `OPENRUST_TEST_SQS_BATCH_REJECT_SECOND=1` 启用断言。核对队列已收到首项、账本只保留余下两项、追加等待初次发布、并发 `queue:publish-batches` 恢复及 `queue:work sqs` 各消费一次。原有 SQLite、MySQL、Redis 和跨 MySQL 库回归同时通过。
- 格式、专项严格 Clippy、三语站点构建及 12 项浏览器检查通过。进程在 broker 接收后、SQL 确认前退出时仍可能重发。该窗口、FIFO 批次实测与托管 AWS 验收继续跟踪。
