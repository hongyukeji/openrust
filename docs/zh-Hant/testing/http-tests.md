# HTTP 測試

## 分派請求

使用 [TestApp](getting-started.md) 設定應用程式，並在測試程序中經 HTTP 核心分派請求。空 GET 使用 `get`，需要指定方法、請求標頭與內容時，將 HTTP 請求傳給 `call`。

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

## 檢查回應

`TestResponse` 提供狀態、回應標頭與內容。使用 `assertStatus`、`assertHeader`、`assertContent` 或 `assertExactJson` 斷言。內容擷取限制為 1 MiB，逾時為五秒。驗證連線中繼資料等傳輸行為時，啟動監聽器並使用 HTTP 用戶端。
## 回應斷言

`assertJsonPath` 檢查點路徑，`assertJsonStructure` 檢查巢狀或萬用字元結構，`assertJsonValidationErrors` 檢查 JSON `errors` 物件中的欄位。`assertRedirect`、`assertSessionHas`、`assertDownload` 檢查實際回應及工作階段狀態。`assertCookie` 在傳入預期值時解密並驗證框架加密的字串 Cookie；未加密 Cookie 使用 `assertPlainCookie`。不提供 PHP Cookie 反序列化及 PHP 斷言的全部可選參數。斷言失敗會透過 panic 顯示不符的值或缺少的鍵。

## 狀態與正文斷言

`assertSuccessful`、`assertClientError`、`assertServerError` 檢查狀態範圍；`assertCreated`、`assertNotFound`、`assertUnprocessable` 等具名斷言檢查準確狀態。`assertNoContent(None)` 檢查 204 及空正文；傳入 `Some(StatusCode::OK)` 可指定其他狀態。`assertSuccessfulPrecognition` 還要求成功回應標頭。`assertHeader("x-probe", None)` 只檢查標頭存在，`assertHeaderMissing` 檢查不存在。

`assertSee`、`assertDontSee` 接受字串或字串清單，並明確指定是否 HTML 轉義。`assertSeeHtml`、`assertDontSeeHtml` 搜尋原始 HTML；`assertSeeInOrder`、`assertSeeHtmlInOrder` 要求值按順序出現。`assertSeeText`、`assertDontSeeText`、`assertSeeTextInOrder` 移除標籤、解碼 HTML401 實體並正規化空白後比對。HTML 順序斷言也會解碼實體。文字及順序斷言要求正文為有效 UTF-8。

```rust
response.assertSuccessful().assertHeaderMissing("x-error");
response.assertSee(["Ada", "Bob"], true);
response.assertSeeHtml("<strong>Ada</strong>");
response.assertSeeTextInOrder(["Ada", "Bob"], false);
```

## JSON 斷言

`assertJson(&expected, false)` 按遞迴子集及寬鬆純量比較檢查 JSON；傳入 `true` 要求嚴格型別。陣列子集仍按位置比對。`assertExactJson` 檢查完整內容，忽略關聯鍵順序；`assertSimilarJson` 還會遞迴排序清單。`assertJsonCount(count, key)` 在 `key=None` 時檢查整個文件的項目數，`Some("data")` 檢查對應點路徑。

`assertJsonFragment` 在文件任意位置搜尋各鍵值片段，`assertJsonFragments` 檢查多個片段。`assertJsonMissing(&fragment, false)` 要求給出的每個鍵值對均不存在；`assertJsonMissingExact` 在至少一個鍵值對不存在時通過。

`assertJsonPath`、`assertJsonPaths` 按點路徑嚴格比較值和型別，缺失路徑解析為 null。需要忽略清單順序時使用 `assertJsonPathCanonicalizing` 或 `assertJsonPathsCanonicalizing`。批次路徑預期使用 `serde_json::Map<String, Value>`。`assertJsonMissingPath`、`assertJsonMissingPaths` 檢查路徑不存在，支援萬用字元。`assertJsonStructure` 檢查必要欄位；`assertExactJsonStructure` 還拒絕宣告層級中的額外欄位。清單項目結構可寫為 `{"data":{"*":["id"]}}`。`assertJsonIsArray(key)`、`assertJsonIsObject(key)` 區分解碼後的清單與關聯結構；空物件按空清單處理。

斷言使用 JSON 值及明確型別參數，不接受動態 Fluent 斷言回呼和模型物件。JSON 深度及路徑沿用框架支援元件的現有限制。

