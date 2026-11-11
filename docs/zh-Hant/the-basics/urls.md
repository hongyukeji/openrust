# URL 產生

## 命名路由 URL

透過 `URL::route` 傳入路由名稱與有序參數。傳 `true` 取得絕對 URL，傳 `false` 取得路徑。`RouteParameter` 支援具名參數和位置參數。替換路由預留位置後剩餘的參數會加入查詢字串。可使用 `RouteParameter::model` 傳入 Eloquent 路由鍵。

```rust
use openrust::{
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link() -> UrlResult<String> {
    URL::route("article", &[RouteParameter::named("article", "42")], true)
}
```

## 簽名 URL

使用 `URL::signedRoute` 產生長期連結，或使用 `URL::temporarySignedRoute` 和 `SystemTime` 設定到期時間。在目的路由掛載與簽名模式對應的 `signed` 或 `signed:relative` 中介軟體。遭竄改或已到期的連結會傳回 HTTP 403。簽名依設定原樣使用 `APP_KEY`，輪替時可以透過 `APP_PREVIOUS_KEYS` 保留舊連結。

## 請求 URL 與資源 URL

HTTP 請求內使用目前請求的通訊協定與主機。應用程式作用域內的命令列使用 `APP_URL`。`URL::asset` 支援 `ASSET_URL`。一般 URL 可使用 `URL::to`、`URL::current` 和 `URL::query`。
