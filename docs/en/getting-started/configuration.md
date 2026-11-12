# Configuration

Copy `.env.example` to `.env` in the application root to customize local settings. `APP_HOST` and `APP_PORT` configure the HTTP listener and default to `127.0.0.1:8000`. When `APP_URL` is unset, it defaults to `http://localhost` with the configured `APP_PORT`. The template follows Laravel’s local development defaults. The application loads the sections in `config/` into one repository. Use `application.config()?.get("app.url")` to read the effective value. Application, authentication, database and Redis, cache, session, logging, mail, queue, filesystems, broadcasting, CORS, concurrency, hashing, images, services, Reverb, gRPC, and view settings each have a dedicated section. Loading settings performs no external connections.

## Current configuration repository

Configuration becomes available during boot, before providers register. As in Laravel's LoadConfiguration, boot binds a new repository before evaluating the configuration loader, then fills that repository. `create()` does not preload an empty configuration service. `Application::config()` returns `Result<Arc<dyn Repository>, BindingResolutionError>` by resolving the current `config` service each time; access before boot or after an incompatible replacement returns an error.

`config` is the canonical name; `dyn openrust::contracts::config::Repository` is its typed alias. To replace the application repository, construct `openrust::config::Repository::new(values)` and call `instanceNamed::<dyn Repository>("config", repository)?`. Consumers already holding an `Arc` keep that snapshot; resolve again or use a rebinding/refresh listener when they must follow replacements. Rebinding the typed alias as a raw key replaces that alias rather than the canonical configuration service, matching the container's raw-key rules.

Provider replacements are used by HTTP listener binding, route/static-disk assembly and view-path setup. Health HTML reads the current repository on each request, and Artisan uses the same application accessor. Already assembled routes and view finders are not automatically rebuilt after a runtime configuration replacement. Container errors propagate through the relevant bootstrap, HTTP or Artisan error path. See [rebinding](../architecture-concepts/container.md#rebinding-and-refreshing-consumers).

Numeric and boolean settings are validated at startup. Booleans accept `true`, `false`, `1`, and `0`. Nullable settings such as `SESSION_DOMAIN` accept `null`. Each driver still requires the corresponding framework service to be available.

`APP_PORT` is checked when HTTP binds its listener. Artisan does not require a valid HTTP listener address. Port `0` lets the operating system select a listener port; set `APP_URL` explicitly when URLs must use a stable public address.

The HTTP view renderer searches the directories in `view.paths` in order. By default it reads HTML files from `resources/views`. Relative directory entries are resolved from the application root.

Process environment variables take precedence over values in the file. Set `APP_ENV` in the process environment to select a file such as `.env.local`. If the selected file is absent, the application reads `.env`. See [application root resolution](../architecture-concepts/lifecycle.md) for deployment paths and the `APP_BASE_PATH` override.

<a id="determining-the-current-environment"></a>

## Determining the current environment

The application detects its environment after configuration loads and before providers register. It reads the string at `app.env` and uses `production` when the key is absent. The default configuration gets `app.env` from `APP_ENV`. HTTP boots with this configured name.

Artisan accepts `--env=staging` and `--env staging` before or after the command. For an invocation with repeated options, the first value determines the environment. The loader tries the file for `--env`, then the file for the external `APP_ENV`, then `.env`. Process variables take precedence over file values. The option also sets the detected environment for the boot, even when file selection falls back. It leaves the configuration value under `app.env` as loaded.

```sh
./artisan env --env=staging
```

Pass `None` to `Application::environment` for the detected name. Pass a single pattern or a slice of patterns inside `Some` to get a Boolean result. Matching is case-sensitive and `*` spans any characters. An empty slice returns false. The application installs the same matcher as the container environment resolver during boot.

```rust
use openrust::{
    container::EnvironmentResult,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withConfig(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().currentEnvironmentIs(&["local", "staging"])?));
    assert!(!bool::from(app.container().currentEnvironmentIs(&[])?));
    Ok(())
}
```

During bootstrap the detected name is stored in the `env` service as `Arc<str>`. The environment query reads this service. Editing `app.env` after boot edits configuration only.

<a id="public-files"></a>

## Public files

| Environment setting | Purpose | Default |
|---|---|---|
| `APP_PUBLIC_PATH` | Application public directory, exposed by `Application::publicPath()` | `public` |
| `APP_PUBLIC_URL_PREFIX` | URL prefix for the application public directory | `/` |
| `PUBLIC_STORAGE_ROOT` | `filesystems.disks.public.root` for the public local disk | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | `filesystems.disks.public.url` | `APP_URL` with `/storage` appended |

Relative paths use the application root. An application can override `APP_PUBLIC_PATH` by calling `usePublicPath()` before boot. The default Web routes apply the public disk settings through `Route::staticDisk("public")`. The mount uses the full path of the disk URL. The default is `/storage`. A same-origin URL ending in `/storage/tmp/` or the absolute URL path `/storage/tmp/` mounts at `/storage/tmp`. A cross-origin URL fails route registration. Restart the application after changing mount settings.

## Maintenance mode {#maintenance-mode}

Run Artisan `down` to return HTTP 503 while you deploy. Use `up` to reopen the application. A running server reads the shared state on each request. You can run `down` again to update the options.

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

Use `--retry` for a positive delay in seconds or a retry date, and `--refresh` to tell a browser when to reload. Local date-times use the application timezone and are sent in GMT. `--redirect` takes a local path. `--render` prerenders a named view once when you run `down`, passing it `$retryAfter`. `--status` overrides the default 503. Requests that expect JSON follow the usual exception renderer and skip the browser redirect and prerendered view.

To check the site during maintenance, open the URL printed by `--with-secret`. The application sets a signed browser cookie valid for 12 hours and redirects to the home page when no intended session URL is available. Use `--secret` to supply your own path. Use `withMiddleware` in `bootstrap/app.rs` to leave specific endpoints available.

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["up", "status/*"]);
})
```

For a file-backed deployment, `./artisan down --render=errors::503` serves the prepared HTML while providers are still booting. Put a custom page at `resources/views/errors/503.blade.html`. The listener settings must load successfully. JSON, excluded paths, and valid bypass requests wait for the application kernel. Cache-backed maintenance uses the running kernel. See [workers during maintenance](../digging-deeper/queues.md#workers-during-maintenance) for the queue behavior.

### Shared maintenance state {#shared-maintenance-state}

The application exposes maintenance state through `maintenanceMode()`. The default `file` driver stores the payload in `storage/framework/down`. Choose `cache` and a shared cache store to make the state visible to workers on multiple hosts. The `array` driver keeps state inside a single application instance for testing.

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenanceMode()?.activate(&payload).await?;
    Ok(())
}
```

Read the state with `isDownForMaintenance().await` and clear it with `app.maintenanceMode()?.deactivate().await?`. Queue workers read this state before reserving jobs.
