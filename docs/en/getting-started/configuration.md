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

<a id="public-files"></a>

## Public files

| Environment setting | Purpose | Default |
|---|---|---|
| `PUBLIC_PATH` | Application public directory, exposed by `Application::publicPath()` | `public` |
| `PUBLIC_URL_PREFIX` | URL prefix for the application public directory | `/` |
| `PUBLIC_STORAGE_ROOT` | `filesystems.disks.public.root` for the public local disk | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | `filesystems.disks.public.url` | `APP_URL` with `/storage` appended |

Relative paths use the application root. An application can override `PUBLIC_PATH` by calling `usePublicPath()` before boot. The default Web routes apply the public disk settings through `Route::staticDisk("public")`. The mount uses the full path of the disk URL. The default is `/storage`. A same-origin URL ending in `/storage/tmp/` or the absolute URL path `/storage/tmp/` mounts at `/storage/tmp`. A cross-origin URL fails route registration. Restart the application after changing mount settings.
