# 驗證

## 手動建立驗證器

用 `Validator::make` 建立驗證器，再呼叫 `validate().await`。回傳值包含通過驗證的欄位。用點路徑和 `*` 選擇巢狀元素。用 `nullable` 允許明確的 null，用 `sometimes` 在欄位存在時才執行規則。

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

## 表單請求驗證

為端點實作 `FormRequest`。`validateResolved` 依序執行 `prepareForValidation`、`authorize`、規則驗證和 `passedValidation`。授權遭拒時回傳 HTTP 403。

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

## 驗證錯誤回應格式

請求預期 JSON 時，驗證失敗回傳 HTTP 422，本文包含 `message` 和依欄位索引的 `errors`。展開後的欄位用點路徑表示，例如 `users.0.email`。可在目前語言的 `lang/<locale>/validation.json` 中覆寫規則訊息和欄位名稱。
