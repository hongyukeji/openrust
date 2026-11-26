# Configuration

The application name and version default to the application package’s `name` and `version` in `Cargo.toml`, embedded at compile time. Set `APP_NAME` or `APP_VERSION` in `.env` to override them. The skeleton’s `.env.example` explicitly sets `APP_NAME=OpenRust`, which takes precedence over the package name.

Copy `.env.example` to `.env` in the application root to customize local settings. `APP_HOST` and `APP_PORT` configure the HTTP listener and default to `127.0.0.1:8000`. The template follows local development defaults. The application loads the sections in `config/` into one repository. Use `application.config()?.get("app.url")` to read the effective value. Application, authentication, database and Redis, cache, session, logging, mail, queue, filesystems, broadcasting, CORS, concurrency, hashing, images, services, Reverb, gRPC, and view settings each have a dedicated section. Loading settings performs no external connections.

An explicit `APP_URL` takes precedence for `app.url`. When it is unset, a specific `APP_HOST` and `APP_PORT` produce a URL such as `http://example.test:8080` or `http://[2001:db8::1]:8080`. An unset host, a loopback address such as `127.0.0.1` or `::1`, or a wildcard listener such as `0.0.0.0` or `::` uses `http://localhost:{port}`. The environment template sets `APP_URL="http://${APP_HOST}:${APP_PORT}"`, which expands to `http://127.0.0.1:8000` with its default settings.

HTTP URL generation, including `Url::for_request(request)`, uses the explicit `APP_URL` first. With no explicit URL, it uses the current request origin, then the listener-derived `app.url` if the request has no origin. Artisan uses `app.url` directly.

Set `APP_VERSION` to override the application version stored in `app.version`. When `APP_VERSION` is unset, the version defaults to the Cargo package version captured at compile time by `env!("CARGO_PKG_VERSION")`. The welcome page displays `app.version`.

## Framework and Application Versions {#framework-version}

`Application::VERSION` and `application.version()` return the framework package version, embedded from `vendor/openrust-framework/Cargo.toml` at compile time. Within an application scope, `App::version()?` resolves the current Application and returns the same value; outside that scope it returns a container binding error. The constant and instance method do not require booting configuration.

```rust
use openrust::{foundation::Application, support::facades::App};

let framework_version = Application::VERSION;
let current_framework_version = App::version()?;
```

`./artisan --version` reports this framework version using the existing `artisan <version>` format. By contrast, `Config::get("app.version")?` reads the application's version. It defaults to the application package version and can be overridden with `APP_VERSION`; the welcome page continues to display it. Changing the application package version, `app.version` or `APP_VERSION` never changes the framework version. Rebuild after updating the framework package to embed its new version.

## Current configuration repository

Configuration becomes available during boot, before providers register. As in LoadConfiguration, boot binds a new repository before evaluating the configuration loader, then fills that repository. `create()` does not preload an empty configuration service. `Application::config()` returns `Result<Arc<dyn Repository>, BindingResolutionError>` by resolving the current `config` service each time; access before boot or after an incompatible replacement returns an error.

Inside an application scope, import `openrust::support::facades::Config` and call `Config::get("app.name")?`. The facade resolves the same application repository on each call. It returns `None` for a missing key and `Some(Value::Null)` for an explicit null. Calling it outside an application scope or before the repository is registered returns a binding error.

`config` is the canonical name; `dyn openrust::contracts::config::Repository` is its typed alias. To replace the application repository, construct `openrust::config::Repository::new(values)` and call `instance_named::<dyn Repository>("config", repository)?`. Consumers already holding an `Arc` keep that snapshot; resolve again or use a rebinding/refresh listener when they must follow replacements. Rebinding the typed alias as a raw key replaces that alias rather than the canonical configuration service, matching the container's raw-key rules.

