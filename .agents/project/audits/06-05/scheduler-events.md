# 06-05 前台调度生命周期事件

## 固定基线

对照固定 Laravel [Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#events)、[ScheduleRunCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleRunCommand.php)、[ScheduleRunCommandTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/ScheduleRunCommandTest.php) 和 [CallbackSchedulingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/CallbackSchedulingTest.php)。参考固定 [Goravel 调度入口](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/console/run_command.go)。固定 Goravel 与 openrust-v0.x 未找到可复用的同名事件。

## 实现与 Rust 适配

- 已通过门禁的前台任务先派发 `ScheduledTaskStarting`。正常结束后派发 `ScheduledTaskFinished`。命令返回非零退出码时依次派发开始、完成和失败事件。执行异常或完成监听器异常派发 `ScheduledTaskFailed`，并交给应用异常报告器处理。调度继续处理后续任务。
- 任务事件共享调度注册时的 `Arc<Event>`。运行时间采用单调时钟的 `Duration`，Laravel 使用按秒四舍五入的浮点数。失败事件共享异常引用，保留报告类型和可用的源错误。
- 重叠锁竞争失败时派发 `ScheduledTaskSkipped`。Rust 直接使用原子加锁的结果，Laravel 先检查锁存在性，并在执行阶段尝试加锁。
- 后台完成事件依赖后台进程和完成命令。任务钩子和任务输出按对应文档小节独立验收。

## 验收

- 独立 Artisan 进程检查任务成功、命令非零退出、回调异常、外部程序启动异常、监听器异常、过滤跳过、异常报告顺序和应用隔离。Redis 竞争检查单机选举与重叠锁跳过事件，以及锁释放后再次运行。
- 三语示例与实际注册夹具一致。三语站点构建和 12 项浏览器检查通过，其中 6 项覆盖事件小节及注册示例。
- Rust 1.94.0 格式、严格 Clippy 和锁定离线全工作区测试通过。使用 SQLite、MySQL 8.4 和 Redis 8，主测试进程 590 项通过、7 项忽略，独立队列硬超时夹具通过。最终调度测试 11 项通过、1 项忽略。

[English](../../../../docs/en/digging-deeper/scheduling.md#events) · [简体中文](../../../../docs/zh-Hans/digging-deeper/scheduling.md#events) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/scheduling.md#events)
