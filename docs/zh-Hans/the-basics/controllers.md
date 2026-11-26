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

用 `Route::resource("photos", controller)` 或 `Route::api_resource("photos", controller)` 注册约定路由。创建 `ResourceController` 时，用 `.action(ResourceAction::Index, PhotosController::index)` 这样的类型化方法指针提供所选动作。路由名采用 `photos.index`、`photos.show` 等框架约定，集合和成员的 URL 同样遵循该约定。API 资源默认省略创建和编辑表单路由，也可用 `only` 显式选择这些动作。

用 `only`、`except` 选择动作，用 `names` 覆盖路由名，用 `parameters` 覆盖参数名。`albums.photos` 这样的点号资源会生成嵌套路径。`shallow` 保留嵌套集合路径，并将成员路由注册到 `photos/{photo}`。`scoped` 设置绑定字段，`configure` 可为单条路由接入中间件或类型化绑定。

批量注册时，用 `ResourceRegistration::new` 传入每个资源名和类型化控制器描述符。同一批可以混用不同控制器类型，框架按数组顺序注册。

```rust
Route::resources([
    ResourceRegistration::new("photos", photos()),
    ResourceRegistration::new("posts", posts()),
])?;
```

### 自定义模型未命中的行为 {#customizing-missing-model-behavior}

用 `missing` 处理查看、编辑、更新或删除路由中未找到的模型。回调在模型绑定阶段运行。列表、创建表单和保存保留默认处理，嵌套资源找不到父模型时也一样。

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

### 软删除模型 {#soft-deleted-models}

用 `with_trashed()` 允许查看、编辑和更新绑定软删除模型。用 `with_trashed_for` 指定动作，例如允许删除动作绑定软删除模型。空列表采用默认动作。普通资源和 API 资源都支持这两个入口。

```rust
use openrust::routing::{ResourceAction, ResourceController};

fn show_trashed<C>(controller: ResourceController<C>) -> ResourceController<C> {
    controller.with_trashed_for(&[ResourceAction::Show])
}
```

用 `Route::soft_deletable_resources` 为整批资源应用默认的软删除动作集合。各控制器描述符仍须注册对应的模型类型。

```rust
Route::soft_deletable_resources([
    ResourceRegistration::new("archive/photos", photos()),
    ResourceRegistration::new("archive/posts", posts()),
])?;
```

### 指定资源模型 {#specifying-the-resource-model}

传入 `--model=Photo`，生成的成员动作会从路由读取类型化的 `Photo`。注册资源时，对对应路由的 `photo` 参数配置模型绑定。缺少模型时，生成器可调用 `make:model` 创建。

```sh
./artisan make:controller PhotoController --model=Photo --resource
```

嵌套资源使用 `--parent=Album`，生成的动作从 `album` 路由参数读取类型化的父模型。在父 Rust 模块中声明模型和控制器文件。

### 生成表单请求 {#generating-form-requests}

指定模型并传入 `--requests` 可生成 `StorePhotoRequest` 和 `UpdatePhotoRequest`。声明两个请求模块，并用 `ResourceController::action_with_form_request` 注册生成的存储与更新动作。在生成的表单请求中编辑授权和校验规则。

```sh
./artisan make:controller PhotoFormController --model=Photo --resource --requests
```

嵌套模板继续使用普通请求参数，并生成表单请求文件供应用使用。已有请求文件会保留，使用 `--force` 重新生成控制器时也是如此。

### API 资源路由 {#api-resource-routes}

用 `Route::api_resources` 批量注册 API 控制器，默认省略创建和编辑表单。用 `./artisan make:controller PhotoController --api` 生成 API 控制器动作。

```rust
Route::api_resources([
    ResourceRegistration::new("api/photos", photos()),
    ResourceRegistration::new("api/posts", posts()),
])?;
```

### 单例资源

用 `Route::singleton("profile", controller)` 在固定 URL 上注册单个资源，默认动作是查看、编辑和更新。`destroyable` 增加删除，`creatable` 增加创建表单、保存和删除。`Route::api_singleton` 默认选择查看与更新，并在启用相应选项时加入保存和删除。也可用 `only` 显式选择已启用的动作。

`albums.thumbnail` 这样的嵌套单例使用父资源参数和固定的子路径。启用 `shallow` 后，查看与保存仍使用嵌套路径，编辑、更新和删除使用独立的子路径。

### 资源控制器的中间件 {#middleware-and-resource-controllers}

用 `middleware` 为所有动作设置路由中间件，用 `middleware_for` 为指定动作设置额外中间件。再次设置同一动作会替换该动作的专用列表。参数使用已经注册的中间件名或组名。

```rust
Route::resource(
    "articles",
    controller
        .middleware(&["audit", "editor"])
        .middleware_for(&[ResourceAction::Show, ResourceAction::Update], &["log"])
        .without_middleware_for(&[ResourceAction::Show], &["editor"]),
)?;
```

用 `without_middleware_for` 从指定动作移除中间件，用 `without_middleware` 从所有动作移除中间件。普通、API 和单例资源均可使用这些方法，全局中间件仍会执行。
