# Middleware

In `bootstrap/app.rs`, configure global middleware inside the `withMiddleware` closure using `append` or `prepend`. Use `RouteGroup::middleware` for routes in a group and `Route::middleware` for a single route.

Global middleware wraps route matching, so it can also inspect 404 and 405 responses. On a matching route, the outer group runs before the inner group, followed by middleware attached to the route. Each layer can call `next.run(request).await` to pass control downstream and inspect the returned response. A layer can return its own response immediately to skip the remaining layers and the handler.

## Default Locale Middleware {#locale-middleware}

The framework automatically installs `openrust::http::middleware::SetLocale`. It runs after Cookie decryption and queuing on protected Web routes, before ordinary Web Session; stateless APIs use no preference Cookie. See [Automatic and fixed language](../digging-deeper/localization.md#locale-preference) and [Custom locale selection](../digging-deeper/localization.md#custom-locale-selection) for persistence, disabling, execution order and callbacks. [URL selection](../digging-deeper/localization.md#locale-query) and [`app.locale_header`](../digging-deeper/localization.md#locale-header) can be configured or disabled independently. At startup, its default candidates come from [application translation resources](../digging-deeper/localization.md#available-locales); explicit candidate lists take precedence.

## Named groups and priority

In `withMiddleware`, register named groups with `group` and individual instances with `alias`. A group accepts `MiddlewareEntry::Instance` or `MiddlewareEntry::named`. The latter can refer to another group or an alias. Attach a name to a route or route group with `middleware_name`. A missing name or a cyclic reference fails application startup before the listener opens.

Use `priority` with an ordered list of concrete `TypeId` values such as `TypeId::of::<Authenticate>()`. The framework expands groups first and moves an earlier-ranked middleware ahead of the preceding later-ranked middleware when their order conflicts. Unranked entries keep their relative order. The same instance reached through multiple references runs once. Priority applies to the route stack. Global middleware keeps its configured order and wraps the route stack.

Use `withoutMiddleware("name")` on a route or group to exclude a named alias or group. Exclusions also apply to instances reached through nested groups. Global middleware still runs.

## Terminable Middleware

Implement `TerminableMiddleware` to receive request and response metadata after the HTTP transport releases the response body. In `withMiddleware`, use `appendTerminable`, `prependTerminable` or `aliasTerminable`. For a concrete entry in a group, use `MiddlewareEntry::terminable`.

The kernel calls the selected route hooks in resolved order, then the global hooks. The same registered instance handles both phases. A stream may keep the hook pending while the transport consumes it. Hooks also run when a client disconnects and the transport drops the body.

An application using `Kernel::handle` directly must call `Kernel::terminate` with the request and response parts after its transport finishes. The built-in HTTP server handles this automatically.

On graceful shutdown, the built-in HTTP server stops accepting new connections and waits for open connections and pending termination hooks before the process exits.

## Default Web and API Groups

With a configured session driver, the default `web` group runs `EncryptCookies`, `AddQueuedCookiesToResponse`, `StartSession`, `ShareErrorsFromSession`, `PreventRequestForgery` and `SubstituteBindings` in that order. The default `api` group runs `SubstituteBindings`.

### API rate limiting

The default `api` group does not enable rate limiting. Register a named limiter in a service provider's `boot` method as described in [routing](routing.md#defining-rate-limiters), then enable it in `withMiddleware`:

```rust
.withMiddleware(|middleware| {
    middleware.throttleApi(Some("api"), false);
})
```

Arguments are explicit: `Some("api")` selects the `api` policy and `false` keeps the current backend. Pass another policy name to select it, or `None` to disable throttling in the default API group. The last policy selection takes effect; the policy must be registered separately.

Passing `true` as the second argument calls `throttleWithRedis()`, switching the `throttle` alias to Redis for API and other routes. A subsequent call with `false` does not undo that switch.

In the default API group, stateful session middleware enabled by `statefulApi()` runs first, followed by throttling and `SubstituteBindings`. Explicitly replacing the group with `group("api", ...)` takes precedence over `throttleApi`, regardless of call order.
