# 同步日志监听

## 对照与实现

对照固定 Laravel `Logger::listen`、`writeLog` 和 `fireLogEvent`，以及 `LogManager` 的共享 dispatcher 与应急 Logger。Pail 的后续范围见 [专项记录](../06-08/pail.md)。检查旧版 `v0.x` `6d1a81924` 的日志管理器和回归，未找到对应监听入口，本批复用主线渠道、处理器、应急写入和应用作用域。核对固定 Goravel 的派生渠道共享缓存及容器生命周期，监听器随当前管理器共享并按应用隔离。 [Goravel Log Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go) [Laravel Logger](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Logger.php) · [Laravel LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php)

- 公开 `openrust::support::facades::Log::listen`、`LogManager::listen` 和 `Logger::listen`。同一管理器生成的命名、临时、重命名、日志栈、自定义与应急 Logger 共用监听器，各应用独立。
- `MessageLogged` 包含原消息、级别和 context，异常通过现有规范化快照放入 context。渠道处理器及插值对实际写出记录生效，通知保留调用时的载荷。所有处理器均不接受该级别时跳过通知。已接受的空渠道仍通知，应急记录与诊断也通知。
- 写入后按顺序同步调用闭包。回调返回错误时停止后续通知并传播错误，已完成的写入可观察。注册表在调用闭包前释放锁，支持回调重入和注册。一次通知使用注册快照，新注册参与下一次通知。
- Rust 使用同步 `Fn(&MessageLogged) -> LogResult<()>` 表达 PHP 同步事件闭包。保持现有同步日志渠道契约，通用异步 `Event` 采用显式分发。`MessageLogged` 实现事件身份契约。无监听器时跳过事件内容复制。

## 实际验收

隔离源码以 `48561caaa` 为基线，仅覆盖本批六个 Rust 文件。Ubuntu 原生 amd64、Rust 1.94.0，未优化构建。验收后核对所有候选源码 SHA-256。

- `cargo test --locked --offline -p openrust-framework --test log_listeners --test logging --test log_context` 通过 22 项。新增 5 项覆盖跨渠道与阈值、写入后通知、原载荷与处理器、空与应急渠道、注册与重入、错误传播、结构化异常，以及两个应用的并发真实 TCP HTTP 和 `handleCommand` 隔离。
- 全工作区 `cargo clippy --locked --workspace --all-targets -- -D warnings` 通过。首次离线检查缺少工作区其它 crate 的缓存依赖，随后按锁文件下载并完成检查。`cargo +1.94.0 fmt --all --check` 通过。
- 在隔离应用的 `routes/console.rs` 中临时登记 `listener:probe`，监听闭包写出 JSON，执行 `./artisan listener:probe` 和 `./target/debug/main artisan listener:probe` 均退出 0。两份捕获均为原消息 `Artisan {source}` 和 context `source=binary`。`./artisan inspire` 退出 0。命令夹具在退出时恢复，核对原文件字节一致。
- 三语日志正文同步，`npm --prefix docs run build` 及根路径、`/docs/` 的浏览器检查通过。本批覆盖 Linux，平台发布矩阵继续按项目计划执行。

## 文件范围

```text
.agents/project/
├── audits/
│   ├── 01-04/log-listeners.md
│   ├── 06-08/README.md
│   ├── 06-08/pail.md
│   └── core-closure-ledger.md
├── current.md
└── progress.md
docs/
├── en/the-basics/logging.md
├── zh_CN/the-basics/logging.md
└── zh_TW/the-basics/logging.md
vendor/openrust/framework/
├── src/log/
│   ├── events/message_logged.rs
│   ├── events/mod.rs
│   ├── facade.rs
│   ├── log_manager.rs
│   └── logger.rs
└── tests/log_listeners.rs
```

本批远程快照与编译缓存、站点产物已回收，数据卷清单一致。提交前磁盘可用 56.58 GB（52.70 GiB），周额度剩余 16%，第二窗口无读数，按用户忽略本轮额度停线的授权继续。原始验收日志及命令夹具保留于 `/private/tmp/openrust-pail.BLlS903Z`。
