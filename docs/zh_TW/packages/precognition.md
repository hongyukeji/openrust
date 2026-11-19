# Precognition

Precognition 在最終提交前執行表單請求的授權與驗證。成功回傳空 `204` 和 `Precognition-Success: true`；JSON 驗證失敗回傳 `422`，本文使用一般 `message` 與 `errors` 欄位。

## 啟用 Precognition

為需要預驗證的路由加入內建 `precognitive` 中介軟體。使用注入式 `ValidatedFormRequest` 或 `ControllerDispatcher::withFormRequest`，讓驗證在控制器動作前執行。

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

`StoreUser` 實作 [FormRequest](../the-basics/validation.md)。只有中介軟體確認精確請求標頭 `Precognition: true` 後，請求才進入預驗證模式。`Request::isAttemptingPrecognition()` 檢查請求標頭，`Request::isPrecognitive()` 檢查伺服器確認狀態。未啟用中介軟體的路由即使收到該標頭，仍正常執行並儲存 Session。

## 選擇驗證規則

透過 `Precognition-Validate-Only: name,email` 指定欄位。萬用字元比對展開後的屬性：`users.*.email` 比對 `users.0.email`，`users.*` 不比對額外的 `.email` 層級。逗號分隔後不移除空格，空請求標頭不選擇任何規則，未傳送該標頭時驗證全部規則。

對嘗試預驗證的請求，中介軟體加入 `Precognition: true` 回應標頭；所有回應（包括一般請求、驗證失敗和授權失敗）均向 `Vary` 附加 `Precognition`。其他中介軟體提前回傳時保留原狀態，不會被改成驗證成功。

## 請求生命週期與副作用

中介軟體、控制器建構、表單建構、輸入準備與授權仍會執行。注入式表單驗證成功後，在動作執行前短路。存在 `Precognition-Validate-Only` 時，成功掛鉤還會跳過 `passedValidation`；未傳送該標頭時，先執行 `passedValidation` 再停止分派。避免在這些掛鉤和依賴工廠中進行非預期寫入。

確認的預驗證請求正常啟動並讀取 Session、回傳 Cookie，但不儲存 Session，也不更新上一頁 URL。因此快閃資料被保留，中介軟體的一般 Session 修改不會持久化。Precognition 不回滾中介軟體、依賴建構、自訂驗證規則、外部服務或明確 Session 操作產生的寫入。

## 檔案與動態規則

伺服器按一般[檔案規則](../the-basics/validation.md)驗證實際上傳的檔案。前端 helper 預設省略檔案，需要即時上傳驗證時呼叫 `validateFiles()`。用戶端省略必填檔案後，除非調整規則，否則仍會觸發 `required`。

覆寫 `FormRequest::validationRules(&self, request: &Request)`，根據 `request.isPrecognitive()` 選擇規則。預設實作委託 `rules()`。例如只在最終提交時要求密碼或檔案，同時保留已上傳檔案的類型和大小檢查。

## JavaScript helper

官方 `laravel-precognition` 核心 helper `2.0.0` 已通過 Chromium 與真實 OpenRust HTTP 路由驗收：

```js
import { client } from 'laravel-precognition';

await client.post('/api/users', { name: 'Ada' }, {
    only: ['name'],
    onPrecognitionSuccess: () => { /* valid */ },
    onValidationError: response => { console.log(response.data.errors); },
});

await client.post('/api/users', values, { precognitive: false });
```

最終提交通常執行動作。Web 路由保留 Session 與 CSRF 保護，提供一般 CSRF Cookie 或權杖；API 路由繼續遵循既有認證與授權要求。另見[中介軟體](../the-basics/middleware.md)與[驗證](../the-basics/validation.md)。
