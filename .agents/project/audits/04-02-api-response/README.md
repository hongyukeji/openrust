# 04-02 统一 JSON 业务响应

本批验收范围为 `openrust::http::{ApiResponse, PageMeta}` 及路由返回值适配。04-02 的请求、控制器与其他响应类型仍按交付计划推进。

- [Laravel 13.x JSON Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses) 和 [ResponseFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResponseFactory.php) 以 HTTP 状态及 JSON 内容类型构造响应。固定版 [Goravel](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go) 将 JSON 状态和业务数据交给响应接口。
- 从 `openrust-v0.x` 的 `6d1a8192:vendor/openrust/framework/src/core/error.rs` 提取业务字段、`PageMeta`、Serde 省略空字段规则和状态构造方法。复用 `tests/unit/core/error_i18n.rs` 的序列化失败脱敏用例，接到当前 HTTP 内核的异常管线。
- 路由闭包可以直接返回封装。需要自行处理请求的处理器可调用 `into_response()`，用类型化结果把序列化失败传给框架。封装按路由选择并发送真实 HTTP 状态。旧版的全局 HTTP 200 模式、请求 ID、失败时间戳和消息翻译未迁入此类型。Laravel 风格异常由 `withExceptions` 处理。

## 验收

基线 `2770e0fc` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。标准测试结果合计 271 项通过。
- 本批测试经回环 TCP 向 HTTP 内核发送 8 个请求，覆盖 201、分页、业务 422、无载荷 200、空正文 204、序列化失败 500、缺失路由 404 和健康检查 200。断言状态、内容类型、字段及私密错误正文脱敏。
- `RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 成功。三个文档代码块通过 Rust 1.94.0 编译。
- 文档 `npm run build` 成功，`node tests/check_browser.mjs` 在根路径和子路径共 12 项通过。确认三语响应页面均生成。