## 工作階段與驗證斷言

`assertSessionHas(key, expected)` 在 `expected=None` 時檢查非 null 值，傳入具體值時使用寬鬆純量比較。`assertSessionHasAll(&bindings)` 接受欄位清單或預期值物件；`assertSessionMissing(key, unexpected)` 檢查不存在或值不同。`assertSessionHasInput`、`assertSessionMissingInput` 檢查快閃輸入。

`assertSessionHasErrors(&keys, format, errorBag)` 接受欄位清單或精確訊息物件。預設訊息格式傳 `None`，預設包傳 `"default"`。`assertSessionHasErrorsIn(errorBag, &keys, format)` 先指定包。`assertSessionDoesntHaveErrors` 拒絕指定錯誤；選擇為空時要求整個 errors 項不存在。`assertSessionHasNoErrors` 同樣檢查該項不存在。

`assertValid(keys, errorBag, responseKey)` 的 `keys=None` 拒絕所有錯誤，`Some(&["email"])` 只拒絕指定欄位的錯誤。`assertInvalid(errors, errorBag, responseKey)` 檢查指定欄位及訊息子字串；`assertOnlyInvalid` 拒絕額外錯誤欄位。這些方法在 Content-Type 恰為 `application/json` 時檢查 JSON，否則檢查工作階段。常用參數為 `"default"` 和 `"errors"`。

`assertJsonValidationErrors(&["email"])` 檢查標準 JSON 錯誤欄位。訊息預期或其他錯誤路徑使用 `assertJsonValidationErrorsAt(&errors, responseKey)`；單欄位使用 `assertJsonValidationErrorFor`；排他欄位集合使用 `assertOnlyJsonValidationErrors`；無錯誤檢查使用 `assertJsonMissingValidationErrors`。斷言讀取已擷取的工作階段快照，不消耗快閃值。不接受動態 PHP 閉包和 PHP 序列化物件。

## 設定正文擷取

每個 `TestApp` 預設使用 1 MiB 正文上限與五秒擷取逾時。大回應或慢回應可透過 `withResponseCapture` 覆寫，後續請求立即生效，且只影響目前測試應用。超限或逾時會丟棄正文串流，應用仍可處理後續請求。

```rust
use std::time::Duration;
use openrust::testing::{ResponseCaptureOptions, TestApp};

let mut test = TestApp::create(|builder| builder)?;
test.withResponseCapture(ResponseCaptureOptions {
    maxBytes: 8 * 1024 * 1024,
    timeout: Duration::from_secs(30),
});
```

這些限制只屬於處理程序內測試擷取工具。傳輸行為及無界串流使用實際監聽器和 HTTP 用戶端測試。

## 重新導向、串流、Cookie 與檢視斷言

`assertLocation(uri)` 透過應用程式 URL 產生器正規化雙方位址。`assertRedirectContains(uri)` 檢查非空且區分大小寫的子字串；`assertRedirectBack` 檢查上一頁位址。使用 `assertRedirectBackWithErrors(&keys, format, errorBag)` 或 `assertRedirectBackWithoutErrors` 同時檢查工作階段錯誤。`assertRedirectToRoute`、`assertRedirectToAction` 接受明確的路由參數。`assertRedirectToSignedRoute(name, parameters, absolute)` 檢查真實簽章及到期時間；`name=None` 時不比較具名路由。

`assertHeader(name, value)` 比較值時忽略 ASCII 字母大小寫。`assertHeaderContains(name, value)` 檢查非空且區分大小寫的子字串。

`assertStreamed`、`assertNotStreamed` 檢查回應類型。`streamedContent`、`assertStreamedContent` 同時支援檔案回應。`assertStreamedJsonContent` 比較編碼位元組，包括 Unicode 與斜線跳脫，不比較解碼後的 JSON。替換回應本文會清除原串流或檢視中繼資料。

`assertCookieExpired`、`assertCookieNotExpired`、`assertCookieMissing` 檢查回應 Cookie。到期判斷使用目前測試時鐘，Max-Age 從回應建立時開始計算；工作階段 Cookie 沒有到期時間。

`assertViewIs`、`assertViewHas`、`assertViewHasAll`、`assertViewMissing`、`viewData` 讀取 creator 與 composer 執行後的原始檢視資料，包括共用資料，不解析最終 HTML。不接受動態回呼與 PHP 物件。
