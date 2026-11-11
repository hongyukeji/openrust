# 01-05 请求及任务 Context

对照固定 Laravel 13.x [Capturing Context](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/context.md#capturing-context)、[Scoped Context](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/context.md#scoped-context)、[Hidden Context](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/context.md#hidden-context)及[日志元数据](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#contextual-information)。核对 [Repository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Context/Repository.php)、[ContextLogProcessor](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/Context/ContextLogProcessor.php)、[ContextTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/ContextTest.php)和[容器属性](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/Context.php)。参考固定 [Goravel 日志应用](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)按执行上下文取值及应用级渠道缓存。

- 从旧版 `6d1a8192` 的 `src/logging/context.rs`、`tests/unit/logging/context.rs` 和 `src/container/attributes/log_context.rs` 提取公开/隐藏分离、显式 null、独立元数据、容器作用域及类型化注入规则。
- `Context` Facade 访问当前生命周期仓库。HTTP、Artisan 和显式容器任务作用域使用独立仓库。嵌套异步范围按任务轮询使用覆盖快照，正常退出、错误、取消和 panic 均恢复外层。Rust 提供同步 `scopeSync`，使用明确的 JSON 字符串键、严格列表比较和检查溢出的整数计数。
- 日志按拥有该渠道的应用及当前容器作用域读取公开字段。记录字段与公开元数据分为两组，同名键各自保留，插值只读记录字段。隐藏值仅可显式读取，类型错误诊断不包含字段值。自定义 sink 可通过 `logWithContext` 接收两组字段，已有实现仍支持原方法。
- 类型化 Context 属性支持公开或隐藏键和静态或惰性默认值。缺失或 null 使用默认值，JSON 转换采用声明类型。队列序列化和还原事件归 06-04，响应体流仍受当前 HTTP 分发作用域边界约束。

## 验收

macOS arm64，Rust 1.94.0。隔离副本基于 `21a65e98` 加已验收的 01-02y 至 01-04 和本批文件。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 321 项测试。三项新增专项验证仓库行为、并发子作用域、取消/panic 清理、任务隔离及属性注入。
- 四次真实回环 HTTP 请求验证同一应用并发和下一次请求均从空仓库开始，两个应用的文件保持独立，同名字段分组，隐藏值不落盘。Artisan 子进程执行 `log:probe`，验证文件、stderr 与自定义月渠道的公开元数据及隐藏字段排除。 `RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语 Context 示例编译并运行，文档站构建通过。12 个浏览器组合验证根路径与子路径、日志页到 Context 的链接和作用域小节。

下一批 01-06 接默认异常日志，依赖已验收的日志渠道和 Context 生命周期。重点核对实际 HTTP 500 和 Artisan 失败的默认日志、报告回调停止、级别映射、忽略和按异常实例去重。
