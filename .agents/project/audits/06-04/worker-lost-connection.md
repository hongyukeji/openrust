# 06-04 Worker 断连处理

## 基线

核对固定 Laravel 的 [Running the Queue Worker](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#running-the-queue-worker)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php)、[LostConnectionDetector](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/LostConnectionDetector.php) 和 [QueueWorkerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueWorkerTest.php)。参考固定 [Goravel Worker 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/queue.go) 的运行边界。复核 v0.x（`6d1a8192`），未找到对应 Worker 停止机制。

## 实现与 Rust 适配

- 守护进程在预约或处理任务时识别到断连，将异常交给应用报告器，发出 `WorkerStopping`，停止原因为 `lost_connection`、退出码为 `0`。失败、重试与预约租约仍由现有驱动及策略处理。
- 通过 SQLx 与 Redis 类型化错误链识别断连，SQLx 识别连接 I/O 错误及指定 SQLSTATE、MySQL 服务关闭和会话中断编号。Redis 复用驱动自身的断连判断。应用可绑定 `contracts::database::LostConnectionDetector` 扩展判断。通用业务异常继续走原失败策略。
- 应用级 `queue.stop_on_lost_connection` 替代进程静态开关，默认开启。关闭后，预约断连报告后等待一秒重试，处理阶段继续遵守原任务策略。单次 Worker 保留原错误和退出行为。

## 验收

- MySQL 8.4.13 用真实 HTTP 派发任务，并从独立 Artisan 启动 Worker。在任务事务的查询期间，仅中断本次验收会话。默认模式报告异常、保留重试任务并退出 0，关闭自动退出后，Worker 完成重试和后续任务。
- Redis 8.10.3 通过 TCP 代理仅切断独立 Worker 的一次阻塞预约连接。验证默认退出 0、报告异常、关闭自动退出后重新预约并完成真实 HTTP 派发的任务。SQLite 验证自定义检测器仅在绑定它的应用中生效。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，日志合计 575 项通过、6 项忽略，含独立进程夹具。三语 Artisan 命令实跑，文档构建和 18 项 Chrome 检查通过。

[English](../../../../docs/en/digging-deeper/queues.md#worker-lost-connection) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#worker-lost-connection) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#worker-lost-connection)
