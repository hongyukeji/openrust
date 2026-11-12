# 06-04 失败任务重试筛选与事件

## 基线

对照固定 Laravel [Retrying Failed Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#retrying-failed-jobs)、[RetryCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RetryCommand.php)、[QueueRetryCommandTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueRetryCommandTest.php) 和 [DatabaseUuidFailedJobProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Failed/DatabaseUuidFailedJobProvider.php)。参考固定 [Goravel 队列任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)。v0.x 的 `retry_failed_inner` 提供单项事务重试并保留原失败记录，已有测试验证重复操作。当前实现沿用同库事务认领与成功删除失败记录的契约。

## 实现与 Rust 适配

- `queue:retry` 支持多个 ID、`all`、队列筛选和重复数字范围。无匹配和缺失 ID 按命令结果提示，继续处理有效 ID。默认 UUID 存储按队列查询标识，其他存储可覆盖 `ids`。
- `JobRetryRequested` 在普通和批次命令尝试入队前发出，包含原始失败记录。任务回原队列，尝试次数从零开始。同库事务保留并发认领保护。旧 JSON 载荷经语义核对后按原存储内容删除。
- 数字范围适用于数字 ID。Laravel 的动态 `retryUntil` 刷新与集合型失败存储接口仍待核对。跨存储分派沿用现有驱动行为。

## 验收

- 定向实测 SQLite、MySQL、Redis 的队列筛选、范围顺序、重复项、缺失项和入队前事件。最终串行验收含旧载荷回归及同库并发认领。
- 三语示例以真实 Artisan 执行，验证队列、失败记录移除和尝试次数。文档构建与 18 项 Chrome 检查通过。Rust 1.94.0 格式、锁定离线严格 Clippy 及最终串行工作区验收通过，含子进程 575 项通过、6 项忽略。

[English](../../../../docs/en/digging-deeper/queues.md#retrying-failed-jobs) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#retrying-failed-jobs) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#retrying-failed-jobs)
