# Routing

Declare web routes in `routes/web.rs`. The application registers those routes and the `/up` health check through `openrust::routing::Router` during startup. A handler that needs the request takes a mutable `Request` and returns an asynchronous HTTP result. The framework binds the application router while calling the Web route module’s `register()`. Declare a named view with `Route::get("/", || view("welcome"))`, using `openrust::support::facades::Route`.

The default application already registers `routes/api.rs` and `config/api.rs`; plain API endpoints do not require Sanctum. Run `./artisan install:api` only when API authentication is needed. It installs Sanctum and adds the protected `/user` route to an unchanged empty API skeleton. Existing business routes are preserved unless `--force` is explicitly requested; add the protected route manually when retaining a custom routes file. Declare API routes in `routes/api.rs`. The application loads `api.prefix` from `config/api.rs` and applies the `api` middleware group. When `API_PREFIX` is unset, the prefix defaults to `/api`. Set `API_PREFIX=api/admin` in `.env` to mount routes under `/api/admin`, or set `API_PREFIX=` to omit the prefix. An explicit `Routing::api_prefix(...)` in `with_routing` takes precedence. Reinstalling preserves an existing `config/api.rs`, even with `--force`.

In `routes/api.rs`, declare API endpoints in the parameterless `register() -> Result<(), RoutingError>` using the `Route` facade. Route paths are relative to the API prefix. Use `Route::get_with` when a GET route needs a name, model binding, or route-level middleware.

A parameter such as `{id}` is required. Add `?` to allow a trailing parameter to be omitted, as in `/profiles/{id?}/{tab?}` or `/report/{name}.{format?}`. Read the captured value with `request.route("id")`. For an omitted parameter, this returns `None` unless the route supplies a string through `defaults`. A parameter before required text or another required parameter must still be present.

Use `Route::r#where` for a regular expression, or use `where_number`, `where_alpha`, `where_alpha_numeric`, `where_uuid`, `where_ulid`, or `where_in`. Set `Router::pattern` before registering routes to apply a constraint to every subsequent route that uses that parameter name. A route-specific constraint overrides it. `where_in` joins its values as regex alternatives, so any regex metacharacters retain their meaning. Routes without explicit constraints use Rust's linear-time regex engine. Explicit constraints use PCRE2, including lookaround, named backreferences, and `(*MARK:...)`. For example, `r"(?!admin\z)[a-z]+"` excludes `admin`. Matching uses UTF-8 and Unicode character properties. Invalid expressions fail registration. At runtime, a match that exceeds the PCRE2 match, depth, or heap limit is treated as a non-match. In `/paired/{id}/{tab?}`, set the `tab` constraint to `r"\k<id>"` to require a matching pair when `tab` is present. A reference can resolve to another capture in the same compiled path or host. Group and global constraints resolve their references when the route is registered.

Matching uses the UTF-8 percent-decoded path and ignores the query string. Invalid UTF-8 returns HTTP 400. Constraints apply to both route selection and the method check. For example, if `/profiles/{id?}` is a GET route with a numeric ID, `POST /profiles/7` returns 405 with `Allow`, while `POST /profiles/alice` returns 404 when no other route matches that path.

`Router::get` registers both GET and HEAD. HEAD preserves the response status and headers and sends an empty body. An unknown path returns 404. A known path with an unsupported method returns 405 with an `Allow` header. When a matching path has no explicit OPTIONS route, the router returns 200 with the allowed methods. Among overlapping routes for the same method, the first registered match wins.

## HTTP Methods

Use `get`, `post`, `put`, `patch`, `delete`, `options`, or `query` for a single method. Both `Route` and `Router` provide these methods; `get` also accepts HEAD. `Router` returns a mutable route for naming, middleware, constraints and defaults, while facade declarations return `Result<(), RoutingError>`.

```rust
use openrust::{http::Method, support::facades::Route};

Route::get("/users", UserController::index)?;
Route::post("/users", UserController::store)?;
Route::put("/users/{id}", UserController::replace)?;
Route::patch("/users/{id}", UserController::update)?;
Route::delete("/users/{id}", UserController::destroy)?;
Route::options("/users", UserController::options)?;
Route::query("/users/search", UserController::search)?;
Route::r#match(&[Method::PUT, Method::PATCH], "/profile", ProfileController::update)?;
Route::any("/callback", CallbackController::handle)?;
```

Use `r#match` to share a handler across an explicit set of methods. GET in that set also adds HEAD. An empty set returns `RoutingError::MissingMethods`. Use `any` only when the same endpoint should accept all supported methods: GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS and QUERY, in that order. It does not include TRACE or CONNECT.

