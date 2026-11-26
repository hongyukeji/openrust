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

Register conventional routes with `Route::resource("photos", controller)` or `Route::api_resource("photos", controller)`. Create the `ResourceController` descriptor with typed method pointers such as `.action(ResourceAction::Index, PhotosController::index)`. Supply each selected action. The resource route names follow `photos.index` and `photos.show`, and the collection and member URLs follow conventions. API resources omit the create and edit form routes by default. An explicit `only` can select those actions.

Use `only` and `except` to select actions, `names` to override route names, and `parameters` to override wildcard names. Dot-separated resources such as `albums.photos` register nested paths. `shallow` keeps collection routes nested and exposes member routes under `photos/{photo}`. `scoped` sets route binding fields. Use the descriptor's `configure` method to attach middleware or typed bindings to individual routes.

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

### Singleton Resources

Register one instance at a fixed URL with `Route::singleton("profile", controller)`. The default actions are show, edit, and update. Use `destroyable` to add deletion or `creatable` to add create, store, and destroy. `Route::api_singleton` selects show and update by default, plus store and destroy when enabled. An explicit `only` can select a supported lifecycle action.

A nested singleton such as `albums.thumbnail` uses the parent parameter and a fixed child path. With `shallow`, show and store stay nested, while edit, update, and destroy use the child path alone.
