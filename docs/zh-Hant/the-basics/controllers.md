# 控制器

## 定義與註冊控制器

在應用程式的服務提供者中綁定控制器。將具型別的方法交給 `ControllerDispatcher::new`，再把分派器傳給路由。處理請求時，分派器從目前應用程式的容器解析控制器。

以下範例綁定 `TeamController`，並註冊一個讀取巢狀輸入的控制器方法。

```rust
use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{ControllerDispatcher, RoutingError},
    support::{ProviderResult, ServiceProvider, facades::Route},
};

struct TeamController;

impl TeamController {
    fn show<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let team = request.input(Some("team.name")).await?;
            Response::json(StatusCode::OK, team)
        })
    }
}

struct TeamProvider;

impl ServiceProvider for TeamProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().bind::<TeamController, _>(|_| Ok(Arc::new(TeamController)))?;
            Ok(())
        })
    }
}

fn register() -> Result<(), RoutingError> {
    Route::post("/team", ControllerDispatcher::new(TeamController::show))
}
```

將 `Box::new(TeamProvider)` 加入應用程式服務提供者，並將 `register` 設為 Web 路由註冊回呼。

### 單動作控制器 {#single-action-controllers}

透過 Artisan 產生只負責一個動作的控制器。

```sh
./artisan make:controller ProvisionServer --invokable
```

產生的型別實作 `InvokableController`。用 `ControllerDispatcher::<ProvisionServer>::invokable()` 註冊路由，並在應用程式容器中繫結控制器。分派器呼叫 `invoke`，可接受實作 `Responsable` 的回應型別。

```rust
Route::post("/server", ControllerDispatcher::<ProvisionServer>::invokable())?;
```

產生的固有方法 `invoke` 承載應用程式動作。先前發布的自訂單動作範本可實作該 trait 並委派給既有方法。

## 控制器中介層 {#controller-middleware}

實作 `HasMiddleware` 可在控制器內宣告路由中介層。名稱透過目前應用程式的別名和群組解析。用 `only`、`except` 依動作篩選，也可用 `from_fn` 加入內嵌非同步中介層。

```rust
use openrust::routing::{ControllerMiddleware, HasMiddleware};

impl HasMiddleware for PhotosController {
    fn middleware() -> Vec<ControllerMiddleware> {
        vec![
            ControllerMiddleware::named("auth"),
            ControllerMiddleware::named("audit").only(&["index"]),
            ControllerMiddleware::named("verified").except(&["store"]),
            ControllerMiddleware::from_fn(|request, next| {
                Box::pin(async move { Ok(next.run(request).await) })
            }),
        ]
    }
}
```

註冊型別化分派器時指定動作名稱，Rust 用這個名稱篩選宣告。資源描述元可呼叫 `with_controller_middleware()`，框架會使用資源動作名稱。控制器中介層接在路由與路由群組之後，仍參與正常的路由排除和優先順序處理。

```rust
Route::get(
    "/photos",
    ControllerDispatcher::new(PhotosController::index).with_middleware("index"),
)?;
```

## 資源控制器

使用 `Route::resource("photos", controller)` 或 `Route::api_resource("photos", controller)` 註冊慣例路由。建立 `ResourceController` 時，用 `.action(ResourceAction::Index, PhotosController::index)` 這類型別化方法指標提供所選動作。路由名稱採用 `photos.index`、`photos.show` 等框架慣例，集合及成員的 URL 也依同樣慣例。API 資源預設省略建立和編輯表單路由，也可用 `only` 明確選取這些動作。

使用 `only`、`except` 選取動作，透過 `names` 覆寫路由名稱，透過 `parameters` 覆寫參數名稱。`albums.photos` 這類點號資源會產生巢狀路徑。`shallow` 保留巢狀集合路徑，並將成員路由註冊於 `photos/{photo}`。`scoped` 設定繫結欄位，`configure` 可為個別路由加入中介層或型別化繫結。

批次註冊時，用 `ResourceRegistration::new` 傳入每個資源名稱和型別化控制器描述元。同一批可混用不同控制器型別，框架依陣列順序註冊。

```rust
Route::resources([
    ResourceRegistration::new("photos", photos()),
    ResourceRegistration::new("posts", posts()),
])?;
```

### 部分資源路由 {#restful-partial-resource-routes}

用 `only` 選取動作，或用 `except` 排除指定動作。請為最後保留的動作提供處理器。

```rust
Route::resource(
    "partial/photos",
    controller.only(&[ResourceAction::Index, ResourceAction::Show]),
)?;
```

### 巢狀資源 {#restful-nested-resources}

用點號表示巢狀關係。專案筆記的成員路徑為 `/projects/{project}/notes/{note}`。

```rust
Route::resource("projects.notes", controller)?;
```

### 淺層巢狀 {#shallow-nesting}

當子資源識別碼足以定位成員時，用 `shallow`。集合路由保留父路徑，成員路由使用 `/comments/{comment}`。

```rust
Route::resource("photos.comments", controller.shallow())?;
```

### 命名資源路由 {#restful-naming-resource-routes}

用 `names` 設定整組路由名稱覆寫。用 `name` 更改一個動作的名稱並保留其他覆寫。

```rust
Route::resource(
    "titles/photos",
    controller
        .names(&[(ResourceAction::Create, "photos.build")])
        .name(ResourceAction::Show, "photos.exhibit"),
)?;
```

### 命名資源路由參數 {#restful-naming-resource-route-parameters}

用 `parameters` 設定整組參數別名。用 `parameter` 更改一個資源的別名並保留其他別名。

```rust
Route::resource(
    "custom/accounts.users",
    controller
        .parameters(&[("accounts", "tenant"), ("users", "admin_user")])
        .parameter("users", "operator"),
)?;
```

