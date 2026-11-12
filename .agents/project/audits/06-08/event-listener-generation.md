# 06-08 Artisan 事件与监听器生成

- 对照固定 Laravel [事件章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md#generating-events-and-listeners)、[EventMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/EventMakeCommand.php) 和 [ListenerMakeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ListenerMakeCommand.php)。参考固定 Goravel [生成器目录](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/event/console) 的编译型事件与监听器。旧版 `v0.x`（`6d1a8192`）无对应 Artisan 生成器，`vendor/openrust/framework/tests/unit/events/queued.rs` 提供事件常量名、Serde 载荷和队列调度先例。接入主线 06-02 的事件注册和 06-04 的队列监听器。
- `make:event` 与 `make:listener` 使用 Laravel 的应用目录和 `--force` 语义。监听器按 `--event/-e` 和 `--queued` 选出四种模板，`--test` 复用配套功能测试生成。生成路径使用 Rust 小写模块名，应用限定名可显式选择目录。事件模板通过 Serde 支持队列序列化，应用显式声明模块并通过 `withEvents` 装配。Rust 配套测试使用现有 Cargo 测试生成器。
- 以 `42d5b52a` 的隔离副本验收。真实 Artisan 覆盖并发写入保护、应用目录隔离、四种监听器模板、非法事件路径、配套测试和自定义模板覆盖。
- 三语正文 15 条 `./artisan` 命令实跑。生成源码和文档注册示例编译通过。实际 HTTP 返回 204，两个同步监听器即时执行，两个队列任务落入 SQLite，由两个独立的 `queue:work database --once` 进程消费。核对 `event:list --json` 列出四个注册、生成的功能测试通过，以及 `./artisan inspire` 正常执行。
- 三语站点构建、12 项通用浏览器检查和 6 项生成小节检查通过。
- Rust 1.94.0 工作区锁定离线串行测试 569 项通过，5 项既有辅助夹具忽略。`cargo fmt --all --check` 与工作区全目标严格 Clippy 通过。
