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

## 最佳化視圖

Blade 範本在首次使用時編譯。來源檔案變更後，OpenRust 會在 `view.compiled` 重新產生編譯成品，預設目錄為 `storage/framework/views`。可以使用 Artisan 預先編譯視圖或清除編譯成品。

```sh
./artisan view:cache
./artisan view:clear
```
