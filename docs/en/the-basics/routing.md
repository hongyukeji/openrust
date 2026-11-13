# Routing

Declare web routes in `routes/web.rs`. The application registers those routes and the `/up` health check through `openrust::routing::Router` during startup. A handler that needs the request takes a mutable `Request` and returns an asynchronous HTTP result. The framework binds the application router while calling the Web route module’s `register()`. Declare a named view with `Route::get("/", || view("welcome"))`, using `openrust::support::facades::Route`.

Declare API routes in `routes/api.rs`. The application skeleton loads it under `/api` and applies the `api` middleware group. Set `ROUTING_API_PREFIX=/v1/internal` to change `routing.api_prefix`, or call `Routing::apiPrefix("v2")` in the Builder to override that configuration. Leading and trailing slashes are optional. Set `ROUTING_API_PREFIX=` or pass an empty string to mount API routes without a prefix. The `api` middleware group still applies.

In `routes/api.rs`, declare API endpoints in the parameterless `register() -> Result<(), RoutingError>` using the `Route` facade. Route paths are relative to the API prefix. Use `Route::getWith` when a GET route needs a name, model binding, or route-level middleware.

A parameter such as `{id}` is required. Add `?` to allow a trailing parameter to be omitted, as in `/profiles/{id?}/{tab?}` or `/report/{name}.{format?}`. Read the captured value with `request.route("id")`. For an omitted parameter, this returns `None` unless the route supplies a string through `defaults`. A parameter before required text or another required parameter must still be present.

Use `Route::r#where` for a regular expression, or use `where_number`, `where_alpha`, `where_alpha_numeric`, `where_uuid`, `where_ulid`, or `where_in`. Set `Router::pattern` before registering routes to apply a constraint to every subsequent route that uses that parameter name. A route-specific constraint overrides it. `where_in` joins its values as regex alternatives, so any regex metacharacters retain their meaning. OpenRust uses Rust's `regex` syntax. PCRE lookaround and backreferences are rejected during registration.

Matching uses the UTF-8 percent-decoded path and ignores the query string. Invalid UTF-8 returns HTTP 400. Constraints apply to both route selection and the method check. For example, if `/profiles/{id?}` is a GET route with a numeric ID, `POST /profiles/7` returns 405 with `Allow`, while `POST /profiles/alice` returns 404 when no other route matches that path.

`Router::get` registers both GET and HEAD. HEAD preserves the response status and headers and sends an empty body. An unknown path returns 404. A known path with an unsupported method returns 405 with an `Allow` header. When a matching path has no explicit OPTIONS route, the router returns 200 with the allowed methods. Among overlapping routes for the same method, the first registered match wins.

## Mounting a static directory

