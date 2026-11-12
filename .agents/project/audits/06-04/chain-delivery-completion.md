# 06-04 · 同步后继与投递凭据

## 固定基线与旧版

- 对照 Laravel [Chain Connection and Queue](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#chain-connection-queue)、[CallQueuedHandler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php) 与 [SyncQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SyncQueue.php)，核对同步执行、忽略延迟及失败停止。
- 参考固定 Goravel [Queue::Chain](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的编译型入口。旧版 `v0.x:vendor/openrust/framework/src/queue.rs` 的持久任务组在单一队列上执行，未提供跨连接的同步后继及投递凭据。参考旧版 Worker 对链顺序和失败停止的断言。

## 实现与差异

持久链的 `sync` 后继在源 Worker 处理转发任务时执行，继承持久载荷的 Context。同步处理器失败会执行链的 `catch` 并停止后续派发。源预约在同步执行之后确认。进程在确认前崩溃时，该同步处理器可能重放。

只有源预约删除成功后才启动凭据的保留期。Redis 凭据保留 24 小时。SQL 记录确认时间，通过 OpenRust 的 `queue:prune-chain-deliveries` 清理超过保留期的已确认记录。升级前或确认写入失败的记录没有确认时间，清理命令会保留这些记录。Redis 设置过期时间失败时也保留原凭据。该跨驱动确认机制属于 OpenRust 的持久投递实现。

## 验收

- 真实 HTTP 和独立 Artisan Worker，在 SQLite、MySQL 8.4.13 与 Redis 8.10.3 上验证持久队列转 sync、忽略同步延迟、Context、成功后续链和失败回调。
- 模拟 SQL 源预约确认失败，Redis 已投递凭据保持无到期时间。解除故障后再次处理，目标任务只入队一次，源确认后才出现 24 小时 TTL。SQL 测试确认实际凭据写入时间，并用三语正文中的 Artisan 命令验证已确认过期凭据清理、近期及待确认凭据保留。验证迁移回滚及旧凭据保留。
- Rust 1.94.0 格式、严格 Clippy、锁定离线工作区测试。VitePress 构建和 18 项浏览器检查。

[English](../../../../docs/en/digging-deeper/queues.md#chain-connection-queue) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#chain-connection-queue) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#chain-connection-queue)
