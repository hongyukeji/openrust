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

## 顯示驗證錯誤

啟用工作階段的 Web 路由驗證失敗時，會重新導向表單，並將錯誤及送出的輸入快閃至下一次請求。Blade 檢視透過 `$errors` 取得目前請求的錯誤包。可用 `request.old(Some("profile.name"))` 回填欄位。

### 命名錯誤包

在驗證器上呼叫 `validateWithBag("profile")`，或覆寫 `FormRequest::errorBag`，即可指定錯誤包。在 Blade 中，`$errors->profile->name` 選取該包內 name 欄位的訊息。

### 排除敏感輸入

密碼欄位會從舊輸入排除。可在 `withExceptions` 中用 `dontFlash` 註冊額外路徑。

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dontFlash(["profile.secret"]);
}
```
