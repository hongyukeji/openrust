# HTTP 测试

## 分发请求

使用 [TestApp](getting-started.md) 配置应用，并在测试进程中经 HTTP 内核分发请求。空 GET 用 `get`，需要指定方法、请求头和正文时，将 HTTP 请求传给 `call`。

```rust
use openrust::{foundation::configuration::Routing, testing::TestApp};

#[tokio::test]
async fn health_route_responds() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| {
        builder.with_routing(Routing::default().health("/up"))
    })?;
    test.get("/up").await?.assert_ok();
    Ok(())
}
```

## 检查响应

`TestResponse` 提供状态、响应头和正文。用 `assert_status`、`assert_header`、`assert_content` 或 `assert_exact_json` 断言。正文捕获限制为 1 MiB，超时为五秒。验证连接元数据等传输行为时，启动监听器并使用 HTTP 客户端。
## 响应断言

`assert_json_path` 检查点路径，`assert_json_structure` 检查嵌套或通配结构，`assert_json_validation_errors` 检查 JSON `errors` 对象中的字段。`assert_redirect`、`assert_session_has`、`assert_download` 检查实际响应及会话状态。`assert_cookie` 在传入预期值时解密并验证框架加密的字符串 Cookie；未加密 Cookie 使用 `assert_plain_cookie`。不提供 PHP Cookie 反序列化及 PHP 断言的全部可选参数。断言失败会通过 panic 显示不匹配的值或缺少的键。

## 状态与正文断言

`assert_successful`、`assert_client_error`、`assert_server_error` 检查状态范围；`assert_created`、`assert_not_found`、`assert_unprocessable` 等具名断言检查准确状态。`assert_no_content(None)` 检查 204 及空正文；传入 `Some(StatusCode::OK)` 可指定其他状态。`assert_successful_precognition` 还要求成功响应头。`assert_header("x-probe", None)` 只检查头存在，`assert_header_missing` 检查不存在。

`assert_see`、`assert_dont_see` 接受字符串或字符串列表，并显式指定是否 HTML 转义。`assert_see_html`、`assert_dont_see_html` 搜索原始 HTML；`assert_see_in_order`、`assert_see_html_in_order` 要求值按顺序出现。`assert_see_text`、`assert_dont_see_text`、`assert_see_text_in_order` 移除标签、解码 HTML401 实体并归一空白后匹配。HTML 顺序断言也会解码实体。文本及顺序断言要求正文为有效 UTF-8。

```rust
response.assert_successful().assert_header_missing("x-error");
response.assert_see(["Ada", "Bob"], true);
response.assert_see_html("<strong>Ada</strong>");
response.assert_see_text_in_order(["Ada", "Bob"], false);
```

## JSON 断言

`assert_json(&expected, false)` 按递归子集及宽松标量比较检查 JSON；传入 `true` 要求严格类型。数组子集仍按位置匹配。`assert_exact_json` 检查完整内容，忽略关联键顺序；`assert_similar_json` 还会递归排序列表。`assert_json_count(count, key)` 在 `key=None` 时检查整个文档的项目数，`Some("data")` 检查对应点路径。

`assert_json_fragment` 在文档任意位置搜索各键值片段，`assert_json_fragments` 检查多个片段。`assert_json_missing(&fragment, false)` 要求给出的每个键值对均不存在；`assert_json_missing_exact` 在至少一个键值对不存在时通过。

`assert_json_path`、`assert_json_paths` 按点路径严格比较值和类型，缺失路径解析为 null。需要忽略列表顺序时使用 `assert_json_path_canonicalizing` 或 `assert_json_paths_canonicalizing`。批量路径预期使用 `serde_json::Map<String, Value>`。`assert_json_missing_path`、`assert_json_missing_paths` 检查路径不存在，支持通配符。`assert_json_structure` 检查必需字段；`assert_exact_json_structure` 还拒绝声明层级中的额外字段。列表项目结构可写为 `{"data":{"*":["id"]}}`。`assert_json_is_array(key)`、`assert_json_is_object(key)` 区分解码后的列表与关联结构；空对象按空列表处理。

