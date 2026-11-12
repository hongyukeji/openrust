# 06-04 条件释放与跳过中间件

## 基线

对照固定 Laravel [Releasing Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#releasing-jobs)、[Skipping Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#skipping-jobs)、[Release](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/Release.php)、[Skip](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Middleware/Skip.php) 及 [ReleaseMiddlewareTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/ReleaseMiddlewareTest.php)、[SkipMiddlewareTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/SkipMiddlewareTest.php)。沿用固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型运行边界。v0.x（`6d1a8192`）未找到对应中间件。本批复用当前 Worker 的确认、释放和尝试计数。

## 实现与 Rust 适配

- `Skip::when`、`Skip::unless` 条件命中时短路并确认任务。`Release::when`、`Release::unless` 条件命中时短路并按指定延迟释放预约。
- Rust 的 `MiddlewareCondition` 接受布尔值或一次求值的闭包。按 Laravel 的构造时机求值。Rust 用显式 `Duration` 表达延迟，零延迟用 `Duration::ZERO`。

## 验收

- 实际 HTTP 派发及独立 `queue:work` 在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上验收确认、条件未命中时执行、闭包每次构造只求值一次、零延迟立即预约、延迟期间不可预约和到期后第二次尝试成功。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和工作区串行测试通过。三语示例提取后使用实际 sync 队列执行，文档构建和 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#releasing-jobs) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#releasing-jobs) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#releasing-jobs)
