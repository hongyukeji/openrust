# 错误处理

## 报告和渲染异常

在 `bootstrap/app.rs` 的 `withExceptions` 闭包中注册回调。用 `report::<E>` 报告类型为 `E` 的错误，匹配范围包括错误的来源链。报告回调按注册顺序执行。返回 `ReportDecision::Continue` 继续报告，返回 `ReportDecision::Stop` 停止后续回调和默认报告。常规 HTTP 400、403、404 和 405 不报告。未被回调截停的 HTTP 500 会向 stderr 写入诊断。Artisan 报告命令执行和引导错误，并保留默认的 stderr 输出和退出状态。

用 `render::<E>` 根据错误和请求返回 HTTP 响应。返回 `None` 时继续尝试后续回调，最终回退到默认 JSON 或 HTML 渲染。用 `respond` 调整最终异常响应，自定义和默认响应都会经过它。这两个回调只用于 HTTP。

## 将异常渲染为 JSON

应用在 `bootstrap/app.rs` 中通过 `withExceptions` 声明异常策略。路径匹配 `api/*` 或请求期望 JSON 时，框架返回 JSON 异常响应。其他请求收到 HTML 错误页。

例如，访问不存在的 API 路由会返回 `404` 和包含 `message` 字段的 JSON。访问不存在的 Web 路由会返回 HTML `404`，如果请求的 `Accept` 优先选择 JSON 媒体类型，则返回 JSON。
