# URL 產生

## 命名路由 URL

透過 `Url::route` 傳入路由名稱與有序參數。傳 `true` 取得絕對 URL，傳 `false` 取得路徑。`RouteParameter` 支援具名參數和位置參數。替換路由預留位置後剩餘的參數會加入查詢字串。

用 `RouteParameter::model("article", &article)?` 或 `RouteParameter::positional_model(&article)?` 傳入 Eloquent 模型。`{article:slug}` 透過 Eloquent 讀取 `slug`，套用對應存取器或型別轉換。未指定綁定欄位的預留位置使用模型的路由鍵。產生 URL 時借用模型，只讀取選中的欄位，無須查詢資料庫。必填欄位缺少時回傳 `UrlError::MissingParameter`，存取器或型別轉換失敗時回傳 `UrlError::ModelAttribute`。

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::Url,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    Url::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## 簽名 URL

使用 `Url::signed_route` 產生長期連結，或使用 `Url::temporary_signed_route` 和 `SystemTime` 設定到期時間。在目的路由掛載與簽名模式對應的 `signed` 或 `signed:relative` 中介軟體。遭竄改或已到期的連結會傳回 HTTP 403。簽名依設定原樣使用 `APP_KEY`，輪替時可以透過 `APP_PREVIOUS_KEYS` 保留舊連結。

## 請求 URL 與資源 URL

HTTP 請求內產生連結時，依序使用明確設定的 `APP_URL`、目前請求的通訊協定與主機、從監聽設定推導出的 `app.url`。Artisan 直接使用 `app.url`。監聽位址的回退規則請參閱[設定](../getting-started/configuration.md)。`Url::asset` 支援 `ASSET_URL`。一般 URL 可使用 `Url::to`、`Url::current` 和 `Url::query`。

`Url::current()` 傳回移除查詢字串的目前 URL。`Url::full()` 包含原始請求路徑和查詢字串。`Url::previous(request, fallback)` 依序使用同源 Referer、上一個工作階段 URL 和備用位址。`Url::previous_path(request, fallback)` 只傳回路徑，並移除應用程式的基礎路徑和尾端斜線。在 Web 路由中，`request.session()?.previous_route()` 傳回上次記錄的路由名稱。`previous_uri()` 在有上一個 URL 時傳回解析後的 `openrust::support::Uri`。

## 請求層級的 URL 預設值 {#default-values}

在產生連結前呼叫 `Url::defaults`，通常放在路由中介層。多次呼叫會合併目前請求的預設值，後續請求各自使用獨立的預設值。明確傳入的具名和位置參數可覆寫預設值。使用自訂繫結欄位時，指定如 `"article:slug"` 的鍵。預設值也適用於動作 URL、簽章連結和路由重新導向。在 Artisan 中，可用 `Url::generator()?.with_defaults(...)` 為單一產生器設定預設值，再匯入 `openrust::contracts::routing::UrlGenerator` 呼叫產生器方法。

```rust
use openrust::{http::Request, routing::UrlResult, support::facades::Url};

fn articles_url(request: &mut Request, locale: &str) -> UrlResult<String> {
    Url::defaults(request, &[("locale", locale)]);
    Url::route("articles.index", &[], true)
}
```

## 控制器動作 URL {#urls-for-controller-actions}

註冊 Rust 處理常式時為路由設定穩定的動作識別碼。用 `Url::action` 產生 URL，用 `Redirect::action` 重新導向。參數規則與命名路由相同。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::Url,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.set_action(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    Url::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

網域路由優先，同類生效路由中取第一筆註冊。Rust 註冊編譯好的處理常式，因此明確宣告動作識別碼。
## 產生策略

複製請求的 `UrlGenerator` 後，可呼叫 `force_scheme`、`force_https`、`force_root_url`、`format_host_using`、`format_path_using`。這些方法需要該產生器的可變借用，不修改其他請求的策略。傳入 `None` 清除強制協定或根位址。格式化器作用於產生路徑和具名路由，包括簽章位址；驗證簽章時使用產生時相同的絕對或相對模式。信任代理需另行設定，使請求來源使用預期的轉送網域和協定。


## 資源來源與預設參數

透過 `UrlGenerator::use_origin(Some(root))` 覆蓋產生 URL 的來源，傳入 `None` 恢復請求或設定來源。`use_asset_origin(Some(root))` 覆蓋資源來源，`None` 清除資源來源（包括設定的 CDN）。這些修改屬於目前產生器實例及其複本。`secure_asset(path)` 要求 HTTPS，但保留明確的資源來源。`asset_from(root, path, secure)` 使用自訂根位址，通訊協定採用目前請求或強制通訊協定。`get_default_parameters()` 讀取命名預設參數。`Url` Facade 同樣提供 `secure_asset`、`asset_from` 和 `get_default_parameters`。
