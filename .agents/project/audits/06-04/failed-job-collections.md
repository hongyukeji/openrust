# 06-04 集合型失败任务重试

## 基线

对照固定 Laravel [Retrying Failed Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#retrying-failed-jobs)、[RetryCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php)、[RetryBatchCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryBatchCommand.php) 和 [QueueRetryCommandTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueRetryCommandTest.php)。参考固定 [Goravel 队列任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)。v0.x 的 `retry_failed_inner` 处理单一 ID，未提供集合型失败存储。复用现有重试事务、事件和截止时间处理。

## 实现与 Rust 适配

- `FailedJobProviderInterface::findForRetry` 返回可选的异步流，默认适配现有单条查询。区分未知 ID 与空分组。普通 Worker 的 `find` 仍用于终态检查。
- `queue:retry` 按存储键逐条报告、入队和删除，每次只请求当前项。失败时停止读取后续项。事件和截止时间处理收到原始记录。批次重试复用同一入口并核对任务的批次 ID。
- Rust 使用具名流类型表达 PHP 可变返回类型，流条目同时包含存储键和任务记录。相同数据库的条件删除与入队仍在同一个事务。

## 验收

- 独立 Artisan 与自定义失败存储接入真实 SQLite、MySQL 队列，检查按键删除、按需读取、混合选择、未知 ID、空组和入队失败时的停止行为。最终复验还覆盖读流失败、`all` 和按队列选择。
- 三语示例提取编译并执行，文档构建与浏览器检查验收根路径和子路径。Rust 1.94.0 格式、严格 Clippy 及锁定离线串行工作区通过，含子进程 576 项通过、7 项忽略。首轮全量发现读流测试未注入故障，修正夹具后定向与全量复验通过。

[English](../../../../docs/en/digging-deeper/queues.md#retrying-a-group-of-stored-failures) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#retrying-a-group-of-stored-failures) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#retrying-a-group-of-stored-failures)
