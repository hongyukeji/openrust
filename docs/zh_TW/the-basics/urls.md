# URL 產生

## 命名路由 URL

透過 `URL::route` 傳入路由名稱與有序參數。傳 `true` 取得絕對 URL，傳 `false` 取得路徑。`RouteParameter` 支援具名參數和位置參數。替換路由預留位置後剩餘的參數會加入查詢字串。

用 `RouteParameter::model("article", &article)?` 或 `RouteParameter::positional_model(&article)?` 傳入 Eloquent 模型。`{article:slug}` 透過 Eloquent 讀取 `slug`，套用對應存取器或型別轉換。未指定綁定欄位的預留位置使用模型的路由鍵。產生 URL 時借用模型，只讀取選中的欄位，無須查詢資料庫。必填欄位缺少時回傳 `UrlError::MissingParameter`，存取器或型別轉換失敗時回傳 `UrlError::ModelAttribute`。

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    URL::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## 簽名 URL

使用 `URL::signedRoute` 產生長期連結，或使用 `URL::temporarySignedRoute` 和 `SystemTime` 設定到期時間。在目的路由掛載與簽名模式對應的 `signed` 或 `signed:relative` 中介軟體。遭竄改或已到期的連結會傳回 HTTP 403。簽名依設定原樣使用 `APP_KEY`，輪替時可以透過 `APP_PREVIOUS_KEYS` 保留舊連結。

## 請求 URL 與資源 URL

HTTP 請求內使用目前請求的通訊協定與主機。應用程式作用域內的命令列使用 `APP_URL`。`URL::asset` 支援 `ASSET_URL`。一般 URL 可使用 `URL::to`、`URL::current` 和 `URL::query`。

## 控制器動作 URL {#urls-for-controller-actions}

註冊 Rust 處理常式時為路由設定穩定的動作識別碼。用 `URL::action` 產生 URL，用 `Redirect::action` 重新導向。參數規則與命名路由相同。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::URL,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.setAction(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    URL::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

網域路由優先，同類生效路由中取第一筆註冊。Rust 註冊編譯好的處理常式，因此明確宣告動作識別碼。
