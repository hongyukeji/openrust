# 04-02 请求、响应和控制器

## 契约与实现

- 对照固定 Laravel Request、InteractsWithInput、ControllerDispatcher 和响应文档，参考 Goravel 的请求输入与文件契约。复用旧版 `6d1a8192` 的查询变量树、UTF-8 校验、multipart 分离文本与文件、请求体限额及原始正文复用规则。统一业务响应沿用此前迁入的 ApiResponse。 [Laravel Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md) · [Laravel Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md) · [Laravel Controllers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/controllers.md) · [Goravel Request](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/request.go)
- 顶层正文输入优先于查询，保留显式 null。普通 GET/HEAD 使用查询，JSON 媒体类型优先选 JSON。支持精确 query、点号 input、JSON 专用读取、表单数组与 multipart 文件。正文最多缓冲 8 MiB，成功后复用字节。无效编码和正文映射为 400，超量映射为 413。Rust 使用类型化错误拒绝畸形 JSON 和非容器标量。
- Rust 通过显式控制器绑定与类型化方法指针完成应用容器分派。响应支持 JSON、状态码、类型化响应头与异步流。上传元数据与文件字节分开读取。大文件磁盘暂存、文件保存辅助方法、更多类型化输入、资源控制器约定和 Windows 实机仍待覆盖。

## 验收

隔离基线 `21a65e98` 加已验收依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 402 项通过。本批未启用真实数据库或外部存储。
- 真实回环 HTTP 覆盖 JSON 与查询冲突、空值、表单、GET 查询、multipart 二进制上传、两个应用控制器实例隔离、重复读取原始正文、400、413、自定义响应头和分块流。
- 三语控制器示例分别编译并发出 JSON、表单及无效正文请求。实际运行 `./artisan route:list --json --method=HEAD` 与 `./artisan inspire` 成功。
- 三语文档站通过构建、12 项通用浏览器检查及 6 项请求、响应、控制器正文专项检查。

下一批为 04-03 中间件顺序、别名和 CORS，依赖 04-01 路由入口。
