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

### 原生視圖值 {#native-view-values}

視圖資料使用 `ViewData`，它是依插入順序排列的 `ViewValue` 映射。數值經過陣列、引入視圖、版型、匿名和類別元件及已註冊指令回呼時，保留 `INF`、`-INF`、`NAN` 和負零。既有的 `serde_json::Value` 輸入用 `.into()` 轉換；先經 JSON 編碼變成 null 的數值無法恢復。

```rust
use openrust::view::{ViewData, ViewValue};

let mut data = ViewData::new();
data.insert("number".into(), f64::INFINITY.into());
data.insert("items".into(), vec![f64::NAN, -0.0].into());
data.insert("profile".into(), serde_json::json!({"name": "James"}).into());
let value = ViewValue::Object(data);
assert!(value.to_json().is_err());
```

`ViewValue::to_json()` 和 Serde 序列化會遞迴拒絕非有限數。在 JSON 邊界處理傳回的 `Result`；一般 Blade 回顯不經 JSON 編碼，可以渲染同一個值。

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
## 條件與集合渲染

`View::renderWhen` 條件為真才渲染，`renderUnless` 相反。`renderEach(view, data, iterator, empty)` 接收鍵值對，把鍵公開為 `key`、值公開為指定迭代變數，再串接各項結果。空集合渲染空視圖，或直接傳回 `raw|` 後的文字。子視圖繼承共用資料，不自動繼承父視圖區域變數。`getShared` 傳回擁有所有權的快照。啟動階段的 `share` 值作為預設值；請求或佇列作用域內的修改只影響該作用域，顯式視圖資料覆蓋共用同名鍵。
