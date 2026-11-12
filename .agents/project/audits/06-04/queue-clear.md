# 06-04 Artisan 清理队列

## 基线

对照固定 Laravel [Clearing Jobs From Queues](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#clearing-jobs-from-queues)、[ClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/ClearCommand.php)、[ClearableQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Queue/ClearableQueue.php)、[Redis Lua](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/LuaScripts.php) 和 [QueueClearCommandTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueClearCommandTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。v0.x（`6d1a8192`）未找到对应命令。本批复用现有 Artisan 生产确认、数据库连接与 Redis 队列键。

## 实现与 Rust 适配

- `queue:clear [connection] [--queue=…] [--force]` 使用显式连接和队列或配置默认值。队列列表保留顺序，去空白、跳过空项并去重。先验证队列名的通用格式，再按队列执行删除。命令汇总并输出删除数量。
- `ClearableQueue` 是连接可选能力。数据库使用绑定参数按队列删除，Redis 用一条 Lua 命令统计并移除待执行、延迟和已预约的三个键，同时移除唤醒通知键。同步驱动报告不支持。
- 生产确认复用 Artisan 公共实现。`ClearCommand::prohibit(true)` 写入当前应用的 QueueManager，优先于强制选项。清理按队列执行，保留其他队列、失败和批次记录。

## 验收

- SQLite、MySQL 8.4.13、Redis 8.10.3 使用独立 Artisan 验证默认连接与队列、多队列去重、生产拒绝及强制、应用级禁止、同步驱动拒绝、三种任务状态和其他队列保留。清理后已撤销的预约无法确认。Redis 验证目标队列通知键删除、其他队列通知保留。通过伪终端实测生产确认，输入 n 保留任务，输入 y 删除任务。
- Rust 1.94.0 格式、锁定离线严格 Clippy、串行工作区测试、三语命令在隔离 SQLite 上实跑、文档构建和 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#clearing-jobs-from-queues) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#clearing-jobs-from-queues) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#clearing-jobs-from-queues)