Provider replacements are used by HTTP listener binding, route/static-disk assembly and view-path setup. Health HTML reads the current repository on each request, and Artisan uses the same application accessor. Already assembled routes and view finders are not automatically rebuilt after a runtime configuration replacement. Container errors propagate through the relevant bootstrap, HTTP or Artisan error path. See [rebinding](../architecture-concepts/container.md#rebinding-and-refreshing-consumers).

Numeric and boolean settings are validated at startup. Booleans accept `true`, `false`, `1`, and `0`. Nullable settings such as `SESSION_DOMAIN` accept `null`. Each driver still requires the corresponding framework service to be available.

`APP_PORT` is checked when HTTP binds its listener. Artisan does not require a valid HTTP listener address. Port `0` lets the operating system select a listener port; set `APP_URL` explicitly when URLs must use a stable public address.

The HTTP view renderer searches the directories in `view.paths` in order. By default it reads HTML files from `resources/views`. Relative directory entries are resolved from the application root.

Process environment variables take precedence over values in the file. Set `APP_ENV` in the process environment to select a file such as `.env.local`. If the selected file is absent, the application reads `.env`. See [application root resolution](../architecture-concepts/lifecycle.md) for deployment paths and the `APP_BASE_PATH` override.

## Accessing Configuration Values {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::string_or("app.name", "App")
}
```

Use `Config::set` to change a value in the current application. The `string`, `integer`, `float`, `boolean`, `array`, and `collection` methods check the value type. `array` accepts JSON lists and objects, and `collection` preserves their keys.

Use the [App locale methods](../digging-deeper/localization.md#configuring-the-locale) to update configuration, the translator, and locale-change events together. Within a request or queue task, changes to `app.locale` and `app.fallback_locale` affect only that scope. Replacing the entire `app` group also stays in that scope and clears earlier child-key overrides. Writing `Config` directly does not synchronize an already resolved translator.

After replacing `app`, subsequent writes follow call order: replacing `app.ui` removes earlier children, and writing `app.ui.title` afterward adds that child to the new parent. Array-index writes preserve sibling entries. `get`, `all`, `has`, `push`, and `prepend` observe the same scoped value; a sparse array becomes a keyed JSON object. Nested calls within the same lifecycle share these changes; another request or task and the application defaults remain isolated. `push` initializes a currently null configuration entry as a list. Both list methods accept associative arrays: appending preserves keys and prepending renumbers integer keys while retaining named keys. `prepend` on null and either operation on other scalar values return an error.

Use `Config::string_or("app.name", "App")` to get a `String` directly. It returns the default if the key is absent, the value is null or another type, or the repository is unavailable. Use `Config::string("app.name")?` for strict reads. Raw `Config::get_or("app.name", Value::from("App"))?` defaults only when the key is absent and preserves an explicit null. `Config::get_many([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` reads keys with individual defaults in input order. To read an entire section as a struct, derive `serde::Deserialize` and use `Config::deserialize::<MailOptions>("mail")?`.

The strict readers also provide `*_with_default` and `*_or_else`. Defaults are used only when the key is absent; an explicit null or the wrong stored type still returns an error. Lazy defaults run once at most, after the lookup. Integers and floats keep their distinct types. An unavailable repository returns a binding error before calling a default.

```rust
use openrust::config::ConfigKey;
let name = Config::string_with_default("app.name", "App")?;
let retries = Config::integer_with_default("services.retries", 3)?;
let ratio = Config::float_or_else("services.ratio", || 1.5)?;
let selected = Config::get_many(["app.name", "app.locale"])?;
let custom = Config::get_many([
    ConfigKey::from("app.name"),
    ConfigKey::lazy("services.label", || Value::from("fallback")),
])?;
Config::set_many([("services.enabled", Value::Bool(true))])?;
```

All six strict readers have these defaults. String defaults accept `&str` through the facade; array and collection defaults accept JSON arrays or objects. `get_many` also keeps the existing `(key, default)` form. It preserves the first insertion position of duplicate keys and uses their last value. An existing null does not run a lazy default. Reads follow input order, including changes made by a previous default callback. `set_many` writes in order without a transaction. A complete literal key takes precedence over a dot-separated path.

`openrust::config::Repository` exposes the same readers and bulk operations. The configuration contract provides default implementations so replacing the repository preserves the public entry points; facade calls also respect typed-reader overrides.


The named ArrayAccess methods `offset_exists`, `offset_get`, `offset_set`, and `offset_unset` forward to the current repository, including custom overrides. `offset_unset("key")?` assigns null: the key remains present, `offset_exists` returns true, and `offset_get` returns `Some(Value::Null)`. It also creates a null entry when the key was absent. String keys may be empty or contain empty dot segments or literal `*`; the asterisk is a key, not a wildcard. These methods follow the same request/task locale isolation as `set`. Rust uses named methods because a synchronized repository cannot expose stable mutable indexing references.

## Configuration Macros {#configuration-macros}

Register a macro during application boot, then call it through the public facade:

```rust
Config::r#macro("feature", |repository, key: String| {
    repository.boolean(&format!("features.{key}"))
})?;
let enabled = Config::macro_call::<bool, _>("feature", "search".to_owned())?;
```

The callback receives the current repository, including a swapped repository and its overridden readers. `has_macro` checks registration and `flush_macros` clears it. `mixin([(name, ConfigMacro::new(callback))], true)` replaces existing macros; pass false to keep them. `mixin_using` accepts ordered factories: with false, an existing name skips the factory entirely. A factory error retains earlier registrations and stops later factories. Callbacks and retired captures run outside the registry lock, so they can register, call, or clear macros again.

Macros belong to an application, survive replacing its default repository, and are shared by its HTTP, command, and Worker consumers. Different applications have independent registrations. A standalone `Repository::new` outside any application owns its registry. A custom repository can override the object-safe registration, lookup, invocation, or `macro_registry` contract. Ordinary methods such as `Config::get` remain ordinary methods even if a macro has the same name; `macro_call` explicitly invokes the named macro.

Stored callbacks must be `Send + Sync + 'static`; the arguments and result of a synchronous call only require `Any` and may carry non-cloneable or `Rc` values. Missing macros, argument/result type mismatches, and callback failures return explicit errors. This typed invocation replaces dynamic method syntax; no root-object access is needed.

