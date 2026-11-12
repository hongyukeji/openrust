# 06-05 调度维护门禁

## 固定基线

对照固定 Laravel [Maintenance Mode](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#maintenance-mode)、[Event](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Event.php)、[ScheduleRunCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/ScheduleRunCommand.php) 与 [SubMinuteSchedulingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/SubMinuteSchedulingTest.php)。参考固定 [Goravel 调度命令](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/console/run_command.go) 的运行入口。该版本和 openrust-v0.x 未找到同名维护筛选接口。

## 实现与 Rust 适配

- 初始筛选通过当前应用的 `isDownForMaintenance` 读取共享维护状态。Event 的 `evenInMaintenanceMode` 提供显式放行，`runsInMaintenanceMode` 返回该设置。原有时间、环境、条件、互斥和选举检查继续适用。
- 子分钟运行第一次观察到维护后，将该状态保留到本轮结束。普通事件跳过本轮剩余轮次。下一轮重新读取应用状态。Rust 在异步读取失败时沿用 Artisan 的调度基础设施错误路径。
- 本批对应任务的维护模式小节。调度器独立暂停、调度组和对应属性继承仍需分别验收。

## 验收

- 实际 Artisan 子进程执行 `down`、`schedule:run` 和 `up`，通过文件及 Redis 两种状态后端验证普通任务跳过、显式放行、恢复及应用隔离。子分钟运行中跨进程切换维护，确认解除后普通任务直到下一轮才恢复。
- 三语代码示例使用相同的已编译并运行的命令注册，构建与两种部署前缀下的浏览器导航和维护链接通过。
- 旧的并行进程夹具复跑出现过退出及输出收集超时。长运行子进程改用独立临时文件捕获输出，退出检查与内容读取分开。最终调度夹具 5 项通过、1 项忽略，连续 5 轮并行通过。
- Rust 1.94.0 格式与全工作区严格 Clippy 通过。锁定离线全工作区串行运行使用实际 SQLite、MySQL 8.4 和 Redis 8，共 587 项通过、7 项忽略。使用同一工作区依赖特性复跑最终调度夹具。三语站点构建和 12 项导航检查通过，其中 6 项覆盖维护章节及跨页链接。

[English](../../../../docs/en/digging-deeper/scheduling.md#maintenance-mode) · [简体中文](../../../../docs/zh-Hans/digging-deeper/scheduling.md#maintenance-mode) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/scheduling.md#maintenance-mode)
