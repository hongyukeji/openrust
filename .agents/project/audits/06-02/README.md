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

## Eloquent Observer

- 对照固定 Laravel 的 `Model::observe` 和生命周期方法注册。Rust 使用 `Observer<M>` 与编译期 `EVENTS` 清单，通过应用服务提供者注册。沿用 05-01 类型化监听器及 v0.x 的生命周期派发基础。固定 Goravel 的编译型 Provider 作为装配参考。 [Laravel HasEvents](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasEvents.php) · [Laravel Observer 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#observers) · [Goravel ORM Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)
- SQLite 和 MySQL 8.4.13 验证 Provider 注册、保存前修改及取消、插入后在同一连接写入记录、同一清单内重复事件去重，以及未注册应用的隔离。现有生命周期、静默和错误重试用例一并执行。三语 Observer 示例在 SQLite 编译并运行。实际二进制验证 `make:model ObserverArticle` 和 `inspire`。
- PHP 反射与 `ObservedBy` 属性由 Rust 显式类型装配表达。事务后 Observer 使用 05-01 的显式事务写入作为前置。生成器见下节。
- Rust 1.94.0 格式、615 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语站点构建及 18 项浏览器检查通过。

## Observer 生成器

- 对照固定 Laravel 的 `make:observer` 与生成器测试，接入普通结构体、`--model/-m` 的五个写后回调及 `--force/-f`。沿用编译型应用装配和现有模板发布机制。旧版 `v0.x` 未找到对应生成器。Rust 模块声明和 Provider 注册由应用完成，交互式模型候选提示仍待接入。 [Laravel ObserverMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ObserverMakeCommand.php) · [Laravel 生成器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/ObserverMakeCommandTest.php) · [Goravel Database Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)
- 实际 Artisan 验证普通和绑定模型生成、嵌套及限定模型名、重复拒绝、并发只成功一次、强制覆盖、非法路径拒绝和两个自定义模板。生成的普通及绑定模型代码以 Rust 1.94.0 编译，绑定模型的版本通过 SQLite 真实保存验收。
- Rust 1.94.0 格式、616 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语站点构建及 18 项浏览器检查通过。

## 事务后 Observer

- 对照固定 Laravel Dispatcher 对六个写前方法的即时执行例外，以及事务测试的嵌套提交与连接隔离。Rust 通过 `ShouldHandleEventsAfterCommit` 和 `observeAfterCommit` 显式装配，沿用 Goravel 的事务句柄思路。旧版 `v0.x` 提供模型事件与显式事务派发基础，当前复用已有事务回调管理器。 [Laravel Observer 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#observers-and-database-transactions) · [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php) · [Laravel 事务测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/DatabaseTransactionsTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go)
- Laravel 的回调保留对象引用。Rust 延后回调持有事件时刻的模型克隆，在原应用作用域执行。模型状态克隆保留属性、关系和序列化设置，值对象按其类型的克隆契约复制。事务上下文由模型的显式事务入口提供。
- SQLite 与独立 MySQL 8.4.13 验证写前取消、无事务即时执行、外层提交、保存点及外层回滚、事件时刻属性、回调访问已提交行及回调失败时保留已提交记录。三语示例各在 SQLite 执行一次。实际 `artisan inspire` 退出成功。
- Rust 1.94.0 格式、616 项锁定离线工作区测试及全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语站点构建、12 项通用和 6 项正文浏览器检查通过。

## 事件契约监听器与闭合复核

- 固定 Laravel 按精确、通配、接口监听器的顺序派发。Rust 用 `Event::interfaces()` 显式声明契约名称，避免运行时反射。接口监听器收到原始类型化载荷，通配匹配依据原始事件名。沿用 v0.x 的精确与通配匹配，旧版未找到接口匹配。Goravel 的显式事件与监听器登记作为装配参考。 [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php) · [Laravel 事件测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Events/EventsDispatcherTest.php) · [Goravel Event Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/application.go)
- 真实 HTTP 验证精确、通配及两个接口的执行顺序、原始载荷和停止传播。SQLite 与独立 MySQL 8.4.13 复验原有事务派发，事件专项同时复验条件发现、独立进程缓存和应用隔离。实际二进制运行 `event:list --json` 成功。
- 复核覆盖编译清单、缓存、订阅、同步和事务后派发、Observer 及事件接口。类监听器按派发解析容器依赖仍缺少框架入口。队列监听器的 Worker 验收和扩展行为归 06-04。06-02 继续保留为部分验收。
- Rust 1.94.0 格式、617 项锁定离线工作区测试及全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语构建、12 项通用和 6 项事件正文浏览器检查通过。

## 类监听器容器解析

- 对照固定 Laravel Dispatcher 的延迟容器解析和提交后监听器回调。Rust 通过 `listenClass::<L>` 注册编译期类型，由派发器所属的容器提供绑定。保留现有实例监听和队列注册。Goravel 的编译注册作为参考，旧版 `v0.x` 已有派发时调用的类型化 Provider 监听器。 [Laravel 定义监听器](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md#defining-listeners) · [Laravel Dispatcher](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Events/Dispatcher.php) · [Goravel Event Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/event/application.go)
- SQLite 与独立 MySQL 8.4.13 的真实 HTTP 验收覆盖编译清单与 Facade 注册、异步工厂解析应用依赖、两应用隔离、缺少绑定后重试、传播停止、提交执行与回滚取消。延后监听器在派发时解析实例。Artisan 列表读取注册信息，无需构造监听器。
- 06-02 核心验收范围已闭合。队列监听器的 Worker 运行及扩展行为继续归 06-04。
- Rust 1.94.0 格式、618 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。独立 MySQL 专项 7 项通过。根应用实际执行 `artisan event:list --json` 成功。三语站点构建和 18 项浏览器检查通过。
