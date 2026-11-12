# 06-04 Worker 重启、暂停与恢复

## 固定基线

核对固定 Laravel 的 [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[QueueManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/QueueManager.php)、[RestartCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/RestartCommand.php)、[PauseCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/PauseCommand.php)、[ResumeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/ResumeCommand.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 Goravel [队列装配](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/queue)。旧版 `v0.x`（`6d1a8192`）未提供对应 Artisan 命令，沿用其 Worker 测试通过真实入口观察运行状态的方法。

## 实现与差异

- 通过应用的默认缓存协调 Worker。重启用唯一标记区分同一秒内的不同操作。进程完成当前任务后退出，新启动的 Worker 以已有标记为基线。
- 暂停以连接和队列为范围。全局恢复保留单队列暂停状态。Worker 跳过暂停队列，继续按原顺序轮询其他队列。单队列 Redis Worker 保留 `block_for` 阻塞行为。
- `Queue::withoutInterruptionPolling()` 在应用的 QueueManager 保存开关，Rust 同进程内的应用各自配置。Laravel 暂停和恢复事件、限时暂停及 Worker 状态输出留待后续小节核对。

## 验收

- 真实 HTTP 派发和独立 Artisan 进程，在 SQLite、MySQL 8.4.13 和 Redis 8.10.3 上验证全局暂停和恢复、别名、指定队列跳过、运行中暂停和重启、恢复消费及应用关闭轮询。SQLite 与 MySQL 使用文件缓存，Redis 使用隔离前缀。
- Rust 1.94.0 格式检查及锁定离线全目标严格 Clippy 通过。工作区串行锁定离线测试报告 579 项通过、6 项标记忽略。定向用例通过环境变量强制运行三个队列驱动。
- 三语正文提取的 15 条 Artisan 命令通过实际编译产物执行。VitePress 构建通过，Chrome 在 `/` 和 `/docs/` 共通过 12 项通用及 6 项 Worker 页面检查。

[English](../../../../docs/en/digging-deeper/queues.md#pausing-and-resuming-queue-workers) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#pausing-and-resuming-queue-workers) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#pausing-and-resuming-queue-workers)
