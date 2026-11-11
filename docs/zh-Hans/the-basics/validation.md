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
