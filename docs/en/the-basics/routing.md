# Routing

Declare web routes in `routes/web.rs`. The application registers those routes and the `/up` health check through `openrust::routing::Router` during startup. A handler that needs the request takes a mutable `Request` and returns an asynchronous HTTP result. The framework binds the application router while calling the Web route module’s `register()`. Declare a named view with `Route::get("/", || view("welcome"))`, using `openrust::support::facades::Route`.

API routes default to the `/api` URL prefix. Use `Routing::default().api(routes::api::register).apiPrefix("v1")` to mount them under `/v1`, or pass an empty string to omit the prefix. The `api` middleware group still applies. The application’s exception rendering policy is configured separately through `withExceptions`.

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

The `health("/up")` option in `bootstrap/app.rs` registers a health endpoint. A normal request returns an HTML page containing “Application up”; a request expecting JSON returns `{"status":"up"}`. Both return 200, and HEAD sends no body. The HTML title uses the escaped application name.

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

This confirms that HTTP bootstrap succeeded. Diagnostic event listeners and their failure response are pending the event dispatcher implementation. The health page uses local HTML rather than Laravel's Blade view and external styling assets.

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