## Editing Environment Files {#editing-environment-files}

`EnvironmentFile` edits the application's environment file on disk. `Config::set` changes the current configuration repository; the `Environment` boot snapshot remains immutable. File edits do not automatically change either one or restart services.

```rust
use openrust::config::EnvironmentFile;

// After application bootstrap: uses the actual selected .env or .env.<environment>.
let file = EnvironmentFile::for_application(&app)?;
// Or open an existing file explicitly; create(path, content) never overwrites one.
// let file = EnvironmentFile::open(project_root.join(".env"))?;
file.set("APP_NAME", "My application")?;
file.set_many([("MAIL_HOST", "smtp.example.test"), ("MAIL_PORT", "587")])?;
let name = file.get("APP_NAME")?;
let exists = file.contains("MAIL_HOST")?;
let raw = file.get_raw("APP_NAME")?; // token including quotes, without expansion
let values = file.all()?;
file.remove("OBSOLETE_SETTING")?;
```

Use an independent editor to commit several kinds of change together:

```rust
let mut editor = file.edit()?;
editor.set("APP_NAME", "My application")?
    .set_expression("MAIL_FROM_NAME", "${APP_NAME}")?
    .remove("OBSOLETE_SETTING")?;
let changed = editor.save()?;
```

`set` writes a literal string, escaping dollar signs, quotes and backslashes. Use `set_expression` explicitly for dotenv variable expansion; shell commands are never executed. `get` and `all` parse with the application's dotenv loader, including its process-environment fallback when expanding references. They return strings: `null` and `false` remain text. `get_raw` returns the original quoted token. Duplicate definitions of a targeted key are rejected; `all` follows the loader's last-definition rule for duplicates. Unchanged variables keep their original bytes, including expressions, comments, `export`, spacing, UTF-8 BOM and line endings.

Simple writes acquire a per-file lock before reading; `save` detects a changed editor snapshot and returns `Conflict`. `Busy` means another writer holds the lock: retry after it finishes. Batch failure leaves the file unchanged. No-op saves preserve modification time. Replacement uses a temporary file in the same directory, preserves permissions and Unix ownership, and syncs on Unix; Windows uses `ReplaceFileW` to preserve the target ACL. New files are private to the owner on Unix; Windows follows directory ACLs. The stable `<filename>.lock` sidecar must remain in place while writers may run. Do not delete it during normal cleanup.

Files must be regular UTF-8 files, at most 8 MiB; target symbolic links and Windows reparse points are rejected. New variable names use ASCII letters, digits, underscores and dots, beginning with a letter or underscore. Read-only files and malformed documents fail without replacement. Cooperating writers are serialized; tools that bypass the sidecar lock can still race with a save. A `DurabilityUncertain` error means replacement completed but directory synchronization failed: inspect disk state before retrying. A Windows `ReplacementUncertain` error preserves a recovery file and reports its path; inspect both files before retrying. Errors and editor debug output omit values; `content`, `all` and getter results contain plaintext and must not be logged indiscriminately.

