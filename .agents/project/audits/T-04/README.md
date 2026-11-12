# T-04 容器替身与事件假对象

- 对照固定 Laravel 的 [Mocking Objects](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mocking.md#mocking-objects)、[事件测试](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md#testing) 与 [EventFake](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/EventFake.php)。沿用固定 Goravel 的编译型应用装配。旧版 `6d1a8192` 有事件分发和队列回归，未发现独立 EventFake。复用现有容器绑定、分发器及事务回调。
- `TestApp::instance` 安装具体 Rust 服务或 trait 对象，影响当前应用后续解析。`Event::fake` 与 `TestApp::fakeEvents` 共用应用分发器。按事件全名选中或排除，记录类型化负载，跳过监听器、队列及广播。事务回调持有分发时选定的 fake，提交时记录，回滚丢弃。
- 本批对应计划的实例替换、事件断言与应用隔离。Laravel 的回调筛选、`fakeFor`、通用 Facade spy 和测试时间控制仍需相应的 Rust API 与验收。
- 基于 `0de1bf51`。两个测试应用并行经过 HTTP 内核，验证原绑定替换、响应内容、不同监听器计数、事件范围、`except`、`until` 和负载读取。独立 SQLite 文件和真实 MySQL 8.4.13 验证提交、回滚及在途事务跨 fake 替换。事件定向 5 项通过，包含两种数据库的实际 TCP 事件回归。实际 `artisan inspire` 通过。
- Rust 1.94.0 锁定离线工作区串行 560 项通过，5 项已有夹具忽略。格式及全目标严格 Clippy 通过。首次并行工作区回归有 1 项既有 SQLite 队列测试在释放预约时返回 `LostReservation`，串行复验通过。
- 三语文档六份示例提取执行并通过严格 Clippy。VitePress 构建、12 项通用浏览器检查和 6 项测试替身／事件锚点定向检查通过。
