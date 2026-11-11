# 01-03 日志渠道、默认栈与文件输出

按固定 Laravel 13.x 的 [配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#configuration)、[日志栈](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#building-log-stacks)和[写入日志](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#writing-log-messages)核对。对照固定版 [LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php)及[集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/LogManagerTest.php)，参考固定 [Goravel LogServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/service_provider.go)的应用单例装配。

- 从旧版 `6d1a8192` 的 `src/logging/{manager,sinks}.rs` 和 `tests/unit/logging/{manager,sinks}.rs` 提取按应用缓存、栈展开、逐行转义、级别过滤、紧急回退、配置修复与文件重新打开回归。
- 框架基础 Provider 自动登记 `log`、类型化 `LogManager` 和日志契约。Facade 和服务契约解析到同一管理器。默认 `stack → single` 向 `storage/logs/openrust.log` 追加，保持现有配置和环境变量。
- `single`、`stderr`、`null`、`stack` 已接通。文件在达到阈值后惰性打开，支持 Unix 权限和文件锁。普通栈按顺序处理并尊重 `bubble`，`null` 接收后停止传播，忽略失败的组尝试全部处理器。无效渠道或循环栈触发紧急记录。写入失败清退文件句柄，记录诊断并将原记录交给紧急渠道，紧急文件失败再写 stderr。
- Rust 沿用同步日志契约和类型化 JSON 上下文，使用 Chrono 的标准库功能格式化 UTC 时间。诊断不输出原始配置或文件路径。写入阶段重试和 stderr 二级回退是本项目的行为，固定 Laravel 的紧急 Logger 主要处理渠道解析异常。自定义 formatter 配置目前触发紧急渠道。轮转、自定义驱动、事件派发及请求日志上下文由后续交付单元接入。

## 验收

macOS arm64，Rust 1.94.0。在 `21a65e98` 加已验收的 01-02y、01-02z 和本批改动的隔离副本验证。工作区原有 gRPC、额外契约、Facade 等改动未带入隔离副本。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，314 项工作区测试含 5 项日志测试。
- 三次真实回环 TCP HTTP 请求验证两个应用的独立日志文件。六线程同步写入验证渠道实例共享、完整行及记录数量。落盘覆盖级别、栈顺序、锁、权限、上下文转义、循环栈和写入故障恢复。
- Artisan 子进程实际执行 `log:probe`，验证默认文件、显式 stderr 以及紧急文件无法打开时的 stderr 回退。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语示例分别用 Rust 1.94.0 编译并检查实际落盘。`npm run build` 成功，含打包体积提示。两种部署路径、三语、桌面与移动共 12 个浏览器组合通过，检查日志正文和导航。

下一批为 01-04 轮转与自定义渠道，复用已验收的文件处理器、渠道解析缓存与日志栈。
