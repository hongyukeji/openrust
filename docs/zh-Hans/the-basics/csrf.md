# CSRF 保护

## 防止 CSRF 请求

配置会话驱动后，默认 `web` 组在路由模型绑定前运行 `PreventRequestForgery`。GET、HEAD 和 OPTIONS 视为读取请求。其他请求可通过 `Sec-Fetch-Site: same-origin` 直接通过验证。浏览器未提供可接受的来源信息时，中间件回退到当前会话令牌校验。

可通过表单字段 `_token`、请求头 `X-CSRF-TOKEN`，或将解码后的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 请求头传入令牌。令牌缺失或不匹配时返回 HTTP 419。JavaScript 可以读取加密的 `XSRF-TOKEN` Cookie。

## 排除 URI

在 `withMiddleware` 中用 `preventRequestForgery` 排除第三方 webhook 等路径。第二个参数开启仅来源模式，来源不匹配时返回 HTTP 403，并停止写入 XSRF Cookie。第三个参数允许同站来源。

```rust
use openrust::foundation::configuration::Middleware;

fn configure(middleware: &mut Middleware) {
    middleware.preventRequestForgery(["hooks/*"], false, false);
}
```
