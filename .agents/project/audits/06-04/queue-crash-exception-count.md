# 06-04 Worker 崩溃计入异常次数

## 基线

对照固定 Laravel [Max Exceptions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#max-exceptions)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php) 的崩溃标记、恢复前失败及完成后清理。沿用固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型运行边界。v0.x（`6d1a8192`）未找到对应实现。本批复用持久预约、缓存锁、异常次数上限及终态结算。

## 实现与 Rust 适配

- `JobOptions::count_crashes_as_exceptions` 默认关闭。配置异常上限后，Worker 在应用默认缓存保存一日有效的任务 UUID 标记。重新预约发现旧标记时累计一次异常，达到上限在调用处理器前失败。
- OpenRust 以带令牌的缓存标记区分尝试。获取和删除标记时使用短期缓存锁，旧执行器结束时只清理自己的标记。已返回的尝试在重新发布预约前清理，正常释放和已处理的超时不会计为崩溃。跨进程依赖同一支持锁的缓存及命名空间。
- 本批的强制终止实测在 Unix 上运行。其他平台在共享缓存和驱动能重新预约时沿用相同的标记处理，实际终止尚未验证。

## 验收

- 真实 HTTP 派发、独立 Artisan 和实际进程终止，覆盖 SQLite、MySQL 8.4.13 和 Redis 8.10.3。验证两次崩溃后的第三次预约终态失败、未启用时正常重试、普通释放及超时后正常重试、旧 Worker 迟到无法清除新标记。
- Rust 1.94.0 格式、锁定离线严格 Clippy、工作区串行测试、三语 Rust 示例提取运行、文档构建与 18 项 Chrome 检查通过。全量运行确认生产实现与三个持久后端场景通过。夹具随后明确断言旧 Worker 退出码 1 和 Unix SIGKILL 状态，定向复跑三后端。

[English](../../../../docs/en/digging-deeper/queues.md#counting-worker-crashes) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#counting-worker-crashes) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#counting-worker-crashes)
