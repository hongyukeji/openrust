# 06-04 · 任务链中的批次

## 固定基线与旧版

- Laravel [Chains and Batches](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#chains-and-batches)、[ChainedBatch](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Bus/ChainedBatch.php)、[JobChainingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/JobChainingTest.php)。
- 固定 Goravel [queue.Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 参考编译型链参数。
- 检查 `v0.x:vendor/openrust/framework/src/queue.rs` 的 `TaskGroup`、`dispatch_chain`、`dispatch_batch` 及 `tests/server/queue/worker.rs` 的成功、失败和顺序回归。旧版以父任务分别执行两种分组。当前沿用主线逐任务预约、批次账本与发布重试机制，并复用旧版顺序和终态测试思路。

## 实现与边界

`PendingChain::appendBatch` 和 `prependBatch` 用类型明确的入口表达 PHP 链中的 `PendingBatch`。Worker 以队列任务 UUID 作为批次 ID，重复预约不会重新建立批次或重新投递其成员。批次可显式覆盖连接和队列，其后继回到外层链的原目标。SQLite、MySQL 与 Redis 均使用 SQL 批次仓库。`sync` 外层及与批次仓库不共用连接的 SQL 队列暂不支持这一持久组合。空批次在外层链派发前返回无效载荷错误。

批次进入 `finally` 阶段时，终态和待发布续链记录一起提交。常规 Worker 在运行批次回调后发布续链。恢复命令根据已提交的终态重试发布。并发恢复命令可能在该 Worker 的回调执行期间完成发布，回调执行与后继投递之间没有跨进程的顺序保证。SQL 将投递与删除待发布记录置于同一事务，Redis 沿用去重标记。取消会移除待发布记录。批次失败导致自动取消时，外层 `catch` 收到失败成员和错误，允许失败的批次仍可在所有成员抵达终态后续链。回调自身沿用既有尽力执行的语义。

## 验收

- 实际 TCP HTTP、独立 Artisan、SQLite、MySQL 8.4.13 与 Redis 8.10.3 验证挂起、连续两个批次、同队列驱动与 Redis 外层／SQL 内层、重复预约、允许失败、自动取消、手动取消和失败回调。
- 在数据库和 Redis 注入后继发布失败，两个 `queue:publish-batches` 进程并发恢复，核对只发布和执行一次。验证新迁移、回滚、旧 Redis 待发布记录保留及数据库重建。
- Rust 1.94.0 的格式、锁定离线工作区测试和严格 Clippy，三语示例接入实际 HTTP／Artisan 夹具。站点构建和 18 项浏览器检查。

[English](../../../../docs/en/digging-deeper/queues.md#batches-within-chains) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#batches-within-chains) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#batches-within-chains)
