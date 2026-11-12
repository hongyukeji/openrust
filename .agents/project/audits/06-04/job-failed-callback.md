# 06-04 任务失败回调

## 基线

对照固定 Laravel [Cleaning Up After Failed Jobs](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#cleaning-up-after-failed-jobs)、[Job::fail](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/Job.php)、[CallQueuedHandler::failed](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php)、[SyncQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SyncQueue.php) 和 [QueueSyncQueueTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Queue/QueueSyncQueueTest.php)。参考固定 [Goravel 队列任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)。v0.x（`6d1a8192`）的队列未找到可复用的任务级失败回调。

## 实现与 Rust 适配

- `JobHandler::failed` 默认成功返回。普通和批次队列任务的生成器均将它转发给根据原始载荷反序列化的任务对象。
- Worker 只在预约确认后执行回调，沿用失败记录和批次、链处理。在独立应用作用域内恢复日志上下文，传入原始载荷、队列元数据和终态错误。回调错误独立报告，保留原始失败事件。
- 同步队列在返回错误前执行回调，保留调用方事务。回调失败时传播回调错误。进程崩溃可中断清理，任务代码可持久化清理意图。

## 验收

- 独立 Artisan 与真实 HTTP 使用 SQLite、MySQL 和 Redis 验证终态回调、成功重试、日志上下文、硬超时、失败事件和回调错误报告。同步驱动覆盖回调与调用错误，两个应用各自解析已注册处理器。
- 三语示例从文档提取编译并执行。真实 Artisan 生成普通和批次任务，两类输出均编译执行并验证原始载荷反序列化。文档构建、根路径与子路径的 18 项浏览器检查通过。
- Rust 1.94.0 格式、锁定离线严格 Clippy 和串行工作区测试通过，含子进程共 575 项通过、6 项忽略。首次开启可选 MySQL 会话配置时，两项 Web 表单测试因会话表缺失失败。专属 MySQL 会话库经 Artisan 迁移后两项均通过，完整工作区以默认会话配置复跑通过。

[English](../../../../docs/en/digging-deeper/queues.md#cleaning-up-after-failed-jobs) · [简体中文](../../../../docs/zh-Hans/digging-deeper/queues.md#cleaning-up-after-failed-jobs) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/queues.md#cleaning-up-after-failed-jobs)
