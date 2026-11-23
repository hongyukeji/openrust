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

## Resource Controllers

Register conventional routes with `Route::resource("photos", controller)` or `Route::apiResource("photos", controller)`. Create the `ResourceController` descriptor with typed method pointers such as `.action(ResourceAction::Index, PhotosController::index)`. Supply each selected action. The resource route names follow `photos.index` and `photos.show`, and the collection and member URLs follow conventions. API resources omit the create and edit form routes by default. An explicit `only` can select those actions.

Use `only` and `except` to select actions, `names` to override route names, and `parameters` to override wildcard names. Dot-separated resources such as `albums.photos` register nested paths. `shallow` keeps collection routes nested and exposes member routes under `photos/{photo}`. `scoped` sets route binding fields. Use the descriptor's `configure` method to attach middleware or typed bindings to individual routes.

### Singleton Resources

Register one instance at a fixed URL with `Route::singleton("profile", controller)`. The default actions are show, edit, and update. Use `destroyable` to add deletion or `creatable` to add create, store, and destroy. `Route::apiSingleton` selects show and update by default, plus store and destroy when enabled. An explicit `only` can select a supported lifecycle action.

A nested singleton such as `albums.thumbnail` uses the parent parameter and a fixed child path. With `shallow`, show and store stay nested, while edit, update, and destroy use the child path alone.
