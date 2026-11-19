# Configuration

Copy `.env.example` to `.env` in the application root to customize local settings. `APP_HOST` and `APP_PORT` configure the HTTP listener and default to `127.0.0.1:8000`. The template follows Laravel’s local development defaults. The application loads the sections in `config/` into one repository. Use `application.config()?.get("app.url")` to read the effective value. Application, authentication, database and Redis, cache, session, logging, mail, queue, filesystems, broadcasting, CORS, concurrency, hashing, images, services, Reverb, gRPC, and view settings each have a dedicated section. Loading settings performs no external connections.

An explicit `APP_URL` takes precedence for `app.url`. When it is unset, a specific `APP_HOST` and `APP_PORT` produce a URL such as `http://example.test:8080` or `http://[2001:db8::1]:8080`. An unset host, a loopback address such as `127.0.0.1` or `::1`, or a wildcard listener such as `0.0.0.0` or `::` uses `http://localhost:{port}`. The environment template sets `APP_URL="http://${APP_HOST}:${APP_PORT}"`, which expands to `http://127.0.0.1:8000` with its default settings.

HTTP URL generation, including `URL::forRequest(request)`, uses the explicit `APP_URL` first. With no explicit URL, it uses the current request origin, then the listener-derived `app.url` if the request has no origin. Artisan uses `app.url` directly.

Set `APP_VERSION` to override the application version stored in `app.version`. When `APP_VERSION` is unset, the version defaults to the Cargo package version captured at compile time by `env!("CARGO_PKG_VERSION")`. The welcome page displays `app.version`.

## Current configuration repository

Configuration becomes available during boot, before providers register. As in Laravel's LoadConfiguration, boot binds a new repository before evaluating the configuration loader, then fills that repository. `create()` does not preload an empty configuration service. `Application::config()` returns `Result<Arc<dyn Repository>, BindingResolutionError>` by resolving the current `config` service each time; access before boot or after an incompatible replacement returns an error.

Inside an application scope, import `openrust::support::facades::Config` and call `Config::get("app.name")?`. The facade resolves the same application repository on each call. It returns `None` for a missing key and `Some(Value::Null)` for an explicit null. Calling it outside an application scope or before the repository is registered returns a binding error.

`config` is the canonical name; `dyn openrust::contracts::config::Repository` is its typed alias. To replace the application repository, construct `openrust::config::Repository::new(values)` and call `instanceNamed::<dyn Repository>("config", repository)?`. Consumers already holding an `Arc` keep that snapshot; resolve again or use a rebinding/refresh listener when they must follow replacements. Rebinding the typed alias as a raw key replaces that alias rather than the canonical configuration service, matching the container's raw-key rules.

