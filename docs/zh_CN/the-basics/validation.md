# 验证

## 手动创建验证器

用 `Validator::make` 创建验证器，再调用 `validate().await`。返回值包含通过验证的字段。用点路径和 `*` 选择嵌套元素。用 `nullable` 允许显式 null，用 `sometimes` 在字段存在时才运行规则。

```rust
use openrust::{
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Data, RuleSet},
};

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap_or_default())?;
        let rules = RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users.*.email", "required|string"),
        ])?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}
```

## 表单请求验证

为端点实现 `FormRequest`。`validateResolved` 依次执行 `prepareForValidation`、`authorize`、规则验证和 `passedValidation`。授权拒绝时返回 HTTP 403。

```rust
use openrust::{
    foundation::http::FormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{RuleSet, ValidationResult},
};

struct StoreUser;

impl FormRequest for StoreUser {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("name", "required|string")])
    }
}

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = StoreUser.validateResolved(request).await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}
```

## 验证错误响应格式

请求期望 JSON 时，验证失败返回 HTTP 422，正文包含 `message` 和按字段索引的 `errors`。展开后的字段用点路径表示，例如 `users.0.email`。可在当前语言的 `lang/<locale>/validation.json` 中覆盖规则消息和字段名称。

## 显示验证错误

启用会话的 Web 路由验证失败时，会重定向回表单，并把错误和提交的输入闪存到下一次请求。Blade 视图通过 `$errors` 取得当前请求的错误包。可用 `request.old(Some("profile.name"))` 回填字段。

### 命名错误包

在验证器上调用 `validateWithBag("profile")`，或覆盖 `FormRequest::errorBag`，即可指定错误包。在 Blade 中，`$errors->profile->name` 选择该包内 name 字段的消息。

### 排除敏感输入

密码字段会从旧输入中排除。可在 `withExceptions` 中用 `dontFlash` 注册额外路径。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dontFlash(["profile.secret"]);
}
```
