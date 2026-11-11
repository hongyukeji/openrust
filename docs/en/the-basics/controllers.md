# Controllers

## Defining and Registering Controllers

Register the controller in an application service provider. Use `ControllerDispatcher::new` with a typed method, and pass the dispatcher as the route handler. The dispatcher resolves the controller from the current application's container when it handles a request.

The example binds `TeamController` in the container and registers an action that reads nested input.

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

Include `Box::new(TeamProvider)` in the application providers and use `register` as the web route callback.
