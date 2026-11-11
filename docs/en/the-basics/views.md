# Views

## Creating and Rendering Views

Store views under `resources/views`. The `view` helper returns a named view from a route. For example, `view("admin.profile")` looks for `admin/profile.blade.html` and `admin/profile.html` in each directory configured in `view.paths`, in order.

## Passing Data to Views

Pass data with `with`. The [Blade template](./blade.md) can read this value as `$name`.

```rust
use openrust::{routing::RoutingError, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::get("/hello", || view("greeting").with("name", "James"))
}
```

## Sharing Data With All Views

Call `View::share` from your application's service provider to set a default for every view in that application. Data passed to a view takes precedence over shared defaults.

## View Composers

Register a composer in the service provider's `boot` method. It runs immediately before the matching view renders. The name can contain a `*` wildcard. Use `View::creator` when the callback should run as the view is created.

```rust
use openrust::{support::facades::View, view::ViewResult};

fn configure_views() -> ViewResult<()> {
    View::share("site", "OpenRust")?;
    View::composer("dashboard", |view| {
        view.data_mut().insert("title".into(), "Dashboard".into());
        Ok(())
    })
}
```

## Optimizing Views

Blade templates compile on first use. When the source changes, OpenRust regenerates the compiled artifact in `view.compiled`, which defaults to `storage/framework/views`. To precompile views or clear the generated artifacts, run:

```sh
./artisan view:cache
./artisan view:clear
```
