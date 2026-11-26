# Pail 日志跟踪

## 固定对照与交付顺序

以固定 Laravel 文档的 [Tailing Log Messages Using Pail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail) 为入口。固定应用骨架的 `composer.json` 指定 `laravel/pail:^1.2.5`，本专项固定 Pail v1.2.5，提交 `fdb73f5eacf03db576c710d5a00101ba185f2254`。核对旧版 `v0.x` `6d1a81924`，未找到 Pail 或日志监听入口的对应实现。日志驱动、应用装配、进程监督与信号处理复用当前主线。 [Pail 源码](https://github.com/laravel/pail/tree/fdb73f5eacf03db576c710d5a00101ba185f2254) · [固定 Laravel Logger](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Logger.php)

1. 日志同步监听。接通 `Log::listen`、`LogManager::listen` 和 `Logger::listen`，写入被渠道接受的日志后同步通知 `MessageLogged`，涵盖命名、临时、重命名、自定义及应急渠道。保持原消息和 context，保留结构化异常。验证阈值、顺序、回调错误、重入、注册期间快照和应用隔离。
2. Pail 捕获。独立 `vendor/openrust-pail/` 包，公开 `PailServiceProvider`，通过现有包发现及可选依赖接入。与日志驱动无关，订阅日志通知，向当前应用 `storage/pail/*.pail` 会话写入记录。接通 HTTP、命令和队列来源、认证账户及公开 Context，处理并发写入、会话过期和文件清理。
3. Pail 命令。按上游 `pail` 的 `--filter`、`--message`、`--level`、`--auth`、`--user`、`--timeout=3600` 及 `-v`／`-vv` 实现筛选与输出。实际 HTTP、独立 Artisan、Worker 和并发捕获验收，覆盖超时与 Ctrl+C 清理、成功和失败路径，再接开发命令注册。

## Rust 适配

当前日志渠道使用同步 `Result` 契约。日志监听闭包也同步返回 `LogResult<()>`，在当前应用和请求作用域内执行，错误沿调用链传播。Pail 通过 `Log::listen` 订阅，不阻塞 Tokio 去等待异步监听器。框架的通用异步 `Event` 分发保留独立契约，`MessageLogged` 提供事件身份供显式异步分发使用。

PHP 序列化异常转换为现有 Rust 结构化异常快照。Pail 的跨进程会话使用 JSON 行和文件锁，沿用同一应用根目录。Rust 直接异步读取会话并监控取消信号，PHP 的 PCNTL、脚本子进程及 Composer 安装转换为编译依赖、Provider 和 Tokio 信号。来源元数据、筛选、会话生命周期和可观察退出行为依固定上游逐项验收。

## 状态

[日志同步监听](../01-04/log-listeners.md)已实现并通过实际 HTTP、独立 Artisan、22 项回归和全目标严格 Clippy。[Pail 会话捕获](pail-capture.md)已通过双应用 HTTP、独立 Artisan、并发命令／同步任务、MySQL Worker、隔离任务 deferred、文件生命周期和严格 Clippy 验收。同步日志只读取已解析身份；[请求日志时序](pail-auth-timing.md)已实现显式 await 入口，并接入内置 HTTP 异常报告。[认证入口复核](pail-auth-boundary.md)说明同步回调的借用约束；V04 已通过独立请求入口完成 Rust 适配。独立推进 Pail 命令依赖：[消息对象与筛选](pail-options.md)、统一路径和 [CLI 打印器](pail-printer.md)已验收，[实时命令](pail-command.md)、超时／信号和取消清理已验收，[dev 默认注册](pail-dev.md)也已验收。

## Provider 环境门禁

[环境门禁与延迟解析](pail-provider.md)已接通：Application 与其契约的 runningUnitTests、应用环境快照、PAIL_TESTS 例外、testing／Vapor 跳过条件，以及首次日志／队列事件解析 Handler。真实 HTTP／Artisan 矩阵、缓存、并发隔离、MySQL Worker、根自动发现、严格 Clippy 和三语正文通过。[命令注册](pail-command.md)现已通过所属应用的 ConsoleKernel 回调接通，HTTP、独立 Artisan 与程序化调用边界已验收；不复制 PHP 的全局 SAPI 状态。

## 剩余契约

- [认证时序](pail-auth-timing.md)已通过显式请求日志入口、同步快照和 HTTP 异常报告验收，不提前认证或响应后回填；普通同步日志不能自动等待异步 Guard。
- [统一存储路径与 Pail 目录](../07-01/storage-path.md)已通过真实 HTTP、Artisan 与 MySQL Worker 验收。Application storagePath／useStoragePath、storage_path helper 和所属容器的 Files 工厂已接通。[配置与文件消费者](../07-01/storage-consumers.md)已迁移并通过实际二进制验收，保留配置加载／缓存与服务解析时机；[CLI 打印器](pail-printer.md)及[实时命令](pail-command.md)也已验收，[dev 默认注册](pail-dev.md)也已验收。
- [CliPrinter](pail-printer.md)及[实时命令与清理](pail-command.md)已闭合；[dev 默认注册](pail-dev.md)也已闭合；认证时序已按 V04 的 Rust 入口验收，Windows 发布矩阵仍待完成。