The application skeleton mounts the website directory first and the public storage disk second. See [public file configuration](../getting-started/configuration.md#public-files) for the directories and URL prefixes.

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

Relative directories are resolved from the application root. For example, `public/css/app.css` is available at `/css/app.css`. To disable the public storage mount, remove its `Route::staticDisk("public")?` call from `routes/web.rs`. The website directory mounts at `/` and the public disk defaults to `/storage`. Static mounts are checked in declaration order. If both directories contain a file for the same URL, the first match wins. With the skeleton’s order, `public/storage/avatar.png` takes precedence over `storage/app/public/avatar.png` for `/storage/avatar.png`. The disk requires no `public/storage` symbolic link. Application routes take precedence. Only unmatched GET and HEAD requests can serve a file. Missing files and directory requests return 404, and links cannot escape the mounted directory.

Static file responses include `Last-Modified` when the file has a usable modification time. Send that value in `If-Modified-Since` with GET or HEAD to revalidate the file. If it has not changed, the response is `304 Not Modified` with an empty body.

## Health Check

The `health("/up")` option in `bootstrap/app.rs` registers a health endpoint. A healthy request returns 200. The HTML page shows the application name in the title, “Application up”, and how long the request took to reach rendering. Requests expecting JSON receive `{"status":"up"}`. HEAD sends no body.

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

The endpoint dispatches `openrust::foundation::events::DiagnosingHealth`. Register synchronous listeners to check dependencies such as the database or cache. If a listener fails, the framework reports the error and returns 500 with “Application experiencing problems” or `{"status":"down"}`. In debug mode, the normal exception handler renders the failure. The health page ships with its own styles.

## Listing Your Routes

Run `route:list` to see active routes. Use `--json` for structured output. Filter by path, method, name, domain, action or middleware. Use `--except-path` with comma-separated substrings. `--sort=definition` preserves registration order, and `--reverse` reverses the result.

```sh
./artisan route:list --json --method=HEAD
```

## Route Parameters

Use `{id}` for a required parameter or `{id?}` for a trailing optional parameter. Set defaults with `defaults`. A parameter that fails a route constraint falls through to other routes or returns 404.

## Route Groups

Groups share URI prefixes, name prefixes, domains, middleware and constraints. Register specific domain routes before a host-independent route with the same path. A domain placeholder is available through `request.route`, just like a path placeholder.

The group closure receives a mutable Rust router. Use it to configure the registered route. A GET route accepts HEAD, and automatic OPTIONS reports allowed methods for a matching path and domain.

```rust
use openrust::{routing::{RouteGroup, RoutingError}, support::facades::Route, view::view};

fn register() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new()
            .domain("{tenant}.example.test")
            .prefix("admin")
            .name("admin.")
            .r#where("id", "[0-9]+")?,
        |router| {
            router
                .get("/reports/{id?}", || view("welcome"))?
                .name("reports.show")
                .defaults("id", "1");
            Ok(())
        },
    )
}
```

## Cross-Origin Resource Sharing (CORS)

Set the paths, origins, methods and headers in `config/cors.rs`. The global `HandleCors` middleware answers matching browser preflights with an empty HTTP 204 and applies the policy to actual responses, including 404 errors. For a denied origin in a dynamic policy, the browser receives no origin grant. A single configured origin may be sent as a fixed value, which only grants that origin.

When `CORS_PATHS` is unset, `cors.paths` matches Laravel’s defaults, `api/*` and `sanctum/csrf-cookie`. The API route prefix and CORS paths are configured independently. If your API is mounted at `/v1`, set `CORS_PATHS=v1/*,sanctum/csrf-cookie` to allow CORS on those paths. Set `CORS_PATHS=*` to cover all paths or `CORS_PATHS=` to disable CORS. Cross-origin credentials are disabled by default. A path map can override paths by host, using numeric string keys for shared paths. Use `allowed_origins_patterns` for patterns supported by Rust's regex engine. The HTTP kernel loads and validates the policy during bootstrap.

```bash
curl -i -X OPTIONS http://127.0.0.1:8000/api/example \
  -H 'Origin: https://example.com' \
  -H 'Access-Control-Request-Method: GET'
```

## Defining Rate Limiters

Register a policy in a service provider’s boot hook, then attach it to a route with `middleware_name("throttle:uploads")`. Use `Limit::by` for separate buckets, `response` for a custom rejection, or `after` to count selected responses. Rejected requests return HTTP 429 with `Retry-After`, `X-RateLimit-Reset`, `X-RateLimit-Limit` and `X-RateLimit-Remaining`.

```rust
use openrust::{
    cache::{CacheResult, rate_limiting::Limit},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::{RateLimiter, Route},
};

fn register_limits() -> CacheResult<()> {
    RateLimiter::r#for("uploads", |_| Limit::perMinute(5))
}

fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new(), |router| {
        router.get("/uploads", upload)?.middleware_name("throttle:uploads");
        Ok(())
    })
}

fn upload(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}
```

Use `middleware_name("throttle:30,0.5,uploads:")` for 30 requests per 30 seconds. A bare `throttle` allows 60 requests per minute. `throttle:5|20,1,uploads:` allows five requests per minute for guests and twenty for authenticated users. Guests share a bucket by the declared route domain and peer IP. Authenticated requests share a bucket by their authentication identifier. Give policies different prefixes to keep their quotas separate.

### Throttling With Redis

Call `middleware.throttleWithRedis()` in `withMiddleware` to resolve named and numeric throttle aliases through the default Redis connection. Processes sharing the same Redis connection settings and key prefix share their quota. Redis checks and charges the configured limits atomically. Limits using `after` are charged after the response.

## Route Model Binding

Use `Route::model::<Post>("post")?` to register an explicit binding in the application’s router. A route can also declare the model for its own parameter using the router’s `.model::<Post>("post")?` method. Read the resolved model from `request.route_model::<Post>("post")`. Missing models produce a 404. A route can supply a custom response with `missing`.

The model defaults to its primary key. Override `getRouteKeyName` for a slug or use `{post:slug}` for one route. A nested custom-key binding scopes through the preceding bound model. Use `scopeBindings` to require that scope for default keys or `withoutScopedBindings` to disable it. Implement the child model’s `resolveChildRouteBinding` with a typed relationship. Use `withTrashed` to include soft-deleted records. The default `web` and `api` groups run bindings before the handler.

## Redirect routes

Use `Route::redirect` to register a redirect with an explicit HTTP status. `Route::permanentRedirect` sends 301. A redirect destination such as `/articles/{id}` can reuse a parameter from the matched route.
