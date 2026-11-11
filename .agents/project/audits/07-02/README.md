# 07-02 HTTP Client

依赖 01-01。对照固定 [Laravel HTTP Client](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/http-client.md)、[Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/Factory.php)、[PendingRequest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Client/PendingRequest.php) 和 [HTTP 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpClientTest.php)。装配沿用本项目编译型 Provider，参照固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)。复核旧版 HTTP Client 及服务器回归，沿用 reqwest、超时、流式大小校验、错误脱敏和禁止隐式环境代理的做法。

`Http` 从应用容器取得 `Factory`。每个应用拥有独立连接池、fake 注册、响应序列和请求记录。配置链生成一次性 `PendingRequest`。支持常用 HTTP 方法、JSON、表单、原始正文、multipart、查询参数、Bearer 与 Basic、重定向开关、请求前和响应后回调、状态判断、有限重试、fake、测试断言及并发池。HTTP 4xx、5xx 由调用方决定是否转换成错误，重试耗尽的行为可配置。并发池的每项结果独立，并按注册顺序返回。

Rust 使用 `Duration`、类型化 JSON 及分开的配置方法表达 PHP 可选参数。响应缓冲复用字节存储，默认连接选项复用 reqwest 连接池，自定义连接超时或重定向选项创建独立客户端。重试前校验请求是否可重放，一次性流在启用重试时返回明确错误。请求记录可观察已缓冲的正文，流式上传的正文不可回读。显式查询项覆盖 URL 查询串，编码使用 RFC 3986。原始 `reqwest::Request` 可由请求前回调调整。Laravel 的 Guzzle 专用选项、动态宏、Batch 回调、HTTP 事件及 Digest Auth 尚未对齐。旧版目标白名单、DNS 固定和熔断属于额外传输能力，当前范围未接入这些能力。

## 验收

- Rust 1.94.0，`cargo fmt --all --check`、`cargo test --workspace --locked --offline` 和 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。工作区 500 项通过，3 项为集成测试启动的子进程入口。
- 五项定向测试覆盖实际 HTTP JSON、表单、multipart、Raw、认证、查询编码、重定向、状态处理、超时、响应大小、重试成功及耗尽、fake 拦截与记录、并发池和两个实际 HTTP 应用间的 Facade 隔离。
- 独立启动验收使用临时 SQLite，Artisan `migrate --force` 和 `inspire` 退出 0，`GET /up` 和 `GET /` 返回 200。
- 英文、简体、繁体文档的三份 Rust 示例实际运行。VitePress 构建、12 项通用浏览器检查及 6 项 HTTP Client 页面内导航通过。
- 按隔离副本验证后，独立核对与在途 gRPC 差异合并的工作区锁文件。锁定离线 metadata 保留 reqwest、tonic 和 tokio-stream 的解析。

下一单元 07-03 Processes，前置 01-01 已提交。
