# HTTP 响应

## 创建响应

用 `Response::json` 返回 JSON 数据和明确的状态码。通过 `header` 或 `withHeaders` 设置类型化 HTTP 响应头。用 `Response::stream` 从 Rust 流逐块发送正文。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。

## JSON 业务响应

需要统一业务响应格式的路由可以使用 `ApiResponse`。响应包含 `success`、`code` 和 `message`，按需附带 `data`、分页 `meta` 或字段 `errors`。`success(data)` 默认返回 HTTP 200。使用 `failure` 时传入状态码，也可以通过 `with_status` 设置。

```rust
use openrust::{
    http::{ApiResponse, PageMeta, StatusCode},
    routing::RoutingError,
    support::facades::Route,
};

fn register() -> Result<(), RoutingError> {
    Route::get("/items", || {
        ApiResponse::success([1, 2]).with_meta(PageMeta {
            page: 2,
            per_page: 2,
            total: 4,
            total_pages: 2,
        })
    })?;

    Route::get("/example-conflict", || {
        ApiResponse::failure(StatusCode::CONFLICT, "conflict", "The item already exists.")
    })
}
```

`ApiResponse::empty()` 返回省略 `data` 的成功响应。HTTP 204 响应的正文为空。请给 `failure` 传入可安全展示给客户端的消息，需要返回字段错误时使用 `with_errors`。

序列化失败交给[异常处理](errors.md)。内置异常和健康检查各自使用原有的响应格式。统一业务响应是 OpenRust 在 [Laravel JSON 响应机制](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md#json-responses)之上的扩展。
