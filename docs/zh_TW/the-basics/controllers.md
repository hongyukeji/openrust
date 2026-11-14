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
