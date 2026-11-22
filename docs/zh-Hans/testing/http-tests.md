# HTTP 测试

## 分发请求

使用 [TestApp](getting-started.md) 配置应用，并在测试进程中经 HTTP 内核分发请求。空 GET 用 `get`，需要指定方法、请求头和正文时，将 HTTP 请求传给 `call`。

```rust
use openrust::{foundation::configuration::Routing, testing::TestApp};

#[tokio::test]
async fn health_route_responds() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| {
        builder.withRouting(Routing::default().health("/up"))
    })?;
    test.get("/up").await?.assertOk();
    Ok(())
}
```

## 检查响应

`TestResponse` 提供状态、响应头和正文。用 `assertStatus`、`assertHeader`、`assertContent` 或 `assertExactJson` 断言。正文捕获限制为 1 MiB，超时为五秒。验证连接元数据等传输行为时，启动监听器并使用 HTTP 客户端。
## 响应断言

`assertJsonPath` 检查点路径，`assertJsonStructure` 检查嵌套或通配结构，`assertJsonValidationErrors` 检查 JSON `errors` 对象中的字段。`assertRedirect`、`assertSessionHas`、`assertDownload` 检查实际响应及会话状态。`assertCookie` 在传入预期值时解密并验证框架加密的字符串 Cookie；未加密 Cookie 使用 `assertPlainCookie`。不提供 PHP Cookie 反序列化及 PHP 断言的全部可选参数。断言失败会通过 panic 显示不匹配的值或缺少的键。

## 状态与正文断言

`assertSuccessful`、`assertClientError`、`assertServerError` 检查状态范围；`assertCreated`、`assertNotFound`、`assertUnprocessable` 等具名断言检查准确状态。`assertNoContent(None)` 检查 204 及空正文；传入 `Some(StatusCode::OK)` 可指定其他状态。`assertSuccessfulPrecognition` 还要求成功响应头。`assertHeader("x-probe", None)` 只检查头存在，`assertHeaderMissing` 检查不存在。

`assertSee`、`assertDontSee` 接受字符串或字符串列表，并显式指定是否 HTML 转义。`assertSeeHtml`、`assertDontSeeHtml` 搜索原始 HTML；`assertSeeInOrder`、`assertSeeHtmlInOrder` 要求值按顺序出现。`assertSeeText`、`assertDontSeeText`、`assertSeeTextInOrder` 移除标签、解码 HTML401 实体并归一空白后匹配。HTML 顺序断言也会解码实体。文本及顺序断言要求正文为有效 UTF-8。

```rust
response.assertSuccessful().assertHeaderMissing("x-error");
response.assertSee(["Ada", "Bob"], true);
response.assertSeeHtml("<strong>Ada</strong>");
response.assertSeeTextInOrder(["Ada", "Bob"], false);
```

## JSON 断言

`assertJson(&expected, false)` 按递归子集及宽松标量比较检查 JSON；传入 `true` 要求严格类型。数组子集仍按位置匹配。`assertExactJson` 检查完整内容，忽略关联键顺序；`assertSimilarJson` 还会递归排序列表。`assertJsonCount(count, key)` 在 `key=None` 时检查整个文档的项目数，`Some("data")` 检查对应点路径。

`assertJsonFragment` 在文档任意位置搜索各键值片段，`assertJsonFragments` 检查多个片段。`assertJsonMissing(&fragment, false)` 要求给出的每个键值对均不存在；`assertJsonMissingExact` 在至少一个键值对不存在时通过。

`assertJsonPath`、`assertJsonPaths` 按点路径严格比较值和类型，缺失路径解析为 null。需要忽略列表顺序时使用 `assertJsonPathCanonicalizing` 或 `assertJsonPathsCanonicalizing`。批量路径预期使用 `serde_json::Map<String, Value>`。`assertJsonMissingPath`、`assertJsonMissingPaths` 检查路径不存在，支持通配符。`assertJsonStructure` 检查必需字段；`assertExactJsonStructure` 还拒绝声明层级中的额外字段。列表项目结构可写为 `{"data":{"*":["id"]}}`。`assertJsonIsArray(key)`、`assertJsonIsObject(key)` 区分解码后的列表与关联结构；空对象按空列表处理。

