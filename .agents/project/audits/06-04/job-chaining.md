# 06-04 · Job Chaining

## 固定基线

- Laravel [Job Chaining](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-chaining)、[PendingChain](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bus/PendingChain.php)、[CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) 和 [JobChainingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php)。
- Goravel 固定版的 [queue.Job](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 参考编译型任务契约。
- `v0.x:vendor/openrust/framework/src/queue.rs` 的 `dispatch_chain` 和 TaskGroup 持久化实现用于核对失败与恢复。旧版把整条链作为父任务执行，本次沿用当前分步 Worker、载荷、预约和失败仓库，以 Laravel 的每任务重试和推进语义实现。参考旧版 `tests/server/queue/worker.rs` 的顺序与终态停止回归，以及 `tests/queue/redis_driver.rs` 对旧 Redis 驱动能力边界的测试。

## 实现与边界

`Bus::chain` 使用 `PendingChain` 配置连接、队列、首任务延迟、顺序及失败回调。链尾和回调名称随载荷持久化，旧队列载荷可继续读取。Rust 回调通过稳定名称在 Worker 的应用中注册，跨进程无需序列化闭包。队列和连接按整条链继承，Laravel 还允许单个任务覆盖目标。数据库用事务、Redis 用 Lua 将确认预约和下一项入队绑定为一次操作。任务本身仍按至少一次交付执行，回调没有独立重试记录。sync 直接运行后续任务。

本批闭合普通任务链。与批次组合、任务执行期间的链修改、单任务目标覆盖以及 `dispatchIf`／`dispatchUnless` 仍待对应交付。

## 验收

- 隔离候选使用 Rust 1.94.0。实际 TCP HTTP 派发，独立 Artisan Worker 连接 SQLite、MySQL 8.4.13 和 Redis 8.10.3。覆盖仅首任务入队、每项成功后才推进、错误重试、主动释放、最终失败与一次 catch 调用、未知回调、首任务延迟及 sync 成败。
- SQLite 和 MySQL 注入后继写入失败，核对原预约保留，过期后重试只推进一次。Redis 在预约后注入通知键类型错误，核对原预约保留且后继未入队。恢复键后完成一次后继写入，重复确认被拒绝。
- `cargo +1.94.0 fmt --all --check`，`cargo +1.94.0 clippy --locked --offline --workspace --all-targets -- -D warnings`，`cargo +1.94.0 test --locked --offline --workspace -- --test-threads=1`。工作区通过计数见进度记录。
- 三语队列示例分别接入 HTTP 和 Artisan 夹具执行。VitePress 构建、12 项通用和 6 项任务链浏览器检查通过。

正文入口：[English](../../../../docs/en/digging-deeper/queues.md#job-chaining) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#job-chaining) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#job-chaining)。
