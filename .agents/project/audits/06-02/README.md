# 06-02 事件、监听器与事务后分派

- 对照固定 Laravel 的监听器注册、订阅、精确和通配符排序、停止传播、`until` 与事务回调。参考固定 Goravel 的事件服务提供者与应用装配。提取 `v0.x` 的事件匹配、监听器快照和响应处理，移植真实派发与停止传播测试的关键路径。 [Laravel 事件文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md) · [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php) · [Laravel 事务回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseTransactionsManager.php) · [Goravel 事件服务提供者](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go)
- 框架 Provider 注册应用级 Dispatcher，`withEvents` 在应用启动时消费编译后的登记清单。Subscriber 成组登记。执行前只克隆选中的监听器引用，执行中释放注册表锁。Artisan 的 `event:list` 支持按名称过滤与 JSON 输出。
- 事务显式通过 `EventContext` 或 Event Facade 传入。事务持有已拥有的提交回调，仅有回调的保存点分配记录。嵌套提交保留回调并按 Laravel 的已提交子事务顺序执行，保存点和外层回滚丢弃相应回调。最外层提交归还连接后执行监听器，错误以 `DatabaseError::AfterCommit` 报告，避免重试已提交的写入。
- Rust 的事件使用类型与编译登记清单。PHP 的运行时目录扫描、事件缓存命令、动态监听器类解析与 Eloquent Observer 装配仍待接入。队列监听器保留公开骨架，运行接线依赖 06-03 和 06-04。
- 以 `6396f9bb` 为隔离基线。Rust 1.94.0 格式、466 项锁定离线工作区测试、全目标严格 Clippy 和 `./artisan inspire` 通过。SQLite 和真实 MySQL 8.4 的 HTTP 验收覆盖精确和通配符匹配、顺序、停止传播、`until`、Subscriber、应用隔离、保存点提交、保存点和整笔回滚、丢弃事务，以及提交后回调读取已提交数据。回调错误用例验证数据保留且事务未重试。
- 三语示例从正文抽取，在实际 HTTP 中返回监听器结果。示例应用的 Artisan 过滤和 JSON 输出通过。VitePress 构建、12 项通用浏览器组合与 6 项事件页浏览器检查通过。
