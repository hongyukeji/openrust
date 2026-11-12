# 06-04 Artisan 清理过期失败任务

## 基线

对照固定 Laravel [Pruning Failed Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#pruning-failed-jobs)、[PruneFailedJobsCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/PruneFailedJobsCommand.php)、[PrunableFailedJobProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Failed/PrunableFailedJobProvider.php)、[DatabaseUuidFailedJobProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Failed/DatabaseUuidFailedJobProvider.php) 和 [存储测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/DatabaseUuidFailedJobProviderTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。v0.x（`6d1a8192`）未找到同名命令。本批复用现有 Artisan 和失败任务存储。

## 实现与 Rust 适配

- `queue:prune-failed` 通过可选的 `PrunableFailedJobProvider` 清理失败记录，默认保留 24 小时，`--hours` 可覆盖。按 UTC 截止时间严格小于匹配，输出实际删除条数。未提供能力的存储驱动返回失败。
- 每批最多 1000 条。MySQL 使用带限制的 DELETE，SQLite 使用同一语句内按主键选择的子查询。原有 `queue:flush` 语义由原接口处理。

## 验收

- 独立 Artisan 在 SQLite 与 MySQL 8.4.13 上验证默认和自定义期限、1,001 条跨批、重复清理、无存储能力时拒绝、两个进程并发计数和新记录保留。直接驱动调用验证严格截止时间边界。
- Rust 1.94.0 格式、锁定离线严格 Clippy、串行工作区测试、三语命令实际 SQLite 验证、文档构建与 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#pruning-failed-jobs) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#pruning-failed-jobs) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#pruning-failed-jobs)
