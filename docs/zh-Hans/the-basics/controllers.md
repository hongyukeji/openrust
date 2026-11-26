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

### 单动作控制器 {#single-action-controllers}

通过 Artisan 生成只负责一个动作的控制器。

```sh
./artisan make:controller ProvisionServer --invokable
```

生成的类型实现 `InvokableController`。用 `ControllerDispatcher::<ProvisionServer>::invokable()` 注册路由，并在应用容器中绑定控制器。分发器调用 `invoke`，可接受实现 `Responsable` 的响应类型。

```rust
Route::post("/server", ControllerDispatcher::<ProvisionServer>::invokable())?;
```

生成的固有方法 `invoke` 承载应用动作。以前发布过的自定义单动作模板可实现该 trait 并委托给已有方法。

## 控制器中间件 {#controller-middleware}

实现 `HasMiddleware` 可在控制器内声明路由中间件。名称通过当前应用的别名和组解析。用 `only`、`except` 按动作筛选，也可用 `from_fn` 添加内联异步中间件。

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

注册类型化分发器时指定动作名，Rust 用这个名称筛选声明。资源描述符可调用 `with_controller_middleware()`，框架会使用资源动作名。控制器中间件接在路由与路由组之后，仍参与正常的路由排除和优先级处理。

```rust
Route::get(
    "/photos",
    ControllerDispatcher::new(PhotosController::index).with_middleware("index"),
)?;
```

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

### 部分资源路由 {#restful-partial-resource-routes}

用 `only` 选择动作，或用 `except` 排除指定动作。为最终保留的动作提供处理器。

```rust
Route::resource(
    "partial/photos",
    controller.only(&[ResourceAction::Index, ResourceAction::Show]),
)?;
```

### 嵌套资源 {#restful-nested-resources}

用点号表示嵌套关系。项目笔记的成员路径为 `/projects/{project}/notes/{note}`。

```rust
Route::resource("projects.notes", controller)?;
```

### 浅层嵌套 {#shallow-nesting}

当子资源标识足以定位成员时，用 `shallow`。集合路由保留父路径，成员路由使用 `/comments/{comment}`。

```rust
Route::resource("photos.comments", controller.shallow())?;
```

### 命名资源路由 {#restful-naming-resource-routes}

用 `names` 设置整组路由名称覆盖。用 `name` 更改一个动作的名称并保留其他覆盖。

```rust
Route::resource(
    "titles/photos",
    controller
        .names(&[(ResourceAction::Create, "photos.build")])
        .name(ResourceAction::Show, "photos.exhibit"),
)?;
```

### 命名资源路由参数 {#restful-naming-resource-route-parameters}

用 `parameters` 设置整组参数别名。用 `parameter` 更改一个资源的别名并保留其他别名。

```rust
Route::resource(
    "custom/accounts.users",
    controller
        .parameters(&[("accounts", "tenant"), ("users", "admin_user")])
        .parameter("users", "operator"),
)?;
```

### 补充资源路由 {#restful-supplementing-resource-controllers}

在资源路由之前注册补充路径。这样 `/photos/popular` 会进入专用动作，成员路由稍后才会尝试把 popular 当作照片标识。

```rust
Route::get("/photos/popular", popular)?;
Route::resource("photos", controller)?;
```

### 限定嵌套资源的模型绑定 {#scoping-nested-resources}

用 `scoped(&[])` 确保嵌套模型属于已绑定的父模型。空列表保留模型各自的默认路由键。为每个参数注册对应的模型类型。

```rust
Route::resource("accounts.posts", controller.scoped(&[]))?;
```

### 限定资源路由的模型绑定 {#restful-scoping-resource-routes}

用参数名和字段名组成的键值对覆盖绑定字段。未指定字段的嵌套模型继续使用自身默认路由键，并受父模型约束。再次调用 `scoped` 会替换字段列表。

```rust
Route::resource("accounts.posts.notes", controller.scoped(&[("post", "slug")]))?;
```

在模型的 `resolve_child_route_binding` 中声明类型化子关系，可复用 `HasMany::resolve_route_binding`。在 `configure` 中调用 `without_scoped_bindings()` 可单独关闭某条路由的父子作用域。

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
