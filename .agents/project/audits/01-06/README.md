# 01-06 异常报告接默认日志

对照固定 Laravel 13.x [报告异常](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#reporting-exceptions)、[级别](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#exception-log-levels)、[忽略](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#ignoring-exceptions-by-type)和[去重](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#deduplicating-reported-exceptions)，核对 [Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)和[集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Foundation/ExceptionHandlerTest.php)。日志解析沿用固定 [Goravel 应用单例装配](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/service_provider.go)作为实现参考。

- 旧版 `6d1a8192` 的 `src/foundation/exceptions.rs` 提供类型及来源链匹配、报告停止、忽略谓词、级别映射和共享实例身份。提取报告规则和弱身份跟踪。按固定 Laravel 修正首次匹配级别顺序，以及被忽略的实例不占用去重登记。
- Handler 通过应用容器的日志契约选择配置渠道。默认 ERROR，支持 `level`、`context`、`dontReport`、`dontReportWhen`、`stopIgnoring` 和 `dontReportDuplicates`。报告回调停止时保留 HTTP/Artisan 渲染。常规 HTTP 客户端错误仍按内置规则忽略。
- Rust 的 `SharedException` 用 Arc 标识对象，通过 Weak 跟踪同一实例，支持显式 `Application::report` 和 HTTP 错误来源链。普通借用错误可能复用栈地址，因此仅对显式共享实例去重。去重登记按应用隔离并在并发调用间原子完成。
- HTTP 默认报告记录状态码和状态消息，省略来源对象内容。Laravel 会向日志传递异常对象，Rust 保留现有 HTTP 来源脱敏策略，应用可通过报告回调及上下文字段附加允许记录的诊断。启动失败若发生在日志绑定前，仍使用现有终端诊断。队列限流和渲染扩展不属于本批验收范围。

## 验收

macOS arm64，Rust 1.94.0。在 `21a65e98` 加已验收的 01-02y 至 01-05 和本批文件的隔离副本执行。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 324 项测试。
- 8 次真实回环 HTTP 请求覆盖默认日志、级别、公开元数据、隐藏字段排除、报告停止、按类型或条件忽略、普通 404 和共享实例重复。4 个线程同时报告一个共享实例，只写入一次。不同应用各自记录同一实例，忽略条件解除后可报告。
- Artisan 子进程执行故意失败的 `report:probe`，实际退出码为 1，保留 stderr 输出且文件写入异常及公开 Context。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语示例分别编译运行，均在隔离目录生成一条 WARNING 日志。文档构建及根路径、子路径共 12 个浏览器组合通过。

下一批 01-07 接异常渲染、自定义响应及生产脱敏，使用本批已接通的报告链分别验证报告和响应行为。
