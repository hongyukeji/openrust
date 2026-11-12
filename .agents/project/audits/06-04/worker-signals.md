# 06-04 Worker 系统信号

## 基线

对照固定 Laravel 的 [Reacting to Worker Signals](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#reacting-to-worker-signals)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 的信号监听与暂停检查、[事件类](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。复核旧版 v0.x（`6d1a8192`），未发现对应事件和 Unix 暂停信号。

## 实现与 Rust 适配

- Unix 的 SIGINT、SIGQUIT 和 SIGTERM 派发 `WorkerInterrupted`，保留真实信号编号、连接、队列及选项，完成当前任务后优雅退出。SIGUSR2 和 SIGCONT 对应 `WorkerPausing` 与 `WorkerResuming`，作用于当前守护进程。共享队列暂停继续走缓存。
- 用 Tokio 信号流和 watch 状态管理暂停及退出，异步任务在暂停和退出时继续执行。暂停生效前已经开始的预约允许返回，沿用现有驱动的预约一致性策略。Windows 沿用 Ctrl+C 入口，Windows 实机验收待补。
- `--once` 保留原有中断行为和事件边界。应用主动取消 Worker 时使用现有 CancellationToken。任务级 `Interruptible` 和 `JobInterrupted` 为后续批次。

## 验收

- SQLite、MySQL 8.4.13 和 Redis 8.10.3 通过真实 HTTP 派发与独立 Artisan 进程验证。向执行中进程发送 SIGUSR2 后，当前任务完成且下一任务保留。SIGCONT 后继续处理。SIGTERM 验证进行中任务完成，SIGQUIT 与空闲 SIGINT 验证退出状态和实际信号编号。
- Rust 1.94.0 下通过 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 和 `cargo test --workspace --locked --offline -- --test-threads=1`。日志合计 575 项通过、6 项忽略，含独立进程夹具。
- 三语文档中的 `kill` 命令以编译后 Worker 的实际 PID 逐条执行。文档构建与 18 项 Chrome 检查通过，覆盖根路径和 `/docs/`。

[English](../../../../docs/en/digging-deeper/queues.md#reacting-to-worker-signals) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#reacting-to-worker-signals) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#reacting-to-worker-signals)
