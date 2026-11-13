# 00-02 补验 · 健康端点响应

2026-11-13，macOS arm64，Rust 1.94.0。

`/up` 按 Laravel Framework v13.40.0 分发 `DiagnosingHealth`，根据同步监听器结果及请求期望的内容类型返回响应。复用旧版 `v0.x`（`6d1a8192`）的事件和异常处理契约。 [Laravel HealthCheck](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/HealthCheck.php) · [Laravel 健康页](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/resources/health-up.blade.php)

- 健康时返回 200，失败时报告异常并返回 500，JSON 状态字段分别为 `up` 和 `down`。调试模式下诊断失败走常规异常处理。HTML 标题转义应用名称，正文显示状态、状态指示灯和请求到达后的渲染耗时。样式随二进制分发，耗时由 Rust 单调时钟测量。HEAD 保留响应状态和头部，正文为空。
- 锁定离线执行框架 `health_check`、`api_response` 及应用 `rebinding_lifecycle`、`maintenance_http`，共 10 项通过。真实 TCP 回归覆盖健康、生产诊断失败、调试异常、报告回调、HTML 转义和 HEAD。健康专项 Clippy 启用 `-D warnings` 通过。
- 使用真实 `main` 进程发送 HTML GET、JSON GET、HTML HEAD，均返回 200。正文分别为带配置应用名与渲染耗时的页面、`{"status":"up"}` 和空正文。SIGINT 与独立 `artisan inspire` 均退出 0。
- 三语 Routing 文档同步。VitePress 构建通过，既有包体积提示仍出现。浏览器在 `/`、`/docs/` 下的三语桌面与移动端共 12 项通过。

## 维护模式例外

- 与 Laravel 路由装配一致，`Routing::health` 自动把配置的健康路径加入维护模式例外。配置归属当前应用，Artisan `down` 将其与显式例外写入维护状态。预渲染启动阶段也读取应用当前例外，既有状态文件没有记录健康路径时，请求仍等待内核接手。 [Laravel 路由装配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/ApplicationBuilder.php) · [Laravel DownCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/DownCommand.php)
- 真实 HTTP 验证不同应用可分别使用 `/up` 和 `/status/ready`，各自配置的健康端点在维护期间提供 HTML 和 JSON，普通路径仍进入维护处理。也验证了预渲染启动、Provider 启动失败、真实 `main` 的 Artisan `down`／`up` 和 Redis 8.10.3 共享维护状态。锁定离线测试共 13 项通过，专项严格 Clippy、工作区格式及差异检查通过。
- 三语 Routing 与 Configuration 文档更新。VitePress 构建及根路径、`/docs/` 下三语桌面与移动端共 12 项浏览器检查通过。
