# 01-07 显式 HTTP 异常

## 基线

对照固定 Laravel 的 [HTTP Exceptions](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md#http-exceptions)、[abort 助手](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/helpers.php)、[Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) 和 [Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)。参考固定 [Goravel HTTP Context](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/context.go)。旧版 v0.x（`6d1a8192`）的 HTTP 模块使用状态码构建响应，未找到对应的全局异常助手。

## 实现

`http::abort` 构建带状态、消息和响应头的异常，路由与中间件通过 `Err` 交给现有异常处理器。HTML 转义消息，JSON 保留应用显式提供的消息，意外内部错误沿用既有生产脱敏策略。显式 HTTP 异常沿用现有的默认报告过滤。Rust 使用类型化的状态码、头字段和错误返回值。本批覆盖 Laravel 接收状态码的调用形式，完整 Response 仍可从处理器直接返回。

原工作区草稿只增加固定 403 变体并重排原有分支。核对原始差异后，以通用状态异常接入替代草稿。

## 验收

- 实际 TCP HTTP 请求验证 403 的 HTML 转义和 JSON 原文、HEAD 空正文、503 JSON 的显式消息、Retry-After 及默认报告过滤。
- 三语示例与实际 503 路由的函数主体逐项核对。VitePress 构建及 18 项 Chrome 检查通过，覆盖 `/` 和 `/docs/`。

[English](../../../../docs/en/the-basics/errors.md#http-exceptions) · [简体中文](../../../../docs/zh-Hans/the-basics/errors.md#http-exceptions) · [繁體中文](../../../../docs/zh-Hant/the-basics/errors.md#http-exceptions)

Rust 1.94.0 格式、全目标严格 Clippy 和串行工作区测试通过。测试日志合计 579 项通过、6 项忽略，含独立进程夹具。启用 Redis 8.10.3 回归，MySQL 可选测试本批未开启。
