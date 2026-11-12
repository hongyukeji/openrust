# 06-04 手工重试截止时间

## 基线

对照固定 Laravel [Time Based Attempts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#time-based-attempts)、[RetryCommand::refreshRetryUntil](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php)、[Queue::getJobExpiration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Queue.php) 与 [QueueRetryCommandTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueRetryCommandTest.php)。固定 [Goravel 队列契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 和 v0.x（`6d1a8192`）没有等价的手工截止时间刷新接口。复用现有绝对截止时间和应用队列注册表。

## 实现与 Rust 适配

- `JobHandler::retryUntil` 接收原始任务载荷，默认保留截止时间。普通和批次任务生成器在初次构造载荷时计算截止时间，手工重试时从原始载荷反序列化任务再计算。显式传入旧值，以支持保留或清除截止时间。
- 普通与批次 Artisan 重试均在同库认领前计算。计算失败返回命令错误并保留失败记录。自动重试、未知任务和静态截止时间保留现有行为。
- PHP 模型反序列化及集合型失败存储仍需按各自契约验收。

## 验收

- 独立 Artisan 与 Worker 在 SQLite、MySQL 和 Redis 验证已过期截止时间可刷新、清除、保持或返回错误。错误路径保留失败记录。普通和批次任务由真实 Artisan 生成，编译执行验证初次和手工截止时间的计算。
- 三语示例提取编译并执行，文档构建与 18 项浏览器检查通过。Rust 1.94.0 格式、锁定离线严格 Clippy 及最终串行工作区验收通过，含子进程 575 项通过、6 项忽略。

[English](../../../../docs/en/digging-deeper/queues.md#refreshing-a-retry-deadline) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#refreshing-a-retry-deadline) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#refreshing-a-retry-deadline)
