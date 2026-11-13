# URL Generation

## Named route URLs

Use `URL::route` with a route name and ordered parameters. Pass `true` for an absolute URL or `false` for a path. A `RouteParameter` can be named or positional. Parameters left after substituting the route pattern become the query string. Use `RouteParameter::model` to pass an Eloquent route key.

```rust
use openrust::{
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link() -> UrlResult<String> {
    URL::route("article", &[RouteParameter::named("article", "42")], true)
}
```

## Signed URLs

Use `URL::signedRoute` for a persistent link or `URL::temporarySignedRoute` with a `SystemTime` deadline. Attach `signed` or `signed:relative` to the destination route to match the signing mode. A modified or expired link receives HTTP 403. Signing uses `APP_KEY` as configured. `APP_PREVIOUS_KEYS` allows existing links to survive key rotation.

## Request and asset URLs

Inside HTTP handlers the generator uses the request scheme and authority. CLI code in the application scope uses `APP_URL`. `URL::asset` honors `ASSET_URL`. Use `URL::to`, `URL::current` or `URL::query` for ordinary URLs.

## Controller action URLs {#urls-for-controller-actions}

Set a stable action identifier on the route that registers the Rust handler. Use `URL::action` to generate a URL and `Redirect::action` to redirect. The same parameter rules apply as for named routes.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::URL,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.setAction(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    URL::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

Domain routes take precedence. Among active routes of the same kind, the first registration wins. The identifier is explicit because Rust registers compiled handlers.