Provider replacements are used by HTTP listener binding, route/static-disk assembly and view-path setup. Health HTML reads the current repository on each request, and Artisan uses the same application accessor. Already assembled routes and view finders are not automatically rebuilt after a runtime configuration replacement. Container errors propagate through the relevant bootstrap, HTTP or Artisan error path. See [rebinding](../architecture-concepts/container.md#rebinding-and-refreshing-consumers).

Numeric and boolean settings are validated at startup. Booleans accept `true`, `false`, `1`, and `0`. Nullable settings such as `SESSION_DOMAIN` accept `null`. Each driver still requires the corresponding framework service to be available.

`APP_PORT` is checked when HTTP binds its listener. Artisan does not require a valid HTTP listener address. Port `0` lets the operating system select a listener port; set `APP_URL` explicitly when URLs must use a stable public address.

The HTTP view renderer searches the directories in `view.paths` in order. By default it reads HTML files from `resources/views`. Relative directory entries are resolved from the application root.

Process environment variables take precedence over values in the file. Set `APP_ENV` in the process environment to select a file such as `.env.local`. If the selected file is absent, the application reads `.env`. See [application root resolution](../architecture-concepts/lifecycle.md) for deployment paths and the `APP_BASE_PATH` override.

## Accessing Configuration Values {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::stringOr("app.name", "App")
}
```

Use `Config::set` to change a value in the current application. The `string`, `integer`, `float`, `boolean`, `array`, and `collection` methods check the value type. `array` accepts JSON lists and objects, and `collection` preserves their keys.

Use `Config::stringOr("app.name", "App")` to get a `String` directly. It returns the default if the key is absent, the value is null or another type, or the repository is unavailable. Use `Config::string("app.name")?` for strict reads. Raw `Config::getOr("app.name", Value::from("App"))?` defaults only when the key is absent and preserves an explicit null. `Config::getMany([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` reads keys with individual defaults in input order. To read an entire section as a struct, derive `serde::Deserialize` and use `Config::deserialize::<MailOptions>("mail")?`.

## Configuration Caching {#configuration-caching}

Use `config:cache` to write the configuration from a fresh application boot, including service provider changes. Later HTTP and Artisan boots read the snapshot. Use `config:clear` to remove it before restarting with updated settings.

```sh
./artisan config:cache
./artisan config:clear
```

When `APP_URL` is set and the configuration is cached, ordinary links and absolute signed links use that URL.

The refresh removes the selected old snapshot before booting the fresh application. If that boot fails, the cache remains absent.

The default location is `storage/framework/config.json`. Set `APP_CONFIG_CACHE` to an absolute path or a path relative to the application root to select a different file. Export the override in the process environment to make that path available before loading `.env`. Use a separate cache path for each deployment environment.

A cache hit found before dotenv loading skips that file. The cache contains effective settings and may contain secrets. Keep it with private deployment data. Invalid JSON stops startup. If the cache file is damaged, remove that file before rebuilding it.

Use `./artisan config:show app.name` to inspect a key, or `./artisan config:show app` to inspect a section. The command displays the active values, which may contain credentials.

## Environment File Encryption {#environment-file-encryption}

Use `env:encrypt` to create an encrypted copy of an environment file. Set `ENV_ENCRYPTION_KEY` in the process environment before running `env:decrypt`, or supply `--key`. If encryption generates a key, copy the displayed `base64:` value to your secret store separately from the encrypted file.

```sh
./artisan env:encrypt --env=staging --no-interaction
./artisan env:decrypt --env=staging --force --no-interaction
```

The default cipher is AES-256-CBC. Supply the same `--cipher` for encryption and decryption when choosing AES-128-CBC, AES-128-GCM or AES-256-GCM. `--env=staging` selects `.env.staging` and `.env.staging.encrypted`. Without `--env`, the encrypted source follows the environment file selected by bootstrap, and decryption writes `.env` by default.

Use `--readable` to encrypt values individually and keep variable names visible. A later readable run with the same key reuses unchanged ciphertext. Use `--force` to replace existing output and `--prune` to remove the source after successful encryption. For decryption, `--path` and `--filename` select the output directory and name.

A supplied key is kept out of command output. When using `--quiet` or `--silent` for encryption, supply a key so you can decrypt the output later.

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
| `APP_PUBLIC_PATH` | `filesystems.disks.web.root` / `Application::publicPath()` | `public` |
| `FILESYSTEM_WEB_URL` | `filesystems.disks.web.url` | `app.url` |
| `FILESYSTEM_PUBLIC_ROOT` | `filesystems.disks.public.root` for the public local disk | `storage/app/public` |
| `FILESYSTEM_PUBLIC_URL` | `filesystems.disks.public.url` | `app.url` with `/storage` appended |

When `APP_PUBLIC_PATH` is omitted, `config/app.rs` explicitly sets `app.public_path` to `public`. The `web` disk reuses that value as its default root. Configure website files under `filesystems.disks.web` and uploaded public files under `filesystems.disks.public`. The default routes mount them in that order using `Route::staticDisk("web")` and `Route::staticDisk("public")`. Calling `usePublicPath()` before boot overrides the website directory and updates `web.root`. Relative paths use the application root. A disk mount uses the full path of its URL, so `/storage/tmp/` mounts at `/storage/tmp`. Absolute URLs must have the same origin as `app.url`. Restart the application after changing mount settings. An explicitly empty or whitespace-only `APP_PUBLIC_PATH` fails configuration loading.

## Maintenance mode {#maintenance-mode}

Run Artisan `down` to return HTTP 503 while you deploy. Use `up` to reopen the application. A running server reads the shared state on each request. You can run `down` again to update the options.

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

Use `--retry` for a positive delay in seconds or a retry date, and `--refresh` to tell a browser when to reload. Local date-times use the application timezone and are sent in GMT. `--redirect` takes a local path. `--render` prerenders a named view once when you run `down`, passing it `$retryAfter`. `--status` overrides the default 503. Requests that expect JSON follow the usual exception renderer and skip the browser redirect and prerendered view.

To check the site during maintenance, open the URL printed by `--with-secret`. The application sets a signed browser cookie valid for 12 hours and redirects to the home page when no intended session URL is available. Use `--secret` to supply your own path. The configured health path (default `/up`) stays accessible during maintenance mode. Use `withMiddleware` in `bootstrap/app.rs` to leave additional endpoints available.

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["status/*"]);
})
```

For a file-backed deployment, `./artisan down --render=errors::503` serves the prepared HTML while providers are still booting. Put a custom page at `resources/views/errors/503.blade.html`. The listener settings must load successfully. JSON, excluded paths, and valid bypass requests wait for the application kernel. Cache-backed maintenance uses the running kernel. See [workers during maintenance](../digging-deeper/queues.md#workers-during-maintenance) for the queue behavior.

During startup, use the exact bypass URL printed by Artisan. The early handler compares the request target including its query string and trailing slash. After the HTTP kernel starts, maintenance middleware compares the path without its query string or surrounding slashes.

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

## Testing Environment and Bootstrap Snapshot {#testing-environment}

`Application::runningUnitTests()` and the application contract return `Result<bool, BindingResolutionError>`. The result is true only when the bound `env` is exactly `testing`, regardless of whether a test harness is running. Before the environment is bound it returns `Ok(false)` without resolving a service. Resolution errors propagate. Changing `config("app.env")` later does not replace the detected environment binding.

After boot, use `app.make::<openrust::config::Environment>().await?` to read the immutable environment snapshot for that application. Existing `get` and `get_utf8` methods return raw strings; package-specific flag parsing remains the package's responsibility. The snapshot follows process precedence, environment-file selection and configuration-cache rules. It neither reloads files nor mutates process-global variables.

## Application Storage Path {#application-storage-path}

`app.storagePath("")` returns the storage root; pass a string such as `"pail"` for a subpath. `app.useStoragePath(path)?` overrides it and rebinds `path.storage`. The getter is also in the application contract. `openrust::support::storage_path(path)?` reads the current application scope. The readonly `foundation::Paths` service shares the same state with compiled packages, without retaining Application through its container.

Precedence is a nonempty override other than `0`, then `APP_STORAGE_PATH` from the application environment snapshot, then the application's `storage` directory. This unbranded variable maps Laravel's `LARAVEL_STORAGE_PATH`. An explicitly present empty environment value is retained. Setter values stay relative when supplied as relative paths. Joining appends the native separator and strips leading native separators from the subpath; it preserves duplicate separators and `..`. There is no default Rust argument, so pass `""` for the root.

The constructor initializes `path.storage`; a later environment-file value changes the getter without automatically rewriting that binding. Calling `useStoragePath` explicitly rebinds it, following Laravel's order. Rebinding errors return `Result` after updating path state.

Logging, file sessions, file cache data and locks, compiled views, and default local/public disks use the storage root when configuration loads. `FILESYSTEM_PUBLIC_ROOT` and `VIEW_COMPILED_PATH` retain their own overrides; the public URL and mount order are unchanged. `storage:link` targets the configured public disk. Set an absolute `APP_STORAGE_PATH` before startup, or call `useStoragePath` before configuration loads.

The compiled configuration loader uses `env.storage_path(base_path, subpath)` as its Rust context adapter. Its weak reference selects the owning application's paths without using a global Facade or retaining the application. Standalone environment snapshots resolve their own variable against the supplied default base. Later changes to the application path do not rewrite already loaded or cached configuration values.

File maintenance resolves the current path on each operation, including after the service was resolved. The early prerender check uses paths available before boot: provide a process variable or explicit setter for this check; a dotenv-only value is not available yet. Pail fixes its directory on first Files resolution, so configure the path before that use. Configuration, route, event and package cache artifact locations keep their separate cache-path contracts.
