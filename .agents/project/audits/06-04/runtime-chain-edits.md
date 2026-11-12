# 06-04 · 运行时修改任务链

## 固定基线与旧版

- Laravel [Adding Jobs to the Chain](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#adding-jobs-to-the-chain)、[Queueable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/Queueable.php) 和 [JobChainingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php)。
- 固定 Goravel [queue.Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 参考编译型任务参数。
- 旧版 `v0.x:vendor/openrust/framework/src/queue.rs` 的 `dispatch_chain` 使用固定任务组，未发现运行时前插或追加入口。参考旧版 `tests/server/queue/worker.rs` 的顺序和失败停止断言，沿用主线逐任务预约与原子交接。

## 实现与差异

Rust 在 `JobContext` 上提供 `prependToChain` 和 `appendToChain`。每次接收一个任务载荷或待派发批次，可多次调用。新增任务只随成功的尝试交接，释放和失败保留原链。普通链可以插入持久批次，已有批次的链可以插入同批次的任务。批次链内再插入子批次的组合尚待实现。

批次成员新增任务时，同一账本事务保存新增计数和修改后的后继。若数据库或 Redis 队列交接失败，重领预约从已记录的后继继续。新增迁移为账本添加可空的 `chain_successor`，旧记录按原始链处理。

## 验收

- 实际 HTTP 与独立 Artisan Worker，覆盖 SQLite、MySQL 8.4.13、Redis 8.10.3。验证前插、原后继、尾部追加的顺序，失败／主动释放重试、最终失败、运行时插入批次和 sync 末节点扩链。
- SQLite 和 MySQL 注入记账后的后继入队故障。重领预约后只执行后继，原任务体不重跑。批次计数只增加一次，完成时进度为 100%。验证迁移升级与回滚。
- Rust 1.94.0 格式检查、严格 Clippy、锁定离线工作区测试。三语示例分别接入实际 HTTP 和 Artisan 夹具，站点构建及 18 项浏览器检查。

[English](../../../../docs/en/digging-deeper/queues.md#adding-jobs-to-the-chain) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#adding-jobs-to-the-chain) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#adding-jobs-to-the-chain)
