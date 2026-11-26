# Controllers

## Defining and Registering Controllers

Register the controller in an application service provider. Use `ControllerDispatcher::new` with a typed method, and pass the dispatcher as the route handler. The dispatcher resolves the controller from the current application's container when it handles a request.

The example binds `TeamController` in the container and registers an action that reads nested input.

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

Include `Box::new(TeamProvider)` in the application providers and use `register` as the web route callback.

### Single Action Controllers {#single-action-controllers}

Generate a controller dedicated to one action using Artisan.

```sh
./artisan make:controller ProvisionServer --invokable
```

The generated type implements `InvokableController`. Register it through `ControllerDispatcher::<ProvisionServer>::invokable()` and bind the controller in the application container. The dispatcher calls `invoke` and accepts any response type implementing `Responsable`.

```rust
Route::post("/server", ControllerDispatcher::<ProvisionServer>::invokable())?;
```

The generated inherent `invoke` method is the application’s action. A previously published custom invokable template can implement the trait and delegate to its existing method.

## Controller Middleware {#controller-middleware}

Implement `HasMiddleware` to declare route middleware on a controller. Named entries are resolved through the application’s aliases and groups. Use `only` and `except` to filter actions, or `from_fn` to add an inline asynchronous middleware.

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

Select the action when registering a typed dispatcher. Rust uses the supplied action name to filter the declaration. For a resource descriptor, call `with_controller_middleware()` to use the registered resource action names. The controller stack follows route and group middleware, and normal route exclusions and middleware priority still apply.

```rust
Route::get(
    "/photos",
    ControllerDispatcher::new(PhotosController::index).with_middleware("index"),
)?;
```

## Resource Controllers

Register conventional routes with `Route::resource("photos", controller)` or `Route::api_resource("photos", controller)`. Create the `ResourceController` descriptor with typed method pointers such as `.action(ResourceAction::Index, PhotosController::index)`. Supply each selected action. The resource route names follow `photos.index` and `photos.show`, and the collection and member URLs follow conventions. API resources omit the create and edit form routes by default. An explicit `only` can select those actions.

Use `only` and `except` to select actions, `names` to override route names, and `parameters` to override wildcard names. Dot-separated resources such as `albums.photos` register nested paths. `shallow` keeps collection routes nested and exposes member routes under `photos/{photo}`. `scoped` sets route binding fields. Use the descriptor's `configure` method to attach middleware or typed bindings to individual routes.

To register resource controllers together, wrap each resource name and typed controller descriptor with `ResourceRegistration::new`. The batch accepts different controller types and registers them in the order you supply.

```rust
Route::resources([
    ResourceRegistration::new("photos", photos()),
    ResourceRegistration::new("posts", posts()),
])?;
```

### Partial Resource Routes {#restful-partial-resource-routes}

Select the actions with `only`, or remove specific actions with `except`. Provide handlers for the actions that remain.

```rust
Route::resource(
    "partial/photos",
    controller.only(&[ResourceAction::Index, ResourceAction::Show]),
)?;
```

### Nested Resources {#restful-nested-resources}

Use dot notation to register nested resources. A project note can then be addressed at `/projects/{project}/notes/{note}`.

```rust
Route::resource("projects.notes", controller)?;
```

### Shallow Nesting {#shallow-nesting}

Use `shallow` when the child identifier suffices for member actions. Collection routes keep the parent path and member routes use `/comments/{comment}`.

```rust
Route::resource("photos.comments", controller.shallow())?;
```

### Naming Resource Routes {#restful-naming-resource-routes}

Use `names` to replace the route-name overrides. Use `name` to change one action’s name and keep the other overrides.

```rust
Route::resource(
    "titles/photos",
    controller
        .names(&[(ResourceAction::Create, "photos.build")])
        .name(ResourceAction::Show, "photos.exhibit"),
)?;
```

### Naming Resource Route Parameters {#restful-naming-resource-route-parameters}

Use `parameters` to replace the parameter aliases. Use `parameter` to change one resource’s alias and keep the other aliases.

```rust
Route::resource(
    "custom/accounts.users",
    controller
        .parameters(&[("accounts", "tenant"), ("users", "admin_user")])
        .parameter("users", "operator"),
)?;
```

### Supplemental Resource Routes {#restful-supplementing-resource-controllers}

Register a supplemental path before the resource routes. This lets `/photos/popular` reach the dedicated action before the member route can interpret “popular” as the photo identifier.

```rust
Route::get("/photos/popular", popular)?;
Route::resource("photos", controller)?;
```

