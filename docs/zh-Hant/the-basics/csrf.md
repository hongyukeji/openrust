# CSRF 防護

## 防止 CSRF 請求

設定工作階段驅動程式後，預設 `web` 群組在路由模型繫結前執行 `PreventRequestForgery`。GET、HEAD 和 OPTIONS 視為讀取請求。其他請求可透過 `Sec-Fetch-Site: same-origin` 直接通過驗證。瀏覽器未提供可接受的來源資訊時，中介軟體回退到目前工作階段權杖驗證。

可透過表單欄位 `_token`、請求標頭 `X-CSRF-TOKEN`，或將解碼後的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 請求標頭傳入權杖。權杖缺少或不相符時回傳 HTTP 419。JavaScript 可以讀取加密的 `XSRF-TOKEN` Cookie。

## 排除 URI

在 `withMiddleware` 中用 `preventRequestForgery` 排除第三方 webhook 等路徑。第二個參數啟用僅來源模式，來源不相符時回傳 HTTP 403，並停止寫入 XSRF Cookie。第三個參數允許同站來源。

```rust
use openrust::foundation::configuration::Middleware;

fn configure(middleware: &mut Middleware) {
    middleware.preventRequestForgery(["hooks/*"], false, false);
}
```
