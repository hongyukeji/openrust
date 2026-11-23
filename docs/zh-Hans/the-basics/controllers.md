# 控制器

## 定义与注册控制器

在应用服务提供者中绑定控制器。将类型化方法交给 `ControllerDispatcher::new`，再把分发器传给路由。处理请求时，分发器从当前应用容器解析控制器。

下面的示例绑定 `TeamController`，并注册一个读取嵌套输入的控制器方法。

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

将 `Box::new(TeamProvider)` 加入应用服务提供者，并将 `register` 设为 Web 路由注册回调。

## 资源控制器

用 `Route::resource("photos", controller)` 或 `Route::apiResource("photos", controller)` 注册约定路由。创建 `ResourceController` 时，用 `.action(ResourceAction::Index, PhotosController::index)` 这样的类型化方法指针提供所选动作。路由名采用 `photos.index`、`photos.show` 等框架约定，集合和成员的 URL 同样遵循该约定。API 资源默认省略创建和编辑表单路由，也可用 `only` 显式选择这些动作。

用 `only`、`except` 选择动作，用 `names` 覆盖路由名，用 `parameters` 覆盖参数名。`albums.photos` 这样的点号资源会生成嵌套路径。`shallow` 保留嵌套集合路径，并将成员路由注册到 `photos/{photo}`。`scoped` 设置绑定字段，`configure` 可为单条路由接入中间件或类型化绑定。

### 单例资源

用 `Route::singleton("profile", controller)` 在固定 URL 上注册单个资源，默认动作是查看、编辑和更新。`destroyable` 增加删除，`creatable` 增加创建表单、保存和删除。`Route::apiSingleton` 默认选择查看与更新，并在启用相应选项时加入保存和删除。也可用 `only` 显式选择已启用的动作。

`albums.thumbnail` 这样的嵌套单例使用父资源参数和固定的子路径。启用 `shallow` 后，查看与保存仍使用嵌套路径，编辑、更新和删除使用独立的子路径。
