# 01-07 异常渲染、自定义响应与脱敏

对照固定 Laravel 13.x [配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#configuration)、[异常渲染](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#rendering-exceptions)、[自定义响应](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#customizing-the-exception-response)及[错误页](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#custom-http-error-pages)，核对 [Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)和[集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Foundation/ExceptionHandlerTest.php)。应用装配参考固定 [Goravel Boot/Start](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 复用旧版 `6d1a8192` 的 `src/foundation/exceptions.rs` 中来源信息限制、HTML 转义、状态页回退和生产脱敏回归。应用即使省略 `withExceptions` 也根据请求协商默认 JSON/HTML。保留 `render` 首个有值响应及最终 `respond`。
- 通用 500 的生产 JSON 消息按固定 Laravel 返回 `Server Error`。状态码和 HTML 状态标题保留。调试 JSON 给出异常来源消息，HTML 调试详情转义。Rust `Error` 无统一文件、行号和追踪接口，采用有上限的来源链。调试详情由当前应用的 `app.debug` 控制。
- 默认 HTML 通过现有视图根查找 `errors/{status}.html`，必要时查找 `4xx.html` 或 `5xx.html`。404、500 和 503 保留专用回退顺序。Rust 沿用静态 UTF-8 视图读取。Blade 渲染和认证、验证的专用异常交由各组件单元。
- 默认 405 在 JSON、HTML 及 HEAD 保留 `Allow`。HEAD 丢弃正文并在长度已知时保留响应元数据，信息响应及 204、304 不补写正文长度。

## 验收

macOS arm64，Rust 1.94.0。隔离副本基于 `21a65e98` 与已验收的 01-02y 至 01-06，加入本批文件。工作区已有广播、授权和视图扩展未带入候选。未纳入隔离副本的应用 HTTP 测试文件仅同步了受影响的 500 JSON 断言。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 326 项测试。
- 13 次真实回环 HTTP 专项请求覆盖默认协商、状态页、调试与生产脱敏、回调回退、最终响应、HEAD 和 405 `Allow`。主程序独立运行后，三条 `curl` 验证 Web 404、API 404 和显式 JSON 404。
- `RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。三语各两段 Rust 示例编译执行。文档构建成功，12 个浏览器组合通过并验证异常文档和 Context 链接。

下一批 02-01 接数据库配置、连接与基本 SQL，依赖 01-01 的配置和 Provider 装配，并需实际 SQLite 与专用 MySQL 8.4 服务验收。
