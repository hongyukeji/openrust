# Precognition

Precognition 在最终提交前执行表单请求的授权与验证。成功返回空 `204` 和 `Precognition-Success: true`；JSON 验证失败返回 `422`，正文使用常规 `message` 与 `errors` 字段。

## 启用 Precognition

为需要预验证的路由添加内置 `precognitive` 中间件。使用注入式 `ValidatedFormRequest` 或 `ControllerDispatcher::withFormRequest`，让验证在控制器动作前执行。

```rust
use openrust::{
    foundation::http::ValidatedFormRequest,
    http::{HttpError, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};

async fn store(form: ValidatedFormRequest<StoreUser>) -> Result<Response, HttpError> {
    Response::json(StatusCode::OK, form.validated().all())
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().middleware_name("precognitive"), |router| {
        router.post("/users", store)?;
        Ok(())
    })
}
```

`StoreUser` 实现 [FormRequest](../the-basics/validation.md)。只有中间件确认精确请求头 `Precognition: true` 后，请求才进入预验证模式。`Request::isAttemptingPrecognition()` 检查请求头，`Request::isPrecognitive()` 检查服务端确认状态。未启用中间件的路由即使收到该头，仍正常执行并保存 Session。

## 选择验证规则

通过 `Precognition-Validate-Only: name,email` 指定字段。通配符匹配展开后的属性：`users.*.email` 匹配 `users.0.email`，`users.*` 不匹配额外的 `.email` 层级。逗号分隔后不移除空格，空请求头不选择任何规则，未发送该头时验证全部规则。

对尝试预验证的请求，中间件添加 `Precognition: true` 响应头；所有响应（包括普通请求、验证失败和授权失败）均向 `Vary` 追加 `Precognition`。其他中间件提前返回时保留原状态，不会被改成验证成功。

## 请求生命周期与副作用

中间件、控制器构造、表单构造、输入准备与授权仍会执行。注入式表单验证成功后，在动作执行前短路。存在 `Precognition-Validate-Only` 时，成功钩子还会跳过 `passedValidation`；未发送该头时，先执行 `passedValidation` 再停止分发。避免在这些钩子和依赖工厂中进行非预期写入。

确认的预验证请求正常启动并读取 Session、返回 Cookie，但不保存 Session，也不更新上一页 URL。因此闪存被保留，中间件的常规 Session 修改不会持久化。Precognition 不回滚中间件、依赖构造、自定义验证规则、外部服务或显式 Session 操作产生的写入。

## 文件与动态规则

服务端按常规[文件规则](../the-basics/validation.md)验证实际上传的文件。前端 helper 默认省略文件，需要实时上传验证时调用 `validateFiles()`。客户端省略必填文件后，除非调整规则，否则仍会触发 `required`。

重写 `FormRequest::validationRules(&self, request: &Request)`，根据 `request.isPrecognitive()` 选择规则。默认实现委托 `rules()`。例如只在最终提交时要求密码或文件，同时保留已上传文件的类型和大小检查。

## JavaScript helper

官方 `laravel-precognition` 核心 helper `2.0.0` 已通过 Chromium 与真实 OpenRust HTTP 路由验收：

```js
import { client } from 'laravel-precognition';

await client.post('/api/users', { name: 'Ada' }, {
    only: ['name'],
    onPrecognitionSuccess: () => { /* valid */ },
    onValidationError: response => { console.log(response.data.errors); },
});

await client.post('/api/users', values, { precognitive: false });
```

最终提交正常执行动作。Web 路由保留 Session 与 CSRF 保护，提供常规 CSRF Cookie 或令牌；API 路由继续遵循已有认证与授权要求。另见[中间件](../the-basics/middleware.md)与[验证](../the-basics/validation.md)。
