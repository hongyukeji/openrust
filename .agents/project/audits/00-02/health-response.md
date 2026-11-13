# 00-02 补验 · 健康端点响应

2026-11-13，macOS arm64，Rust 1.94.0。

`/up` 按 Laravel Framework v13.40.0 分发 `DiagnosingHealth`，根据同步监听器结果及请求期望的内容类型返回响应。复用旧版 `v0.x`（`6d1a8192`）的事件和异常处理契约。 [Laravel HealthCheck](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/HealthCheck.php) · [Laravel 健康页](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/resources/health-up.blade.php)

- 健康时返回 200，失败时报告异常并返回 500，JSON 状态字段分别为 `up` 和 `down`。调试模式下诊断失败走常规异常处理。HTML 标题转义应用名称，正文显示状态、状态指示灯和请求到达后的渲染耗时。样式随二进制分发，耗时由 Rust 单调时钟测量。HEAD 保留响应状态和头部，正文为空。
- 本次核对响应和诊断。OpenRust 当前维护中间件仍可拦截 `/up` 并返回 503。Laravel 将配置的健康路径自动加入维护模式例外。 [Laravel 路由装配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/ApplicationBuilder.php)
- 锁定离线执行框架 `health_check`、`api_response` 及应用 `rebinding_lifecycle`、`maintenance_http`，共 10 项通过。真实 TCP 回归覆盖健康、生产诊断失败、调试异常、报告回调、HTML 转义和 HEAD。健康专项 Clippy 启用 `-D warnings` 通过。
- 使用真实 `main` 进程发送 HTML GET、JSON GET、HTML HEAD，均返回 200。正文分别为带配置应用名与渲染耗时的页面、`{"status":"up"}` 和空正文。SIGINT 与独立 `artisan inspire` 均退出 0。
- 三语 Routing 文档同步。VitePress 构建通过，既有包体积提示仍出现。浏览器在 `/`、`/docs/` 下的三语桌面与移动端共 12 项通过。
