# 06-05 调度组与属性继承

## 固定基线

对照固定 Laravel [Schedule Groups](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md#schedule-groups)、[Schedule](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/Schedule.php)、[PendingEventAttributes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Scheduling/PendingEventAttributes.php) 和 [ScheduleGroupTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/Scheduling/ScheduleGroupTest.php)。参考固定 [Goravel 调度入口](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/schedule/console/run_command.go)。固定 Goravel 与 openrust-v0.x 未找到对应的调度组实现或测试。

## 实现与 Rust 适配

- 使用 `Schedule::configure` 的可变应用作用域声明调度组。组属性应用于 Artisan 命令、回调、外部命令及队列任务。嵌套组继承父级，临时属性在下一次注册时消费，组结束或返回错误时恢复外围设置。
- 支持组内共用频率、时区、环境、过滤条件、名称、任务元数据、重叠锁、单机选举及维护和暂停例外。任务注册后可覆盖单值设置，过滤条件按现有 Event 契约累积。待应用属性通过有类型的闭包保存，分组解析发生在注册期。
- Rust 保留现有的频率覆盖行为。改用分钟级表达式会清除子分钟重复。Laravel 将表达式和重复秒数分别存储。元数据使用 JSON 对象和数组表达递归合并结果。生命周期钩子、输出处理、后台进程和宏的分组支持待对应接口闭合。

## 验收

- 独立 Artisan 进程验证嵌套时区、任务频率覆盖、条件过滤、临时属性消费、错误后作用域恢复、维护与暂停例外、子分钟频率及元数据继承。三语示例随夹具编译和执行。
- Rust 1.94.0 格式通过。调度集成测试 10 项通过、1 项忽略。
- 三语站点构建和 12 项浏览器检查通过，其中 6 项覆盖调度组小节和示例。
- 锁定离线全工作区测试使用 SQLite、MySQL 8.4 和 Redis 8，主测试进程 589 项通过、7 项忽略。独立队列硬超时夹具也通过。全工作区严格 Clippy 通过。

[English](../../../../docs/en/digging-deeper/scheduling.md#schedule-groups) · [简体中文](../../../../docs/zh-Hans/digging-deeper/scheduling.md#schedule-groups) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/scheduling.md#schedule-groups)