For the same method, domain and registered path, a later declaration replaces the earlier handler. This also applies to `any` and `r#match`: they have no separate priority. For different overlapping patterns, the first matching declaration wins. Declare specific paths before broader patterns.

## View Routes

Use `Route::view` when a GET/HEAD route only returns a view. The first two arguments are the URI and view name. Rust requires the third options argument explicitly; `Default::default()` supplies empty data, status 200 and no additional headers.

```rust
use openrust::{routing::ViewRouteOptions, support::facades::Route, view::ViewData};

Route::view("/preview", "welcome", Default::default())?;
Route::view("/hello/{name}", "welcome", ViewRouteOptions {
    data: ViewData::from_iter([("name".into(), "Taylor".into())]),
    ..Default::default()
})?;
```

Put templates in `resources/views`, for example `welcome.blade.html`. Captured path and domain parameters override matching keys in `data`, before view creators and composers run. Set `status` and `headers` in `ViewRouteOptions` for a custom response. PHP's alternative headers-in-status argument is represented by the explicit `headers` field. Do not use `view`, `data`, `status` or `headers` as parameter names. Route defaults currently carry strings; arbitrary typed changes to these four view settings through `Route::defaults` are not supported. Bound model objects are not automatically converted into view data.

`Router::view` returns a mutable route. For facade declarations, use `Route::view_with(uri, view, options, |route| { route.name("welcome"); Ok(()) })` to configure the route inside its registration borrow. Rendering uses the current application's view factory and middleware. A missing template or failing creator/composer follows the normal HTTP exception pipeline.

## Fallback Routes

Declare a fallback in `routes/web.rs` to handle GET/HEAD requests that no ordinary route matches. It inherits the current group's prefix, domain and middleware. A handler's returned error or 404 response does not cause another fallback attempt.

```rust
use openrust::support::facades::Route;

Route::fallback(NotFoundController::show)?;
```

Ordinary routes take precedence even when the fallback was declared earlier. Among matching fallbacks, registration order applies; a later declaration for the same method, domain and registered URI replaces the earlier one. An unmatched POST returns 405 with `Allow: GET, HEAD` when the fallback matches its path; automatic OPTIONS returns 200. A fallback is a registered route, so it takes precedence over the static file mounts described below.

Use `Route::fallback_with(handler, |route| { route.name("missing"); Ok(()) })` for facade configuration. `Router::fallback` returns a mutable route; `route.fallback()` marks an existing route, and `route.set_fallback(false)` removes the marker. Routing cache preserves this precedence.


A handler can explicitly return another named route's response. `respond_with_route` binds that route against the same request URI, runs its middleware and replaces the request's current route parameters and name; it does not generate a redirect or substitute the target URI.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
};

