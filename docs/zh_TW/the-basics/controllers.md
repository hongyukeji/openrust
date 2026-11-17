# 控制器

## 定義與註冊控制器

在應用程式的服務提供者中綁定控制器。將具型別的方法交給 `ControllerDispatcher::new`，再把分派器傳給路由。處理請求時，分派器從目前應用程式的容器解析控制器。

以下範例綁定 `TeamController`，並註冊一個讀取巢狀輸入的控制器方法。

```rust
use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
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
    Route::r#match(&[Method::POST], "/team", ControllerDispatcher::new(TeamController::show))
}
```

將 `Box::new(TeamProvider)` 加入應用程式服務提供者，並將 `register` 設為 Web 路由註冊回呼。

## 資源控制器

使用 `Route::resource("photos", controller)` 或 `Route::apiResource("photos", controller)` 註冊慣例路由。建立 `ResourceController` 時，用 `.action(ResourceAction::Index, PhotosController::index)` 這類型別化方法指標提供所選動作。路由名稱採用 `photos.index`、`photos.show` 等 Laravel 慣例，集合及成員的 URL 也依同樣慣例。API 資源省略建立和編輯表單路由。

使用 `only`、`except` 選取動作，透過 `names` 覆寫路由名稱，透過 `parameters` 覆寫參數名稱。`albums.photos` 這類點號資源會產生巢狀路徑。`shallow` 保留巢狀集合路徑，並將成員路由註冊於 `photos/{photo}`。`scoped` 設定繫結欄位，`configure` 可為個別路由加入中介層或型別化繫結。
