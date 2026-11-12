# 06-04 HTTP 维护门禁与 Artisan down／up

## 固定基线

对照 Laravel 的 [Maintenance Mode](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#maintenance-mode)、[HTTP 中间件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/Middleware/PreventRequestsDuringMaintenance.php)、[维护集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Foundation/MaintenanceModeTest.php) 和 [Goravel 维护命令](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/console/maintenance.go)。v0.x 对应源码和测试无可提取的 HTTP 维护门禁。

## 行为与 Rust 适配

- HTTP 全局中间件位于 CORS 后，复用请求匹配、异常渲染、重定向和 Cookie。默认响应为 503，明确期望 JSON 的请求沿用异常渲染。浏览器可以接收定向跳转或预渲染页面。维护状态读取消失时放行。
- `down` 原子更新维护负载，`up` 幂等恢复访问。应用内维护事件只在对应命令操作成功后分发。`down` 和 HTTP 共用一次性的应用级中间件配置，负载记录排除路径。
- `--retry` 支持秒数、标准日期、应用时区的本地日期、Unix 时间戳，以及常用的相对时间写法。Carbon 的其余自然语言解析保留为差异。签名绕过 Cookie 使用 Laravel 的名称、HMAC-SHA256 与 12 小时有效期。无可用会话时跳转首页。
- `--render` 提前渲染应用命名视图。当前 HTTP 进程在应用启动成功后监听。Laravel 在完整框架启动之前提供预渲染内容的路径，以及内置 `errors::503` 视图入口，仍需独立验收。调度器的维护门禁和 `evenInMaintenanceMode` 随调度交付核对。

## 验收

- 实际 Artisan 与 HTTP 验证默认 503、失败渲染保留原状态、预渲染快照、状态码、Retry-After、Refresh、JSON、HEAD、签名 Cookie、无效与轮换后的 Cookie、重复 down／up 和恢复访问。
- 文件驱动验证独立应用根隔离，真实 Redis 8 验证两个 HTTP 进程共享状态。框架集成验证排除路径、状态负载、事件次数与应用隔离。
- Rust 1.94.0 格式、全工作区严格 Clippy 和锁定依赖全量测试通过。全量 582 项通过、7 项忽略，包含 SQLite、MySQL 8.4 与 Redis 8 进程验收。首次运行时会话测试共用 MySQL 表，后续表单测试缺少表。为其单独建库和准备会话表后，两项真实 MySQL 表单验收均通过，另按基线配置重跑全量。
- 三语示例的 Artisan wrapper、状态与恢复均实测通过。文档构建、三语双部署路径导航及维护小节浏览器检查共 18 项通过。

[English](../../../../docs/en/getting-started/configuration.md#maintenance-mode) · [简体中文](../../../../docs/zh-Hans/getting-started/configuration.md#maintenance-mode) · [繁體中文](../../../../docs/zh-Hant/getting-started/configuration.md#maintenance-mode)