fn show_missing(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(Route::respond_with_route("missing", request))
}
```

Register `missing` using `fallback_with` as above, then register `show_missing` as a handler. Rust requires an explicit mutable request borrow; the HTTP kernel supplies the shared runtime router through the application container. Each request keeps its own route state. Missing names return an error rendered as HTTP 500. The same public method is available on `Router` for direct consumers. Runtime router resolution is available after HTTP bootstrap; registration continues to use the route registration scope.

## Mounting a static directory

The application skeleton mounts the website directory first and the public storage disk second. See [public file configuration](../getting-started/configuration.md#public-files) for the directories and URL prefixes.

```rust
Route::static_disk("web")?;
Route::static_disk("public")?;
```

The `web` and `public` disks are configured in `config/filesystems.rs`. Their default roots are `public` and `storage/app/public`, and their default URLs are `app.url` and `app.url` with `/storage` appended. Mount paths come from those URLs, including any application URL path prefix. Application routes take precedence. Disk mounts are checked in declaration order, with the first existing public file winning. Both mounts reject symbolic links, so a `public/storage` link is skipped and the `public` disk can serve the file directly. Only unmatched GET and HEAD requests serve files. Missing files and directory requests return 404.

Static file responses include `Last-Modified` when the file has a usable modification time. Send that value in `If-Modified-Since` with GET or HEAD to revalidate the file. If it has not changed, the response is `304 Not Modified` with an empty body.

## Health Check

The `health("/up")` option in `bootstrap/app.rs` registers a health endpoint. A healthy request returns 200. The HTML page shows the application name in the title, “Application up”, and how long the request took to reach rendering. Requests expecting JSON receive `{"status":"up"}`. HEAD sends no body.

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

The endpoint dispatches `openrust::foundation::events::DiagnosingHealth`. Register synchronous listeners to check dependencies such as the database or cache. If a listener fails, the framework reports the error and returns 500 with “Application experiencing problems” or `{"status":"down"}`. In debug mode, the normal exception handler renders the failure. The health page ships with its own styles. The configured health path stays accessible during maintenance mode.

## Listing Your Routes

Run `route:list` to see active routes. Use `--json` for structured output. Filter by path, method, name, domain, action or middleware. Use `--except-path` with comma-separated substrings. `--sort=definition` preserves registration order, and `--reverse` reverses the result.

```sh
./artisan route:list --json --method=HEAD
```

Use `-v` to show the middleware declared on each route. Use `-vv` to expand groups and see the middleware type names in request order, with exclusions applied and alias parameters retained. The output covers route middleware. With `--json`, `middleware` remains an array, and `--middleware` filters the view selected by the verbosity level.

```sh
./artisan route:list -vv --json --middleware=SubstituteBindings
```

The route list preserves binding fields such as `{post:slug}`, optional markers and domain patterns such as `{tenant}.example.test`. Text and JSON output include the controller action registered with `set_action`. A later route for the same method, domain and path replaces the earlier route even if the binding field changes. Other methods keep their original routes.

```sh
./artisan route:list --domain=example.test --action=PostController --json
```

## Route Caching {#route-caching}

Use `route:cache` to store a matching index for the deployed application.

```sh
./artisan route:cache
./artisan route:clear
```

The cache stores candidates by HTTP method and first path segment. Rust still registers handlers, model bindings and middleware at startup. The index preserves registration order. Missing, unreadable or incompatible indexes fall back to the normal matcher. Run `route:cache` again after changing routes, and use `route:clear` to remove the index.

The default path is `storage/framework/cache/routes.json`. Set `APP_ROUTES_CACHE` to an absolute path or a path relative to the application root. Export it in the service environment when using cached configuration, because that startup can skip loading `.env`.

After boot, `get_cached_routes_path()` returns the resolved path and `routes_are_cached()` tells you whether this application loaded a valid index.

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

When `CORS_PATHS` is unset, `cors.paths` matches defaults, `api/*` and `sanctum/csrf-cookie`. The API route prefix and CORS paths are configured independently. If your API is mounted at `/v1`, set `CORS_PATHS=v1/*,sanctum/csrf-cookie` to allow CORS on those paths. Set `CORS_PATHS=*` to cover all paths or `CORS_PATHS=` to disable CORS. Cross-origin credentials are disabled by default. A path map can override paths by host, using numeric string keys for shared paths. Use `allowed_origins_patterns` for patterns supported by Rust's regex engine. The HTTP kernel loads and validates the policy during bootstrap.

Inside `with_middleware`, use `middleware.skip_cors_when(|request| request.is("api/internal/*"));` to bypass CORS for matching requests in the current application. When a callback returns `true`, `HandleCors` passes the request to the remaining middleware and leaves the response headers untouched. This also applies to `OPTIONS` preflights.

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
    RateLimiter::r#for("uploads", |_| Limit::per_minute(5))
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

Use `throttle:5|upload_limit,1,uploads:` to read the authenticated user’s `upload_limit` while retaining a fixed guest quota. Database-provider users expose their columns. An application model can expose its attributes through `Authenticatable::get_auth_attribute`, delegating to `Model::get_attribute_value` for accessors and casts. Quotas accept non-negative numbers and numeric strings within the signed 64-bit range. A missing or nonnumeric attribute is a configuration error.

### Throttling With Redis

Call `middleware.throttle_with_redis()` in `with_middleware` to resolve named and numeric throttle aliases through the default Redis connection. Processes sharing the same Redis connection settings and key prefix share their quota. Redis checks and charges the configured limits atomically. Limits using `after` are charged after the response.

With a [default Redis cluster](../database/redis.md#clusters), all counters for one named policy share a hash slot so Redis can check and charge them together. Different named policies can use different slots. Upgrading from the previous Cluster key layout starts a new window for named policies. The old counters expire at their original times.

## Route Model Binding

Use `Route::model::<Post>("post")?` to register an explicit binding in the application’s router. A route can also declare the model for its own parameter using the router’s `.model::<Post>("post")?` method. Read the resolved model from `request.route_model::<Post>("post")`. Missing models produce a 404. A route can supply a custom response with `missing`.

The model defaults to its primary key. Override `get_route_key_name` for a slug or use `{post:slug}` for one route. A nested custom-key binding scopes through the preceding bound model. Use `scope_bindings` to require that scope for default keys or `without_scoped_bindings` to disable it. Implement the child model’s `resolve_child_route_binding` with a typed relationship. Use `with_trashed` to include soft-deleted records. The default `web` and `api` groups run bindings before the handler.

## Redirect routes

Use `Route::redirect` to register a redirect with an explicit HTTP status. `Route::permanent_redirect` sends 301. A redirect destination such as `/articles/{id}` can reuse a parameter from the matched route.