### Scoping Nested Resources {#scoping-nested-resources}

Use `scoped(&[])` to require each nested model to belong to its bound parent. The empty list keeps the models’ default route keys. Register each model type on its parameter.

```rust
Route::resource("accounts.posts", controller.scoped(&[]))?;
```

### Scoping Resource Routes {#restful-scoping-resource-routes}

Supply key overrides as parameter and field pairs. Nested models without an override still use their default key and remain scoped to the parent. A second call to `scoped` replaces the field list.

```rust
Route::resource("accounts.posts.notes", controller.scoped(&[("post", "slug")]))?;
```

Define the typed child relationship with your model’s `resolve_child_route_binding` and an existing relation such as `HasMany::resolve_route_binding`. Use `without_scoped_bindings()` in `configure` to opt out for an individual route.

### Customizing Missing Model Behavior {#customizing-missing-model-behavior}

Use `missing` to handle an unresolved model on the show, edit, update, or destroy route. The callback runs during model binding. The index, create, and store routes keep their default handling, including when a nested resource cannot bind its parent.

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

### Soft Deleted Models {#soft-deleted-models}

Use `with_trashed()` to allow soft-deleted models on show, edit, and update. Use `with_trashed_for` to select an explicit action list, for example to include destroy. An empty list uses the default actions. Both options apply to regular and API resources.

```rust
use openrust::routing::{ResourceAction, ResourceController};

fn show_trashed<C>(controller: ResourceController<C>) -> ResourceController<C> {
    controller.with_trashed_for(&[ResourceAction::Show])
}
```

Use `Route::soft_deletable_resources` to apply the default soft-delete action set to all resources in a batch. Continue to register the model type in each controller descriptor.

```rust
Route::soft_deletable_resources([
    ResourceRegistration::new("archive/photos", photos()),
    ResourceRegistration::new("archive/posts", posts()),
])?;
```

### Specifying the Resource Model {#specifying-the-resource-model}

Pass `--model=Photo` to have the generated member actions read a typed `Photo` from the route. In your resource registration, configure model binding for the `photo` parameter on the corresponding routes. The generator can create a missing model using `make:model`.

```sh
./artisan make:controller PhotoController --model=Photo --resource
```

Use `--parent=Album` for a nested resource. The generated actions read the typed album from the `album` route parameter. Declare model and controller source files in their parent Rust modules.

### Generating Form Requests {#generating-form-requests}

Use `--requests` with a model to generate `StorePhotoRequest` and `UpdatePhotoRequest`. Declare both request modules and pass the generated store and update actions to `ResourceController::action_with_form_request`. Edit the authorization and rules in the generated form requests.

```sh
./artisan make:controller PhotoFormController --model=Photo --resource --requests
```

Nested templates keep the ordinary request argument and generate the requested FormRequest files for application use. Existing request files are preserved, including when you regenerate the controller with `--force`.

### API Resource Routes {#api-resource-routes}

Use `Route::api_resources` for a batch of API controllers. The default action set excludes create and edit form routes. Use `./artisan make:controller PhotoController --api` to generate the API controller actions.

```rust
Route::api_resources([
    ResourceRegistration::new("api/photos", photos()),
    ResourceRegistration::new("api/posts", posts()),
])?;
```

### Singleton Resources

Register one instance at a fixed URL with `Route::singleton("profile", controller)`. The default actions are show, edit, and update. Use `destroyable` to add deletion or `creatable` to add create, store, and destroy. `Route::api_singleton` selects show and update by default, plus store and destroy when enabled. An explicit `only` can select a supported lifecycle action.

A nested singleton such as `albums.thumbnail` uses the parent parameter and a fixed child path. With `shallow`, show and store stay nested, while edit, update, and destroy use the child path alone.

### Middleware and Resource Controllers {#middleware-and-resource-controllers}

Use `middleware` to set route middleware for every action. Use `middleware_for` to set additional middleware for specific actions. Calling it again for the same action replaces that action’s specific list. Pass registered middleware names or group names.

```rust
Route::resource(
    "articles",
    controller
        .middleware(&["audit", "editor"])
        .middleware_for(&[ResourceAction::Show, ResourceAction::Update], &["log"])
        .without_middleware_for(&[ResourceAction::Show], &["editor"]),
)?;
```

Use `without_middleware_for` to remove middleware from particular actions. Use `without_middleware` to remove it from every action. These methods apply to ordinary, API and singleton resources. Global middleware still runs.