断言使用 JSON 值及显式类型参数，不接受动态 Fluent 断言回调和模型对象。JSON 深度及路径沿用框架支持组件的现有限制。

## 会话与验证断言

`assertSessionHas(key, expected)` 在 `expected=None` 时检查非 null 值，传入具体值时使用宽松标量比较。`assertSessionHasAll(&bindings)` 接受字段列表或预期值对象；`assertSessionMissing(key, unexpected)` 检查不存在或值不同。`assertSessionHasInput`、`assertSessionMissingInput` 检查闪存输入。

`assertSessionHasErrors(&keys, format, errorBag)` 接受字段列表或精确消息对象。默认消息格式传 `None`，默认包传 `"default"`。`assertSessionHasErrorsIn(errorBag, &keys, format)` 先指定包。`assertSessionDoesntHaveErrors` 拒绝指定错误；选择为空时要求整个 errors 项不存在。`assertSessionHasNoErrors` 同样检查该项不存在。

`assertValid(keys, errorBag, responseKey)` 的 `keys=None` 拒绝所有错误，`Some(&["email"])` 只拒绝指定字段的错误。`assertInvalid(errors, errorBag, responseKey)` 检查指定字段及消息子串；`assertOnlyInvalid` 拒绝额外错误字段。这些方法在 Content-Type 恰为 `application/json` 时检查 JSON，否则检查会话。常用参数为 `"default"` 和 `"errors"`。

`assertJsonValidationErrors(&["email"])` 检查标准 JSON 错误字段。消息预期或其他错误路径使用 `assertJsonValidationErrorsAt(&errors, responseKey)`；单字段使用 `assertJsonValidationErrorFor`；排他字段集合使用 `assertOnlyJsonValidationErrors`；无错误检查使用 `assertJsonMissingValidationErrors`。断言读取已捕获的会话快照，不消耗闪存值。不接受动态 PHP 闭包和 PHP 序列化对象。

## 配置正文捕获

每个 `TestApp` 默认使用 1 MiB 正文上限与五秒捕获超时。大响应或慢响应可通过 `withResponseCapture` 覆盖，后续请求立即生效，且只影响当前测试应用。超限或超时会丢弃正文流，应用仍可处理后续请求。

```rust
use std::time::Duration;
use openrust::testing::{ResponseCaptureOptions, TestApp};

let mut test = TestApp::create(|builder| builder)?;
test.withResponseCapture(ResponseCaptureOptions {
    maxBytes: 8 * 1024 * 1024,
    timeout: Duration::from_secs(30),
});
```

这些限制只属于进程内测试捕获工具。传输行为及无界流使用实际监听器和 HTTP 客户端测试。

## 重定向、流、Cookie 与视图断言

`assertLocation(uri)` 通过应用 URL 生成器规范化双方地址。`assertRedirectContains(uri)` 检查非空且区分大小写的子串；`assertRedirectBack` 检查上一页地址。使用 `assertRedirectBackWithErrors(&keys, format, errorBag)` 或 `assertRedirectBackWithoutErrors` 同时检查会话错误。`assertRedirectToRoute`、`assertRedirectToAction` 接受显式路由参数。`assertRedirectToSignedRoute(name, parameters, absolute)` 检查真实签名和过期时间；`name=None` 时不比较具名路由。

`assertHeader(name, value)` 比较值时忽略 ASCII 字母大小写。`assertHeaderContains(name, value)` 检查非空且区分大小写的子串。

`assertStreamed`、`assertNotStreamed` 检查响应类型。`streamedContent`、`assertStreamedContent` 同时支持文件响应。`assertStreamedJsonContent` 比较编码字节，包括 Unicode 与斜线转义，不比较解码后的 JSON。替换响应正文会清除原流或视图元数据。

`assertCookieExpired`、`assertCookieNotExpired`、`assertCookieMissing` 检查响应 Cookie。过期判断使用当前测试时钟，Max-Age 从响应创建时计起；会话 Cookie 没有过期时间。

`assertViewIs`、`assertViewHas`、`assertViewHasAll`、`assertViewMissing`、`viewData` 读取 creator 和 composer 执行后的原始视图数据，包括共享数据，不解析最终 HTML。不接受动态回调与 PHP 对象。