断言使用 JSON 值及显式类型参数，不接受动态 Fluent 断言回调和模型对象。JSON 深度及路径沿用框架支持组件的现有限制。

## 会话与验证断言

`assert_session_has(key, expected)` 在 `expected=None` 时检查非 null 值，传入具体值时使用宽松标量比较。`assert_session_has_all(&bindings)` 接受字段列表或预期值对象；`assert_session_missing(key, unexpected)` 检查不存在或值不同。`assert_session_has_input`、`assert_session_missing_input` 检查闪存输入。

`assert_session_has_errors(&keys, format, error_bag)` 接受字段列表或精确消息对象。默认消息格式传 `None`，默认包传 `"default"`。`assert_session_has_errors_in(error_bag, &keys, format)` 先指定包。`assert_session_doesnt_have_errors` 拒绝指定错误；选择为空时要求整个 errors 项不存在。`assert_session_has_no_errors` 同样检查该项不存在。

`assert_valid(keys, error_bag, response_key)` 的 `keys=None` 拒绝所有错误，`Some(&["email"])` 只拒绝指定字段的错误。`assert_invalid(errors, error_bag, response_key)` 检查指定字段及消息子串；`assert_only_invalid` 拒绝额外错误字段。这些方法在 Content-Type 恰为 `application/json` 时检查 JSON，否则检查会话。常用参数为 `"default"` 和 `"errors"`。

`assert_json_validation_errors(&["email"])` 检查标准 JSON 错误字段。消息预期或其他错误路径使用 `assert_json_validation_errors_at(&errors, response_key)`；单字段使用 `assert_json_validation_error_for`；排他字段集合使用 `assert_only_json_validation_errors`；无错误检查使用 `assert_json_missing_validation_errors`。断言读取已捕获的会话快照，不消耗闪存值。不接受动态 PHP 闭包和 PHP 序列化对象。

## 配置正文捕获

每个 `TestApp` 默认使用 1 MiB 正文上限与五秒捕获超时。大响应或慢响应可通过 `with_response_capture` 覆盖，后续请求立即生效，且只影响当前测试应用。超限或超时会丢弃正文流，应用仍可处理后续请求。

```rust
use std::time::Duration;
use openrust::testing::{ResponseCaptureOptions, TestApp};

let mut test = TestApp::create(|builder| builder)?;
test.with_response_capture(ResponseCaptureOptions {
    max_bytes: 8 * 1024 * 1024,
    timeout: Duration::from_secs(30),
});
```

这些限制只属于进程内测试捕获工具。传输行为及无界流使用实际监听器和 HTTP 客户端测试。

## 重定向、流、Cookie 与视图断言

`assert_location(uri)` 通过应用 URL 生成器规范化双方地址。`assert_redirect_contains(uri)` 检查非空且区分大小写的子串；`assert_redirect_back` 检查上一页地址。使用 `assert_redirect_back_with_errors(&keys, format, error_bag)` 或 `assert_redirect_back_without_errors` 同时检查会话错误。`assert_redirect_to_route`、`assert_redirect_to_action` 接受显式路由参数。`assert_redirect_to_signed_route(name, parameters, absolute)` 检查真实签名和过期时间；`name=None` 时不比较具名路由。

`assert_header(name, value)` 比较值时忽略 ASCII 字母大小写。`assert_header_contains(name, value)` 检查非空且区分大小写的子串。

`assert_streamed`、`assert_not_streamed` 检查响应类型。`streamed_content`、`assert_streamed_content` 同时支持文件响应。`assert_streamed_json_content` 比较编码字节，包括 Unicode 与斜线转义，不比较解码后的 JSON。替换响应正文会清除原流或视图元数据。

`assert_cookie_expired`、`assert_cookie_not_expired`、`assert_cookie_missing` 检查响应 Cookie。过期判断使用当前测试时钟，Max-Age 从响应创建时计起；会话 Cookie 没有过期时间。

`assert_view_is`、`assert_view_has`、`assert_view_has_all`、`assert_view_missing`、`view_data` 读取 creator 和 composer 执行后的原始视图数据，包括共享数据，不解析最终 HTML。不接受动态回调与 PHP 对象。