After editing production configuration, explicitly rebuild the configuration cache (`artisan config:cache`) and restart long-running HTTP, queue and scheduler processes. Without a cache, a newly booted application reads the changed file. Existing snapshots and process environment overrides retain their original precedence. `key:generate` and `init` share this file-writing backend while keeping their own key validation, production confirmation and initialization rules.

### Position and Spacing {#environment-file-layout}

```rust
use openrust::config::{EnvironmentDocument, EnvironmentWriteOptions};

let options = EnvironmentWriteOptions::new()
    .after("DB_*")
    .blank_lines(1);
file.set_with_options("SMS_DRIVER", "log", &options)?;
```

Use `before(key)`, `after(key)`, `at_start()` or `at_end()`. `blank_lines(1)` puts one empty line on each side; `blank_lines_before(n)` and `blank_lines_after(n)` control them separately. Zero means no additional empty line; the line break separating assignments still exists. Unspecified spacing preserves existing whitespace. Explicit counts normalize only adjacent blank lines, leaving comments untouched. File boundaries can have explicitly requested leading or trailing blank lines. LF/CRLF and the UTF-8 BOM are retained.

Existing variables are updated in place, with explicit spacing still applied. Add `.move_existing(true)` to move their declarations too. Position anchors are checked for insertion and explicit moves: missing or duplicate anchors fail without changing the document; positioning a variable relative to itself fails. Moves retain `export` and inline comments, while standalone comments stay in their original position. The operation is idempotent, so repeating it does not accumulate empty lines. Moving declarations can change variable-expansion order. Use `set_expression_with_options` for an explicit expression, or perform several edits in a file editing session before saving once.

Anchors also accept `*` for zero or more characters, matching the whole key with case sensitivity. For example, `after("DB_*")` inserts after the last matching declaration in file order; `before("DB_*")` inserts before the first. Prefix, suffix and multiple-star patterns are supported. Dots are literal: `DB.*` matches names such as `DB.host`. These are not regular expressions; `?` and character classes (`[]`) are not supported. Comments, values and text inside multiline values are not anchors.

When explicitly moving a variable, wildcard matching excludes that variable itself, so repeated moves remain stable. If nothing else matches, the operation returns `MissingAnchor` without a change. Duplicate definitions of a matched key return `DuplicateKey`; multiple different matched keys are a normal group. Existing variables still update in place unless `move_existing(true)` is enabled.

During a known Windows replacement window, file reads wait up to 100 ms for the writer; a still-held lock returns `Busy`. A genuinely missing file still fails.

The same engine is available without an application, container or disk file:

```rust
let mut document = EnvironmentDocument::parse("APP_NAME=Example\n")?;
document.set_with_options(
    "SMS_DRIVER", "log",
    &EnvironmentWriteOptions::new().after("APP_NAME").blank_lines(1),
)?;
let output = document.content();
```

