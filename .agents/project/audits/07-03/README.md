# 07-03 Processes

依赖 01-01。对照固定 [Processes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/processes.md)、[PendingProcess](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Process/PendingProcess.php)、[InvokedProcess](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Process/InvokedProcess.php)、[Pipe](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Process/Pipe.php) 和 [ProcessTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Process/ProcessTest.php)。Provider 由应用容器装配，沿用固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go) 的编译型注册方式。旧版 `6d1a8192` 没有独立 Process 模块，从 `tools/cli/src/dev_watch.rs` 提取进程组、`waitid(NOWAIT)` 和组长回收顺序，沿用 `tools/cli/tests/unit/dev_watch.rs` 的 TERM trap 与同组后代回归场景。

`Process` 通过应用容器解析独立 Factory。支持 Shell 命令与原样参数、工作目录、环境增删、字节输入、输出回调、退出码及错误转换。默认超时 60 秒，空闲超时由 stdout 和 stderr 共同刷新。Unix 在组长回收前终止同组后代，超时、正常退出和取消均适用。命名进程池同时启动，按注册顺序返回各自结果。Pipe 逐步传递成功输出并在失败时停止。fake 支持匹配、序列、调用断言和拦截意外执行。

Rust 使用 `Duration`、字节切片和类型化 `Result`。进程句柄所有权控制取消，输出回调显式接收数据来源。当前记录匹配的 fake 错误尝试，记录中无退出结果。TTY、`waitUntil`、超时错误内嵌输出、进程描述脚本和任意数值信号尚未对齐。Windows 路径使用直接子进程终止。Linux 和 Windows 未实机验收。

## 验收

- Rust 1.94.0、Apple Silicon macOS。锁定离线工作区 508 项通过，3 个忽略项为集成测试启动的子进程入口。格式与严格 Clippy 通过。
- 八项 Process 回归覆盖真实目录和环境、原样参数、stdin、stdout/stderr、退出码、实时输出、两类超时、取消、TERM/INT、组长退出后清理后代、管道、并行池及两个 Artisan 应用的 fake 隔离。
- 临时 SQLite 上执行 Artisan `migrate --force` 和 `inspire`，退出码均为 0。实际 HTTP 首页及 `/up` 返回 200。
- 三语六份 Rust 示例实际运行，VitePress 构建、12 项通用浏览器检查及 6 项 Processes 页面导航通过。

下一单元 07-04 Concurrency，依赖本单元。
