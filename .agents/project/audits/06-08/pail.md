# Pail 日志跟踪

## 固定对照与交付顺序

以固定 Laravel 文档的 [Tailing Log Messages Using Pail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail) 为入口。固定应用骨架的 `composer.json` 指定 `laravel/pail:^1.2.5`，本专项固定 Pail v1.2.5，提交 `fdb73f5eacf03db576c710d5a00101ba185f2254`。核对旧版 `v0.x` `6d1a81924`，未找到 Pail 或日志监听入口的对应实现。日志驱动、应用装配、进程监督与信号处理复用当前主线。 [Pail 源码](https://github.com/laravel/pail/tree/fdb73f5eacf03db576c710d5a00101ba185f2254) · [固定 Laravel Logger](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Logger.php)

1. 日志同步监听。接通 `Log::listen`、`LogManager::listen` 和 `Logger::listen`，写入被渠道接受的日志后同步通知 `MessageLogged`，涵盖命名、临时、重命名、自定义及应急渠道。保持原消息和 context，保留结构化异常。验证阈值、顺序、回调错误、重入、注册期间快照和应用隔离。
2. Pail 捕获。独立 `vendor/openrust/pail/` 包，公开 `PailServiceProvider`，通过现有包发现及可选依赖接入。与日志驱动无关，订阅日志通知，向当前应用 `storage/pail/*.pail` 会话写入记录。接通 HTTP、命令和队列来源、认证账户及公开 Context，处理并发写入、会话过期和文件清理。
3. Pail 命令。按上游 `pail` 的 `--filter`、`--message`、`--level`、`--auth`、`--user`、`--timeout=3600` 及 `-v`／`-vv` 实现筛选与输出。实际 HTTP、独立 Artisan、Worker 和并发捕获验收，覆盖超时与 Ctrl+C 清理、成功和失败路径，再接开发命令注册。

## Rust 适配

当前日志渠道使用同步 `Result` 契约。日志监听闭包也同步返回 `LogResult<()>`，在当前应用和请求作用域内执行，错误沿调用链传播。Pail 通过 `Log::listen` 订阅，不阻塞 Tokio 去等待异步监听器。框架的通用异步 `Event` 分发保留独立契约，`MessageLogged` 提供事件身份供显式异步分发使用。

PHP 序列化异常转换为现有 Rust 结构化异常快照。Pail 的跨进程会话使用 JSON 行和文件锁，沿用同一应用根目录。Rust 直接异步读取会话并监控取消信号，PHP 的 PCNTL、脚本子进程及 Composer 安装转换为编译依赖、Provider 和 Tokio 信号。来源元数据、筛选、会话生命周期和可观察退出行为依固定上游逐项验收。

## 状态

[日志同步监听](../01-04/log-listeners.md)已实现并通过实际 HTTP、独立 Artisan、22 项回归和全目标严格 Clippy。[Pail 会话捕获](pail-capture.md)已通过双应用 HTTP、独立 Artisan、并发命令／同步任务、MySQL Worker、隔离任务 deferred、文件生命周期和严格 Clippy 验收。同步日志只读取已解析认证身份，与 PHP 自动解析的差异仍待闭合。[认证入口复核](pail-auth-boundary.md)确认现有同步日志无法安全驱动独占借用请求的异步 Guard，该项保留未闭合。独立推进 Pail 命令依赖：[消息对象与筛选](pail-options.md)后，依次接 CliPrinter、会话命令、超时／信号清理及 dev 注册。

## Provider 剩余差异

固定 PailServiceProvider 在非 Pail 自身测试中跳过普通应用测试环境与 PHP Vapor 的 `VAPOR_SSM_PATH` 环境。当前包尚未接入该门禁；框架尚无同名 `runningUnitTests`／`runningInConsole` 查询。需要按当前应用环境和编译型入口核定适配并验收，不能仅以捕获测试通过视为 Provider 全部对齐。下一批先补环境门禁，再推进 CliPrinter、实际会话命令、超时／信号清理与 dev 注册。[固定 Provider](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/PailServiceProvider.php)
