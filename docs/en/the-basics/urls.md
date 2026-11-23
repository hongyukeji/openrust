# URL Generation

## Named route URLs

Use `Url::route` with a route name and ordered parameters. Pass `true` for an absolute URL or `false` for a path. A `RouteParameter` can be named or positional. Parameters left after substituting the route pattern become the query string.

Use `RouteParameter::model("article", &article)?` or `RouteParameter::positional_model(&article)?` to pass an Eloquent model. A placeholder such as `{article:slug}` reads `slug` through Eloquent, including its accessor or cast. A placeholder without a binding field uses the model’s route key. URL generation borrows the model and reads only the selected field, without querying the database. A missing required field returns `UrlError::MissingParameter`. A failed accessor or cast returns `UrlError::ModelAttribute`.

```rust
use openrust::{
    database::eloquent::Model,
    routing::{RouteParameter, UrlResult},
    support::facades::Url,
};

fn article_link(article: &impl Model) -> UrlResult<String> {
    Url::route("article", &[RouteParameter::model("article", article)?], true)
}
```

## Signed URLs

Use `Url::signed_route` for a persistent link or `Url::temporary_signed_route` with a `SystemTime` deadline. Attach `signed` or `signed:relative` to the destination route to match the signing mode. A modified or expired link receives HTTP 403. Signing uses `APP_KEY` as configured. `APP_PREVIOUS_KEYS` allows existing links to survive key rotation.

### Verifying signatures on the request {#validating-signed-route-requests}

Verify a signed link directly on the request instance. Pass `true` for an absolute signature, or use `has_valid_relative_signature()` for a relative signature. Rust requires the absolute flag explicitly.

```rust
use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};

fn unsubscribe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if !request.has_valid_signature(true)? {
            return Ok(Response::empty(StatusCode::UNAUTHORIZED));
        }
        Response::json(StatusCode::OK, request.query("email")?)
    })
}
```

Use `has_valid_signature_while_ignoring(&["page", "order"], true)` or `has_valid_relative_signature_while_ignoring(&["page", "order"])` to permit changes to selected query keys. Ignored values are not protected. A signature covers the URL, not JSON or form input: read signed values with `request.query(...)`, even for GET requests. `input(...)` may return an unsigned body value instead.

## Request and asset URLs

HTTP link generation uses an explicit `APP_URL` first, then the current request scheme and authority, then the listener-derived `app.url`. Artisan uses `app.url` directly. See [configuration](../getting-started/configuration.md) for the listener fallback. `Url::asset` honors `ASSET_URL`. Use `Url::to`, `Url::current` or `Url::query` for ordinary URLs.

`Url::current()` returns the current URL without the query. `Url::full()` includes the original request path and query string. `Url::previous(request, fallback)` resolves a same-origin Referer, the previous session URL or the fallback. `Url::previous_path(request, fallback)` returns only the path, with the application's base path and trailing slash removed. On a web route, `request.session()?.previous_route()` returns the last recorded route name. `previous_uri()` on the same session returns a parsed `openrust::support::Uri` when a previous URL exists.

## Request-wide URL defaults {#default-values}

Call `Url::defaults` before generating links, typically in route middleware. Successive calls merge the defaults for that request. A later request starts with its own defaults. Explicit named and positional parameters can override defaults. Use a key such as `"article:slug"` when the route uses a custom binding field. Defaults also apply to action URLs, signed links and route redirects. To set defaults for a single generator in Artisan, use `Url::generator()?.with_defaults(...)` and import `openrust::contracts::routing::UrlGenerator` to call its methods.

```rust
use openrust::{http::Request, routing::UrlResult, support::facades::Url};

fn articles_url(request: &mut Request, locale: &str) -> UrlResult<String> {
    Url::defaults(request, &[("locale", locale)]);
    Url::route("articles.index", &[], true)
}
```

## Controller action URLs {#urls-for-controller-actions}

Set a stable action identifier on the route that registers the Rust handler. Use `Url::action` to generate a URL and `Redirect::action` to redirect. The same parameter rules apply as for named routes.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, RoutingError},
    support::facades::Url,
};

const SHOW: &str = "ArticleController@show";

fn show(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/articles/{article}", show)?.set_action(SHOW)?;
    Ok(())
}

fn article_url() -> Result<String, openrust::routing::UrlError> {
    Url::action(SHOW, &[RouteParameter::named("article", "42")], true)
}
```

Domain routes take precedence. Among active routes of the same kind, the first registration wins. The identifier is explicit because Rust registers compiled handlers.
## Generation Policies

Clone the request's `UrlGenerator` before applying `force_scheme`, `force_https`, `force_root_url`, `format_host_using`, or `format_path_using`. These methods take mutable access to that generator; they do not mutate another request's policies. Pass `None` to clear forced scheme/root settings. Formatters affect generated paths and named routes, including signed URLs. Verify signatures against the same absolute/relative mode used to generate them. Configure trusted proxies separately so request-derived origins use the intended forwarded host and scheme.


## Resource origins and defaults

Use `UrlGenerator::use_origin(Some(root))` to override the origin for generated URLs and pass `None` to restore the request or configured origin. `use_asset_origin(Some(root))` overrides the asset origin; `None` clears the asset origin, including the configured CDN. These changes belong to that generator instance and its clones. `secure_asset(path)` requests HTTPS while retaining an explicitly configured asset origin. `asset_from(root, path, secure)` uses a custom root with the selected request or forced scheme. `get_default_parameters()` reads the generator’s named defaults. The `Url` facade also exposes `secure_asset`, `asset_from`, and `get_default_parameters`.
