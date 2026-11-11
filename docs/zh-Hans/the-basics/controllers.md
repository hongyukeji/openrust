# 控制器

## 定义与注册控制器

在应用服务提供者中绑定控制器。将类型化方法交给 `ControllerDispatcher::new`，再把分发器传给路由。处理请求时，分发器从当前应用容器解析控制器。

下面的示例绑定 `TeamController`，并注册一个读取嵌套输入的控制器方法。

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

将 `Box::new(TeamProvider)` 加入应用服务提供者，并将 `register` 设为 Web 路由注册回调。
