# URL Generation

## Named route URLs

Use `URL::route` with a route name and ordered parameters. Pass `true` for an absolute URL or `false` for a path. A `RouteParameter` can be named or positional. Parameters left after substituting the route pattern become the query string.

Use `RouteParameter::model("article", &article)?` or `RouteParameter::positional_model(&article)?` to pass an Eloquent model. A placeholder such as `{article:slug}` reads `slug` through Eloquent, including its accessor or cast. A placeholder without a binding field uses the model’s route key. URL generation borrows the model and reads only the selected field, without querying the database. A missing required field returns `UrlError::MissingParameter`. A failed accessor or cast returns `UrlError::ModelAttribute`.

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::URL,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    URL::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## Signed URLs

Use `URL::signedRoute` for a persistent link or `URL::temporarySignedRoute` with a `SystemTime` deadline. Attach `signed` or `signed:relative` to the destination route to match the signing mode. A modified or expired link receives HTTP 403. Signing uses `APP_KEY` as configured. `APP_PREVIOUS_KEYS` allows existing links to survive key rotation.

## Request and asset URLs

Inside HTTP handlers the generator uses the request scheme and authority. CLI code in the application scope uses `APP_URL`. `URL::asset` honors `ASSET_URL`. Use `URL::to`, `URL::current` or `URL::query` for ordinary URLs.

`URL::current()` returns the current URL without the query. `URL::full()` includes the original request path and query string. `URL::previous(request, fallback)` resolves a same-origin Referer, the previous session URL or the fallback. `URL::previousPath(request, fallback)` returns only the path, with the application's base path and trailing slash removed. On a web route, `request.session()?.previousRoute()` returns the last recorded route name. `previousUri()` on the same session returns a parsed `openrust::support::Uri` when a previous URL exists.

## Request-wide URL defaults {#default-values}

Call `URL::defaults` before generating links, typically in route middleware. Successive calls merge the defaults for that request. A later request starts with its own defaults. Explicit named and positional parameters can override defaults. Use a key such as `"article:slug"` when the route uses a custom binding field. Defaults also apply to action URLs, signed links and route redirects. To set defaults for a single generator in Artisan, use `URL::generator()?.withDefaults(...)` and import `openrust::contracts::routing::UrlGenerator` to call its methods.

```rust
use openrust::{http::Request, routing::UrlResult, support::facades::URL};

fn articles_url(request: &mut Request, locale: &str) -> UrlResult<String> {
    URL::defaults(request, &[("locale", locale)]);
    URL::route("articles.index", &[], true)
}
```

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
