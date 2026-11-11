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
