# 06-04 · 按条件派发任务链

## 固定基线与旧版

- 对照 Laravel [PendingChain::dispatchIf 和 dispatchUnless](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bus/PendingChain.php#L203)、[JobChainingTest 的四种真假组合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php#L627) 和[任务链章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#job-chaining)。
- 参考固定 Goravel [Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型入口。旧版 `v0.x:vendor/openrust/framework/src/queue.rs` 提供直接派发和事务内派发，未发现条件派发入口。旧版 Worker 的链顺序用例用于核对条件满足后的执行行为。

## 实现

`PendingChain::dispatchIf` 和 `dispatchUnless` 保留 Laravel 命名。Rust 的 `ChainCondition` 接收布尔值和 `FnOnce() -> bool`，闭包在等待 Future 时求值一次。Laravel 的 `null` 用 `Ok(None)` 表达，实际派发的结果由 `Some(DispatchResult)` 承载。跳过路径在验证链、解析应用队列或执行任务前返回。符合条件时复用既有派发并保留错误传播。

## 验收

- TCP HTTP 覆盖四种条件组合、布尔和闭包、闭包只执行一次、非法连接和空链的跳过及真实错误。独立 Artisan Worker 在 SQLite、MySQL 8.4.13、Redis 8.10.3 只执行符合条件的首尾任务，sync 在请求中执行。检查实际队列大小和任务副作用。
- Rust 1.94.0 格式检查、严格 Clippy、锁定离线工作区测试。三语示例接入 HTTP 和 Artisan 夹具执行，站点构建及 18 项浏览器检查。

[English](../../../../docs/en/digging-deeper/queues.md#conditional-chain-dispatch) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#conditional-chain-dispatch) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#conditional-chain-dispatch)
