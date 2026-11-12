# T-02 HTTP 与 Artisan 测试指南

对照固定 [Laravel HTTP Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/http-tests.md) 和 [Console Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/console-tests.md)，沿用固定 [Goravel 应用入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的编译型装配。核对旧版 `6d1a8192` 的 `testing/app.rs` 和 `testing/response.rs`，复用正常 HTTP 内核、响应体收集上限和超时、响应断言及控制台调用思路。

`TestApp::call` 和 `get` 通过正常 HTTP 内核分发请求并执行终止阶段。测试响应可检查状态、头、正文和 JSON。Artisan 测试使用与生产入口共享的控制台执行函数，捕获命令输出并保留退出码。HTTP 连接元数据和命令直接写入进程流的输出通过真实监听或子进程验收。

## 验收

- 测试夹具覆盖重复请求、状态、响应头、JSON、命令输出、退出码及两应用隔离。三语 HTTP 和控制台示例提取后通过两项测试。
- Rust 1.94.0 锁定离线工作区 536 项通过，5 项忽略测试由集成测试启动。格式、严格 Clippy 通过。
- 启动实际应用验证 `GET /`、`GET /up` 和 Artisan 迁移、`inspire`。三语站点构建通过，12 项通用检查及 12 项 HTTP／控制台文档侧栏和页面检查通过，覆盖根路径和 `/docs/`。

下一单元 T-03 数据库测试指南。