`EnvironmentDocument` supports the same reads, literal/expression writes, batch updates and removals, plus `new()` for an empty document. It changes only its own text. Persist `content()` through your own storage adapter and concurrency policy, or use `EnvironmentFile::open(path)` for the built-in locked atomic file writer. `for_application` is an optional convenience; neither a fixed `.env` filename nor an application container is required.

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
        .with_config(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().current_environment_is(&["local", "staging"])?));
    assert!(!bool::from(app.container().current_environment_is(&[])?));
    Ok(())
}
```

During bootstrap the detected name is stored in the `env` service as `Arc<str>`. The environment query reads this service. Editing `app.env` after boot edits configuration only.

<a id="public-files"></a>

## Public files

| Environment setting | Purpose | Default |
|---|---|---|
| `APP_PUBLIC_PATH` | `filesystems.disks.web.root` / `Application::public_path()` | `public` |
| `FILESYSTEM_WEB_ROOT` | Website disk root; overrides `APP_PUBLIC_PATH` | `app.public_path` |
| `FILESYSTEM_WEB_URL` | `filesystems.disks.web.url` | `app.url` |
| `FILESYSTEM_LOCAL_ROOT` | `filesystems.disks.local.root` for the private local disk | `storage/app/private` |
| `FILESYSTEM_PUBLIC_ROOT` | `filesystems.disks.public.root` for the public local disk | `storage/app/public` |
| `FILESYSTEM_PUBLIC_URL` | `filesystems.disks.public.url` | `app.url` with `/storage` appended |

`FILESYSTEM_WEB_ROOT` overrides the website disk root; when unset, it uses `APP_PUBLIC_PATH` (default `public`). Relative paths are resolved from the application root; absolute paths are used directly. Empty or whitespace-only values fail configuration loading.

When `APP_PUBLIC_PATH` is omitted, `config/app.rs` explicitly sets `app.public_path` to `public`. The `web` disk reuses that value as its default root. Configure website files under `filesystems.disks.web` and uploaded public files under `filesystems.disks.public`. The default routes mount them in that order using `Route::static_disk("web")` and `Route::static_disk("public")`. Calling `use_public_path()` before boot overrides the website directory and updates `web.root`. Relative paths use the application root. A disk mount uses the full path of its URL, so `/storage/tmp/` mounts at `/storage/tmp`. Absolute URLs must have the same origin as `app.url`. Restart the application after changing mount settings. An explicitly empty or whitespace-only `APP_PUBLIC_PATH` fails configuration loading.

## Maintenance mode {#maintenance-mode}

Run Artisan `down` to return HTTP 503 while you deploy. Use `up` to reopen the application. A running server reads the shared state on each request. You can run `down` again to update the options.

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

Use `--retry` for a positive delay in seconds or a retry date, and `--refresh` to tell a browser when to reload. Local date-times use the application timezone and are sent in GMT. `--redirect` takes a local path. `--render` prerenders a named view once when you run `down`, passing it `$retryAfter`. `--status` overrides the default 503. Requests that expect JSON follow the usual exception renderer and skip the browser redirect and prerendered view.

To check the site during maintenance, open the URL printed by `--with-secret`. The application sets a signed browser cookie valid for 12 hours and redirects to the home page when no intended session URL is available. Use `--secret` to supply your own path. The configured health path (default `/up`) stays accessible during maintenance mode. Use `with_middleware` in `bootstrap/app.rs` to leave additional endpoints available.

```rust
.with_middleware(|middleware| {
    middleware.prevent_requests_during_maintenance(["status/*"]);
})
```

For a file-backed deployment, `./artisan down --render=errors::503` serves the prepared HTML while providers are still booting. Put a custom page at `resources/views/errors/503.blade.html`. The listener settings must load successfully. JSON, excluded paths, and valid bypass requests wait for the application kernel. Cache-backed maintenance uses the running kernel. See [workers during maintenance](../digging-deeper/queues.md#workers-during-maintenance) for the queue behavior.

During startup, use the exact bypass URL printed by Artisan. The early handler compares the request target including its query string and trailing slash. After the HTTP kernel starts, maintenance middleware compares the path without its query string or surrounding slashes.

### Shared maintenance state {#shared-maintenance-state}

The application exposes maintenance state through `maintenance_mode()`. The default `file` driver stores the payload in `storage/framework/down`. Choose `cache` and a shared cache store to make the state visible to workers on multiple hosts. The `array` driver keeps state inside a single application instance for testing.

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenance_mode()?.activate(&payload).await?;
    Ok(())
}
```

Read the state with `is_down_for_maintenance().await` and clear it with `app.maintenance_mode()?.deactivate().await?`. Queue workers read this state before reserving jobs.

### Maintenance facade and drivers {#maintenance-facade}

`MaintenanceMode` resolves the same application manager used by HTTP, Artisan and the scheduler. `get_default_driver` reads `app.maintenance.driver`, defaulting to `file` when absent. `driver(None)` selects it; `driver(Some("file"))`, `driver(Some("array"))` and `driver(Some("cache"))` select built-in drivers explicitly. Resolved drivers are shared within an application, while applications remain isolated.

```rust
use openrust::{foundation::maintenance::MaintenanceError, support::facades::MaintenanceMode};
use serde_json::{Map, json};

pub async fn update_maintenance() -> Result<(), MaintenanceError> {
    MaintenanceMode::activate(&Map::from_iter([("status".into(), json!(503))])).await?;
    assert!(MaintenanceMode::active().await?);
    let _payload = MaintenanceMode::data().await?;
    MaintenanceMode::deactivate().await
}
```

