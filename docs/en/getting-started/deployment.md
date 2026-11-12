# Deployment

## Build the Application

Use Rust 1.94 or later and the checked-in lockfile. Build on the target operating system and architecture, or use a separately verified cross-compilation toolchain.

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.html release/resources/views/welcome.html
```

Copy any website assets into `release/public` and public storage files into `release/storage/app/public`. The welcome page is read at runtime, so deploy it with the binary. Rust configuration and route declarations are compiled into `main`; changing their Rust source requires a rebuild. Runtime environment values and view files do not.

The `standalone` feature resolves the application root from the executable's directory, independently of the working directory. An absolute `APP_BASE_PATH` overrides that location. Configure process environment variables or place a `.env` file in the application root. Process environment values take precedence. Never publish production secrets or copy `.env.example` unchanged into production.

## Prepare the Environment

Set production environment values before starting the application. Use `APP_ENV=production`, `APP_DEBUG=false` and your production `APP_KEY`. Give the service account write access to `storage/` and any local database directory. Keep persistent storage and databases outside a release that will be replaced, using absolute paths in deployment configuration where required.

Run migrations from the new release before sending traffic to it:

```sh
./main artisan migrate --force
```

For a fresh SQLite deployment, create the configured database file before running migrations. Existing deployments reuse their application key and persistent data. See [Migrations](../database/migrations.md).

## Start HTTP and Artisan

```sh
cd release
APP_HOST=127.0.0.1 APP_PORT=8088 ./main
```

From another terminal in the release directory:

```sh
./main artisan list
./main artisan help inspire
./main artisan inspire
curl -i http://127.0.0.1:8088/up
```

Set `APP_URL` to the application's public URL. Without it, the configured `APP_PORT` supplies the port in the default local URL. If the public disk uses an absolute URL, it must have the same origin as `APP_URL`. The HTTP listener validates its port before binding; Artisan does not need a valid HTTP port or an HTTP listener.

`SIGINT` and, on Unix, `SIGTERM` request graceful HTTP shutdown. Use a process manager to restart the application. See [Release Notes](../prologue/releases.md) for the verified platforms and drivers.

The source `artisan` script runs Cargo. A standalone deployment does not require Cargo and runs commands with `./main artisan …`. See [Artisan](../digging-deeper/artisan.md) and [application configuration](configuration.md).

## Run Services Together

Run `./main` to start HTTP, the Worker for the default asynchronous queue, and the scheduler when tasks are registered. Connections using the `sync` driver skip the Worker. Other listeners such as gRPC follow their own configuration. Prepare the queue's backend before starting the application. For a database queue, run the migrations above.

Set `APP_DISABLED_RUNNERS=queue,schedule` to skip the queue and scheduler. The comma-separated list populates `app.disabled_runners`. An empty list allows every eligible Runner. Built-in signatures are `http`, `queue`, `schedule`, `grpc`, and `reverb`. A signature can also be matched by a glob such as `q*`. Patterns support `*`, `?`, character classes such as `[a-z]` and `[^a-z]`, and backslash escapes. `*` and `?` do not cross `/`. The application warns and skips invalid patterns. If every Runner is filtered out or ineligible, startup exits successfully.

A Provider supplies runners through `ServiceProvider::runners`. The application can add custom runners with `withRunners(|| vec![...])`. If called more than once, the last callback supplies the custom list. The first registration for each signature wins, with Provider contributions before the custom list. The application applies the disabled list before evaluating `shouldRun`. Eligible runners run concurrently in the application process.

On Ctrl+C, Unix SIGTERM, or a Runner failure, the application shuts down runners in ascending `shutdownPriority` order, concurrently within each group. The default priority is zero. `run` receives a `RunnerContext` cancellation token so it can finish its work. A custom `shutdown` hook can release other resources. A successful return from `run` can mean that a Runner started its own background work and stays eligible for shutdown.

New long-running services join application startup by default once they are registered, configured, and ready to start. They can still be disabled or run independently. One-off Artisan commands run only when explicitly invoked.

```sh
./main artisan queue:work
./main artisan schedule:work
./main artisan schedule:run
```
HTTP, the managed Worker, and the scheduler share the application process. The managed Worker uses the same timeout, retries, and exit codes as independent `queue:work`. It skips the Worker memory limit because process RSS includes all three services. Independent `queue:work` retains its memory limit. Jobs configured for hard timeouts still run in isolated executors. Ctrl+C or Unix SIGTERM stops accepting new work and waits for active requests, queue jobs, and scheduled tasks. The application propagates a nonzero Worker exit code.
