# 06-04 Artisan 队列监控

## 基线

对照固定 Laravel [Monitoring Your Queues](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#monitoring-your-queues)、[MonitorCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/MonitorCommand.php)、[QueueBusy](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Events/QueueBusy.php)、[DatabaseQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/DatabaseQueue.php) 与 [RedisQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/RedisQueue.php)。固定树中无单独的 MonitorCommand 测试，以命令、驱动和队列测试为基线。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go)。v0.x（`6d1a8192`）未找到同等命令。

## 实现与 Rust 适配

- `queue:monitor` 解析逗号分隔的队列与 `connection:queue`，默认阈值 1000。等于或超过阈值输出 `ALERT` 并派发应用级 `QueueBusy`，其余为 `OK`。
- `QueueMetrics` 用一条数据库查询或 Redis Lua 命令观察总数、待执行、延迟、预约和最早待执行时间。提供 Laravel 对应的四个独立查询方法，监控命令直接使用同一快照。同步驱动返回零，旧 Redis 记录缺少时间时返回空值。
- JSON 输出保留时间戳，纯文本以秒为单位显示等待时间。Redis 创建时间加入队列元数据，旧任务可继续读取。自定义驱动仅实现总数时，其他字段使用空值。

## 验收

- 独立 Artisan 在 SQLite、MySQL 8.4.13、Redis 8.10.3 验证多队列和默认连接、三类状态计数、阈值相等时的事件、低于阈值无事件、文本和 JSON、空队列、同步连接及旧 Redis 载荷。
- Rust 1.94.0 格式、锁定离线严格 Clippy、串行工作区测试、三语命令实际 SQLite 验证、文档构建与 18 项 Chrome 检查通过。验证无效连接、阈值与队列列表返回失败且队列无修改。首轮 MySQL 复验发现 delayed 是保留字，修正内部查询别名后复跑。

[English](../../../../docs/en/digging-deeper/queues.md#monitoring-your-queues) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#monitoring-your-queues) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#monitoring-your-queues)
