# CSRF Protection

## Preventing CSRF Requests

With a configured session driver, the default `web` group runs `PreventRequestForgery` before route model binding. GET, HEAD and OPTIONS are read requests. For other methods, `Sec-Fetch-Site: same-origin` is accepted. When browser fetch metadata is unavailable or does not match, the middleware checks the current session token.

Use the `_token` form field, the `X-CSRF-TOKEN` header, or the decoded value of the `XSRF-TOKEN` cookie in the `X-XSRF-TOKEN` header. A missing or mismatched token returns HTTP 419. The encrypted `XSRF-TOKEN` cookie is readable by JavaScript.

## Excluding URIs

Use `preventRequestForgery` in `withMiddleware` to exclude paths such as a third-party webhook. The second argument selects origin-only mode, which rejects origin mismatches with HTTP 403 and suppresses the XSRF cookie. The third argument permits same-site fetch metadata.

```rust
use openrust::foundation::configuration::Middleware;

fn configure(middleware: &mut Middleware) {
    middleware.preventRequestForgery(["hooks/*"], false, false);
}
```