`extend(name, creator)` registers a creator receiving `&Container` and returning `Arc<dyn contracts::foundation::MaintenanceMode>`. Existing resolved drivers stay in use until `forget_drivers`. The creator executes without a registry lock, may resolve another driver, and runs once for concurrent resolutions. Circular dependencies return `MaintenanceError::DriverResolutionCycle`. `get_drivers` returns resolved handles; old handles survive cache clearing. Subsequent HTTP requests and command/scheduler lookups resolve the current binding, including an explicitly replaced MaintenanceMode contract.

`get_container` returns the owning container. `set_container(&Arc<Container>)` affects future creation and retains the manager's captured configuration, resolved drivers and creators. It uses weak ownership; access after that application is dropped returns an error. These are lifecycle controls for embedded applications, rather than per-request configuration.

The cache driver captures `app.maintenance.store`, falling back to `cache.default`, and resolves that named repository for each operation. It follows repository replacement and uses `put_with_default`: initially 3600 seconds, or indefinitely when that repository's default lifetime is `None`. File maintenance stays active until explicitly cleared. Missing state is represented by `data() == None`; backend, decoding and rejected-write errors propagate. The `array` driver is local to the application process, so it cannot coordinate separate server or Worker processes.

## Testing Environment and Bootstrap Snapshot {#testing-environment}

`Application::running_unit_tests()` and the application contract return `Result<bool, BindingResolutionError>`. The result is true only when the bound `env` is exactly `testing`, regardless of whether a test harness is running. Before the environment is bound it returns `Ok(false)` without resolving a service. Resolution errors propagate. Changing `config("app.env")` later does not replace the detected environment binding.

After boot, use `app.make::<openrust::config::Environment>().await?` to read the immutable environment snapshot for that application. Existing `get` and `get_utf8` methods return raw strings; package-specific flag parsing remains the package's responsibility. The snapshot follows process precedence, environment-file selection and configuration-cache rules. It neither reloads files nor mutates process-global variables.

## Application Storage Path {#application-storage-path}

`app.storage_path("")` returns the storage root; pass a string such as `"pail"` for a subpath. `app.use_storage_path(path)?` overrides it and rebinds `path.storage`. The getter is also in the application contract. `openrust::support::storage_path(path)?` reads the current application scope. The readonly `foundation::Paths` service shares the same state with compiled packages, without retaining Application through its container.

Precedence is a nonempty override other than `0`, then `APP_STORAGE_PATH` from the application environment snapshot, then the application's `storage` directory. An explicitly present empty environment value is retained. Setter values stay relative when supplied as relative paths. Joining appends the native separator and strips leading native separators from the subpath; it preserves duplicate separators and `..`. There is no default Rust argument, so pass `""` for the root.

The constructor initializes `path.storage`; a later environment-file value changes the getter without automatically rewriting that binding. Calling `use_storage_path` explicitly rebinds it, following order. Rebinding errors return `Result` after updating path state.

Logging, file sessions, file cache data and locks, compiled views, and default local/public disks use the storage root when configuration loads. `FILESYSTEM_LOCAL_ROOT`, `FILESYSTEM_PUBLIC_ROOT`, and `VIEW_COMPILED_PATH` retain their own overrides; the public URL and mount order are unchanged. `storage:link` targets the configured public disk. Set an absolute `APP_STORAGE_PATH` before startup, or call `use_storage_path` before configuration loads.

The compiled configuration loader uses `env.storage_path(base_path, subpath)` as its Rust context adapter. Its weak reference selects the owning application's paths without using a global Facade or retaining the application. Standalone environment snapshots resolve their own variable against the supplied default base. Later changes to the application path do not rewrite already loaded or cached configuration values.

File maintenance resolves the current path on each operation, including after the service was resolved. The early prerender check uses paths available before boot: provide a process variable or explicit setter for this check; a dotenv-only value is not available yet. Pail fixes its directory on first Files resolution, so configure the path before that use. Configuration, route, event and package cache artifact locations keep their separate cache-path contracts.

When upgrading an existing installation, follow [Upgrading locale identifiers](../digging-deeper/localization.md#bcp47-upgrade) to migrate language resources, stored values and caches together.
