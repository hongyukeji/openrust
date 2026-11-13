# 06-02 事件、监听器与事务后分派

- 对照固定 Laravel 的监听器注册、订阅、精确和通配符排序、停止传播、`until` 与事务回调。参考固定 Goravel 的事件服务提供者与应用装配。提取 `v0.x` 的事件匹配、监听器快照和响应处理，移植真实派发与停止传播测试的关键路径。 [Laravel 事件文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md) · [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php) · [Laravel 事务回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseTransactionsManager.php) · [Goravel 事件服务提供者](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go)
- 框架 Provider 注册应用级 Dispatcher，`withEvents` 在应用启动时消费编译后的登记清单。Subscriber 成组登记。执行前只克隆选中的监听器引用，执行中释放注册表锁。Artisan 的 `event:list` 支持按名称过滤与 JSON 输出。
- 事务显式通过 `EventContext` 或 Event Facade 传入。事务持有已拥有的提交回调，仅有回调的保存点分配记录。嵌套提交保留回调并按 Laravel 的已提交子事务顺序执行，保存点和外层回滚丢弃相应回调。最外层提交归还连接后执行监听器，错误以 `DatabaseError::AfterCommit` 报告，避免重试已提交的写入。
- Rust 的事件使用类型与编译登记清单。PHP 的运行时目录扫描、事件缓存命令、动态监听器类解析与 Eloquent Observer 装配仍待接入。队列监听器保留公开骨架，运行接线依赖 06-03 和 06-04。
- 以 `6396f9bb` 为隔离基线。Rust 1.94.0 格式、466 项锁定离线工作区测试、全目标严格 Clippy 和 `./artisan inspire` 通过。SQLite 和真实 MySQL 8.4 的 HTTP 验收覆盖精确和通配符匹配、顺序、停止传播、`until`、Subscriber、应用隔离、保存点提交、保存点和整笔回滚、丢弃事务，以及提交后回调读取已提交数据。回调错误用例验证数据保留且事务未重试。
- 三语示例从正文抽取，在实际 HTTP 中返回监听器结果。示例应用的 Artisan 过滤和 JSON 输出通过。VitePress 构建、12 项通用浏览器组合与 6 项事件页浏览器检查通过。

## 动态发现

- 固定 Laravel 在发现阶段通过 `ShouldBeDiscovered::shouldBeDiscovered` 过滤监听器。Rust 在 `withEvents` 的编译清单中通过 `withListener` 明确关联条件与注册函数。条件借用正在启动的应用，可读取其环境和配置。注册清单先收集结果，条件失败时不发布该清单。手动注册独立于发现条件。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Events/DiscoverEvents.php) · [Laravel 发现测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Foundation/DiscoverEventsTest.php) · [Laravel 动态发现](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md#dynamic-event-discovery)
- 参考 Goravel 的编译型服务提供者装配。旧版 `v0.x` 提供显式监听器注册，未找到同类发现条件。本批复用现有应用级 Dispatcher 和注册表。运行时 PHP 文件反射由编译清单替代，生产事件缓存及 Observer 分开跟踪。 [Goravel 事件服务提供者](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go)
- 并行启动生产与测试应用，用真实 HTTP 和 Artisan `event:list --event=order.ready --json` 核对条件命中、跳过及应用隔离。条件错误时启动失败且清单无部分注册。三语正文 Rust 示例编译并运行 Artisan，VitePress 构建和 12 项浏览器检查通过。
- Rust 1.94.0 格式检查、锁定离线工作区测试（612 项通过，7 项夹具忽略）及全工作区全目标严格 Clippy 通过。实际二进制的 `artisan inspire` 成功退出。六项事件页检查覆盖两种站点前缀及三语导航。

## 生产事件缓存

- 固定 Laravel 的 Provider 启动时读取发现缓存，`event:cache` 重新发现，`event:clear` 删除缓存。OpenRust 将条件结果写入应用根下的 `storage/framework/cache/events.json`。注册函数仍由二进制提供，无条件和手动注册照常运行。固定 Goravel 参考编译型装配，旧版 `v0.x` 只有显式注册，无同类命令可复用。 [Laravel EventCacheCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/EventCacheCommand.php) · [Laravel EventClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/EventClearCommand.php) · [Goravel 事件 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/service_provider.go)
- 验收覆盖根应用二进制的三个事件命令。带编译清单的应用在独立进程执行 Artisan，另用真实 HTTP 验证监听器派发。覆盖缓存命中、清除后重新发现、重复清除、损坏和清单变化时回退、两个应用隔离、并发刷新和失败后保留旧文件。刷新以同目录原子替换发布。部署新构建或更改发现配置后应重新生成。
- Rust 1.94.0 格式、锁定离线工作区测试、事件专项、全目标严格 Clippy 均通过。VitePress 构建、12 项通用与 6 项事件页浏览器检查通过。
