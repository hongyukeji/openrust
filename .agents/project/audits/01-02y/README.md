# 01-02y 请求属性与路由参数注入

对应 [Laravel 13.x 上下文属性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#contextual-attributes)、[RequestAttribute](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/RequestAttribute.php)、[RouteParameter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes/RouteParameter.php)和[容器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualAttributeBindingTest.php#L242-L268)。HTTP 装配保留固定 [Goravel 的 Boot/Start 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 从旧版 `6d1a8192` 的 `container/attributes/request.rs`、`request_tests.rs` 和 `http/request/attributes.rs` 提取属性精确匹配、路由参数名回退、类型保留、输入隔离及独立容器回归。
- `RequestAttribute::new(name)` 读取服务端属性袋。显式 null 和缺失值进入容器参数处理。请求头、查询参数和表单无法直接填充属性袋。
- `RouteParameter::new(name)` 按名称读取匹配值。`RouteParameter::default()` 使用注入参数名。模型替换前为路由字符串，替换后使用和 `Request::route_model` 相同的对象。Rust 通过共享的类型化值传递模型，保留原路由 API 的借用形式。
- Rust 通过 HTTP 内核建立 Tokio 请求上下文，按请求和容器身份隔离。没有 HTTP 入口时属性提供 null。Laravel 源码通过容器中的 `request` 服务取值，Rust 由内核提供状态。响应体流目前在分发作用域外轮询，旧版流式上下文回归仍需 HTTP 生命周期接线后重验。

## 验收

基线 `21a65e98` 加本批 14 个实现、测试和文档文件的隔离副本，macOS arm64，Rust 1.94.0。工作区 `http/mod.rs` 的已有 API Resources 声明在隔离副本中排除。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 307 项测试。
- 本批 5 次真实回环 TCP HTTP 请求覆盖匹配字符串、模型对象身份、同步和异步注入、请求头和查询参数隔离、并行请求和独立应用、模型缺失 404 及后续恢复。16 个入口探针通过，`artisan construction:probe` 断言两个属性在命令内均为空。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三个语言示例通过 Rust 1.94.0 编译和执行。`npm run build` 成功，有打包体积提示。沿用浏览器检查并加入本节正文和导航断言，`/` 与 `/docs/` 下三语桌面和移动组合共 12 项通过。

下一批核对 01-02 剩余容器公开契约及属性后置回调入口，依赖已完成的属性派发和解析事件。服务型属性等待对应组件的运行契约验收。流式响应作用域纳入 HTTP 生命周期验收。
