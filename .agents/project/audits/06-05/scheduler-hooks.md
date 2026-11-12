# 06-05 任务钩子

## 固定基线

对照固定 Laravel [Task Hooks](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#task-hooks)、[Event](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Event.php)、[CallbackEvent](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/CallbackEvent.php) 和 [ScheduleGroupTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/ScheduleGroupTest.php)。参考固定 [Goravel 调度入口](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/console/run_command.go)。固定 Goravel 和 openrust-v0.x 未找到对应的公开任务钩子。

## 实现与 Rust 适配

- 接通 `before`、`after`、`then`、`onSuccess` 和 `onFailure`。任务执行前运行 before。结束后按注册顺序运行适用的 after、then 与结果钩子。回调异常先运行失败侧钩子，随后报告原执行异常。before 异常跳过任务，after 异常中止余下钩子。锁在任务及其钩子执行结束后释放。
- 调度组、嵌套组和单任务钩子按注册顺序叠加，组外任务恢复外围设置。Rust 钩子显式接收应用和可写的控制台输出，并用 `ConsoleFuture` 返回异步结果。分组通过共享引用复用同一回调。
- 输出型钩子、URL ping、文件输出和后台任务钩子的独立验收仍待完成。

## 验收

- 独立 Artisan 验证成功和非零退出分支、回调错误、前置和后置钩子错误、嵌套继承、生命周期事件顺序以及错误后继续运行后续任务。
- Rust 1.94.0 格式、严格 Clippy 和锁定离线全工作区测试通过。SQLite、MySQL 8.4 和 Redis 8 的主测试进程 591 项通过、7 项忽略，独立队列硬超时夹具通过。调度测试 12 项通过、1 项忽略。三语示例随 Artisan 夹具编译和运行，站点构建与 12 项浏览器检查通过。

[English](../../../../docs/en/digging-deeper/scheduling.md#task-hooks) · [简体中文](../../../../docs/zh-Hans/digging-deeper/scheduling.md#task-hooks) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/scheduling.md#task-hooks)
