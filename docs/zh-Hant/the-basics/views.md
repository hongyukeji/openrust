# 視圖

## 建立與呈現視圖

將視圖放在 `resources/views`。路由透過 `view` 回傳具名視圖。例如 `view("admin.profile")` 依照 `view.paths` 的目錄順序，在每個目錄尋找 `admin/profile.blade.html` 和 `admin/profile.html`。

## 傳遞資料給視圖

使用 `with` 傳遞資料。[Blade 範本](./blade.md)可透過 `$name` 讀取該值。

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::get("/hello", || view("greeting").with("name", "James"))
}
```

## 與所有視圖共用資料

在應用程式的服務提供者中呼叫 `View::share`，為該應用程式的視圖設定預設資料。傳入視圖的資料優先於共用預設值。

## 視圖組合器

在服務提供者的 `boot` 方法中註冊組合器。它會在符合的視圖呈現前執行，視圖名稱可使用 `*` 萬用字元。需要在建立視圖時執行回呼，可以使用 `View::creator`。

```rust
use openrust::{support::facades::View, view::ViewResult};

fn configure_views() -> ViewResult<()> {
    View::share("site", "OpenRust")?;
    View::composer("dashboard", |view| {
        view.data_mut().insert("title".into(), "Dashboard".into());
        Ok(())
    })
}
```

## 檢視契約 {#view-contracts}

使用 `View::contract()` 取得目前應用程式的檢視工廠，也可從容器解析 `dyn contracts::view::Factory`。檢視保留所屬工廠，更新資料後可以再次呈現。`make` 和 `file` 的檢視資料優先於合併資料。

```rust
use openrust::view::{ViewData, ViewResult};

fn render_card(factory: &dyn openrust::contracts::view::Factory) -> ViewResult<String> {
    let mut view = factory.make("contract_card", ViewData::new(), ViewData::new())?;
    view.with("title", "<card>".into());
    view.render()
}
```

使用 `file` 時，檔案須位於已設定的檢視目錄內。相對路徑依應用程式的檢視目錄順序查找。`addNamespace` 和 `replaceNamespace` 管理命名空間目錄。工廠的 creator 和 composer 支援一組檢視名稱或萬用字元模式。明確指定的檔案支援 `.blade.html`、`.html` 或 `.css`。

Engine 契約要求明確提供檢視根目錄，用以限定檔案讀取範圍。契約呈現是同步的。在非同步處理器中，可將直接呈現交給阻塞執行緒，或傳回檢視並由框架呈現。

## 最佳化視圖

Blade 範本在首次使用時編譯。來源檔案變更後，OpenRust 會在 `view.compiled` 重新產生編譯成品，預設目錄為 `storage/framework/views`。可以使用 Artisan 預先編譯視圖或清除編譯成品。

```sh
./artisan view:cache
./artisan view:clear
```