### 補充資源路由 {#restful-supplementing-resource-controllers}

在資源路由之前註冊補充路徑。這樣 `/photos/popular` 會進入專用動作，成員路由稍後才會嘗試把 popular 當作相片識別碼。

```rust
Route::get("/photos/popular", popular)?;
Route::resource("photos", controller)?;
```

### 限制巢狀資源的模型繫結 {#scoping-nested-resources}

用 `scoped(&[])` 確保巢狀模型隸屬於已繫結的父模型。空清單保留模型各自的預設路由鍵。請為每個參數註冊對應的模型型別。

```rust
Route::resource("accounts.posts", controller.scoped(&[]))?;
```

### 限制資源路由的模型繫結 {#restful-scoping-resource-routes}

使用參數名稱和欄位名稱組成的鍵值對覆寫繫結欄位。未指定欄位的巢狀模型繼續使用預設路由鍵，並受父模型約束。再次呼叫 `scoped` 會取代欄位清單。

```rust
Route::resource("accounts.posts.notes", controller.scoped(&[("post", "slug")]))?;
```

在模型的 `resolve_child_route_binding` 中宣告型別化子關聯，可重用 `HasMany::resolve_route_binding`。在 `configure` 中呼叫 `without_scoped_bindings()` 可單獨關閉某條路由的父子範圍。

### 自訂找不到模型時的行為 {#customizing-missing-model-behavior}

用 `missing` 處理檢視、編輯、更新或刪除路由中找不到的模型。回呼在模型繫結階段執行。清單、建立表單與儲存保留預設處理，巢狀資源找不到父模型時也一樣。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    routing::ResourceController,
    support::facades::Redirect,
};

fn with_missing_item<C>(controller: ResourceController<C>) -> ResourceController<C> {
    controller.missing(missing_item)
}

fn missing_item(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Redirect::route("items.index", &[], Default::default())?.into())
    })
}
```

### 軟刪除模型 {#soft-deleted-models}

用 `with_trashed()` 允許檢視、編輯與更新繫結軟刪除模型。用 `with_trashed_for` 指定動作，例如允許刪除動作繫結軟刪除模型。空清單採用預設動作。一般資源和 API 資源都支援這兩個入口。

```rust
use openrust::routing::{ResourceAction, ResourceController};

fn show_trashed<C>(controller: ResourceController<C>) -> ResourceController<C> {
    controller.with_trashed_for(&[ResourceAction::Show])
}
```

用 `Route::soft_deletable_resources` 為整批資源套用預設的軟刪除動作集合。各控制器描述元仍須註冊對應的模型型別。

```rust
Route::soft_deletable_resources([
    ResourceRegistration::new("archive/photos", photos()),
    ResourceRegistration::new("archive/posts", posts()),
])?;
```

### 指定資源模型 {#specifying-the-resource-model}

傳入 `--model=Photo`，產生的成員動作會從路由讀取型別化的 `Photo`。註冊資源時，請在對應路由的 `photo` 參數設定模型繫結。缺少模型時，產生器可呼叫 `make:model` 建立。

```sh
./artisan make:controller PhotoController --model=Photo --resource
```

巢狀資源使用 `--parent=Album`，產生的動作從 `album` 路由參數讀取型別化的父模型。在父 Rust 模組中宣告模型和控制器檔案。

### 產生表單請求 {#generating-form-requests}

指定模型並傳入 `--requests` 可產生 `StorePhotoRequest` 和 `UpdatePhotoRequest`。宣告兩個請求模組，並用 `ResourceController::action_with_form_request` 註冊產生的儲存與更新動作。在產生的表單請求中編輯授權及驗證規則。

```sh
./artisan make:controller PhotoFormController --model=Photo --resource --requests
```

巢狀範本繼續使用一般請求參數，並產生表單請求檔案供應用程式使用。既有請求檔案會保留，使用 `--force` 重新產生控制器時也一樣。

### API 資源路由 {#api-resource-routes}

用 `Route::api_resources` 批次註冊 API 控制器，預設省略建立與編輯表單。用 `./artisan make:controller PhotoController --api` 產生 API 控制器動作。

```rust
Route::api_resources([
    ResourceRegistration::new("api/photos", photos()),
    ResourceRegistration::new("api/posts", posts()),
])?;
```

### 單例資源

用 `Route::singleton("profile", controller)` 在固定 URL 註冊單一資源，預設動作為檢視、編輯與更新。`destroyable` 加入刪除，`creatable` 加入建立表單、儲存與刪除。`Route::api_singleton` 預設選取檢視與更新，並在啟用相應選項時加入儲存與刪除。也可用 `only` 明確選取已啟用的動作。

`albums.thumbnail` 這類巢狀單例使用父資源參數及固定的子路徑。啟用 `shallow` 後，檢視與儲存仍採用巢狀路徑，編輯、更新與刪除採用獨立的子路徑。

### 資源控制器的中介層 {#middleware-and-resource-controllers}

用 `middleware` 為所有動作設定路由中介層，用 `middleware_for` 為指定動作設定額外中介層。再次設定同一動作會取代該動作的專用清單。參數使用已註冊的中介層名稱或群組名稱。

```rust
Route::resource(
    "articles",
    controller
        .middleware(&["audit", "editor"])
        .middleware_for(&[ResourceAction::Show, ResourceAction::Update], &["log"])
        .without_middleware_for(&[ResourceAction::Show], &["editor"]),
)?;
```

用 `without_middleware_for` 從指定動作移除中介層，用 `without_middleware` 從所有動作移除中介層。一般、API 和單例資源均可使用這些方法，全域中介層仍會執行。
