# 06-04 · 批次中的任务链

## 固定基线与复用

- 对照 Laravel [Chains and Batches](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#chains-and-batches)、[Batch::add 和 prepareBatchedChain](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Batch.php)、[CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) 及 [JobChainingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php)。
- 参考 Goravel 固定版 [queue.Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型链参数和 [queue.Job](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go) 的任务契约。
- 检查 `v0.x:vendor/openrust/framework/src/queue.rs` 的 `dispatch_chain`、`dispatch_batch`、`TaskGroup`，以及 `tests/server/queue/worker.rs` 的顺序、并发和终态断言。旧版组执行通过单一父任务编排。当前复用顺序、并发和终态验证思路，并沿用主线的批次账本、逐任务预约、原子任务链交接和 Redis 待发布记录。

## 实现

Rust 用 `PendingBatch::addChain` 和 `Batch::addChain` 表达 PHP 嵌套数组，保留现有 `Bus::batch([])` 和单任务添加调用。每个成员沿用同一个批次 ID、连接和队列，计入总数。各链只入队首任务，Worker 只在当前任务成功时原子交接。账本已成功且交接失败时，重领预约直接补做交接，避免重复执行已记账的任务体。

批次允许失败时，失败的链停止，未执行成员继续保持待处理。手工重试成功后该链继续。下一批处理反向组合，即外层任务链中的批次和批次终态的续链。

## 验收

- 实际 HTTP 派发与追加，独立 Artisan Worker 在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上验证按成员计数、各链按序推进、两个 Worker 并行完成和完成回调。覆盖空链、重复成员、失败停止和 `queue:retry` 恢复。
- SQLite 和 MySQL 在成功记账后注入后继写入失败，恢复后原任务体只运行一次，后继只执行一次，账本进度达到 100%。
- Rust 1.94.0 运行 `cargo fmt --all --check`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`、`cargo test --locked --offline --workspace -- --test-threads=1`。
- 三语示例分别接入 HTTP 和 Artisan 测试夹具。VitePress 构建、12 项通用和 6 项批次任务链浏览器检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#chains-and-batches) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#chains-and-batches) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#chains-and-batches)
