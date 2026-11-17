# T-04 容器替身与事件假对象

- 对照固定 Laravel 的 [Mocking Objects](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mocking.md#mocking-objects)、[事件测试](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md#testing) 与 [EventFake](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/EventFake.php)。沿用固定 Goravel 的编译型应用装配。旧版 `6d1a8192` 有事件分发和队列回归，未发现独立 EventFake。复用现有容器绑定、分发器及事务回调。
- `TestApp::instance` 安装具体 Rust 服务或 trait 对象，影响当前应用后续解析。`Event::fake` 与 `TestApp::fakeEvents` 共用应用分发器。按事件全名选中或排除，记录类型化负载，跳过监听器、队列及广播。事务回调持有分发时选定的 fake，提交时记录，回滚丢弃。
- 本批对应计划的实例替换、事件断言与应用隔离。Laravel 的通用 Facade spy 和测试时间控制仍需相应的 Rust API 与验收。
- 基于 `0de1bf51`。两个测试应用并行经过 HTTP 内核，验证原绑定替换、响应内容、不同监听器计数、事件范围、`except`、`until` 和负载读取。独立 SQLite 文件和真实 MySQL 8.4.13 验证提交、回滚及在途事务跨 fake 替换。事件定向 5 项通过，包含两种数据库的实际 TCP 事件回归。实际 `artisan inspire` 通过。
- Rust 1.94.0 锁定离线工作区串行 560 项通过，5 项已有夹具忽略。格式及全目标严格 Clippy 通过。首次并行工作区回归有 1 项既有 SQLite 队列测试在释放预约时返回 `LostReservation`，串行复验通过。
- 三语文档六份示例提取执行并通过严格 Clippy。VitePress 构建、12 项通用浏览器检查和 6 项测试替身／事件锚点定向检查通过。

## 事务生命周期事件与替身回归

- 在 `4355d5042` 的干净归档复现全事件替身的全局零事件断言失败。固定 Laravel 的连接分发事务开始、正在提交、已提交和已回滚事件，`EventFake` 只对声明提交后分发的事件注册事务回调。核对旧版事务回归，当前继续沿用既有事务回调和分发器。 [固定 Laravel 事务事件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L1059) · [固定 Laravel EventFake](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/EventFake.php#L337)
- SQLite 与实际 MySQL 8.4.13 共用同一回归。按事件全名断言四类数据库生命周期事件出现，验证延迟业务事件在提交前不可见、成功提交后只由派发时的替身记录一次、回滚后两个替身均无新增该事件，业务监听器未运行。测试和三语文档反映现有运行时契约。
- 真实 MySQL 专项 3 项通过。锁定离线整仓串行验收覆盖 167 个测试目标。标准测试 1,326 项通过、0 项失败、9 项忽略，两个自定义测试程序通过。此前两项 Artisan 回归也在这轮整仓通过。未设置的外部驱动夹具按各用例条件运行，实际 MySQL 路径由专项证明。
- Rust 1.94.0 全仓格式、严格 Clippy、实际 `./artisan inspire`、三语站点构建、12 项通用和 6 项事件文档浏览器检查通过。临时 MySQL 与隔离编译及站点产物已回收，卷可用空间净增 31.62 GB（29.45 GiB），剩余 100.48 GB（93.58 GiB）。提交前周额度剩余 37%。日志在 `/private/tmp/openrust-event-fake.YhYUH4PS`。

## 作用域内的事件替身

- 固定 Laravel 的 `fakeFor` 与 `fakeExceptFor` 在回调期间临时替换分发器，结束时恢复原分发器。核对 `openrust-v0.x` 的事件与事务实现后，沿用当前框架的应用级分发器和 Tokio 作用域做法。Rust 将临时替身绑定到回调 Future 和分发器身份，避免并发请求共享替身。加入 `Event::fakeFor`、`Event::fakeExceptFor` 和应用级 `Event::fakeExcept`。 [固定 Laravel Event 门面](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Event.php#L64)
- 事件定向测试验证允许与拦截名单、返回值、嵌套错误、取消、局部替换，以及同一分发器上的并行 Future。回调结束后恢复原有替身，延迟事件继续使用派发时选定的替身。
- 在 `00e06d0e6` 的隔离归档运行 `event_fake` 与 `events`，真实 MySQL 8.4.13 和 SQLite 路径共 11 项通过、1 项子进程夹具忽略。实际 HTTP 用同一个进程接收两个重叠的 TCP 请求，临时替身只拦截所属请求，另一个请求及作用域结束后的分发均到达监听器。三个语言版本的 Rust 示例均从页面提取并在实际请求中执行。
- Rust 1.94.0 全仓格式、严格 Clippy、根应用 `./artisan inspire`、三语站点构建及 18 项浏览器检查通过。临时 MySQL、隔离 Cargo 和站点产物已回收，卷可用空间净增 9.40 GB（8.76 GiB），清理后可用 100.39 GB（93.50 GiB），提交前周额度剩余 37%。日志在 `/private/tmp/openrust-event-scope.bnVl0WfO`。

## 按事件负载筛选与断言

- 固定 Laravel 的 `EventFake` 按事件名称或回调选择替身，优先放行命中排除规则的事件。对照旧版的事件派发和事务回归，未找到可直接复用的 EventFake 过滤器，继续复用当前事件对象、分发器、异步替身作用域与事务交付。Rust 保留现有按名称签名，通过 `fakeWhen`、`fakeForWhen`、`exceptWhen` 和 `Where` 断言显式接收回调。补齐 `assertDispatchedOnce`。 [固定 Laravel 选择机制](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/EventFake.php#L313) · [固定 Laravel 断言](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/EventFake.php#L123)
- 按名或负载回调选中事件，按名或回调排除优先。读取快照后释放替身锁，再调用用户回调，允许回调读取替身记录。事件在派发时选择替身，交易提交后才记录其延迟事件，回滚丢弃。仅能按名称预判的数据库事件会交由实际负载匹配。
- 在 `b64e48f10` 的干净归档中，`event_fake` 与 `events` 在 SQLite 和真实 MySQL 8.4.13 的定向回归共 12 项通过、1 项子进程夹具忽略。实际 TCP HTTP 通过两个重叠请求验证作用域旁路监听器，三语 Rust 示例提取后在请求中执行，验证符合条件的负载被记录、未命中的负载由监听器处理。测试同时验证分发顺序、排除优先、提交与回滚。
- Rust 1.94.0 格式、全工作区严格 Clippy、`./artisan inspire`、三语站点构建和 18 项浏览器检查通过。回收临时 MySQL、隔离 Cargo 和站点产物，磁盘净增 8.98 GB（8.36 GiB），可用 100.35 GB（93.46 GiB），提交前周额度剩余 37%。日志在 `/private/tmp/openrust-event-filter.uJCbSef0`。
