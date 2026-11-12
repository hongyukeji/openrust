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

By default, `./main` starts HTTP and the listeners enabled for it. Run background services independently with `./main artisan queue:work` and `./main artisan schedule:work` or `./main artisan schedule:run`.

To run HTTP, the Worker, and the scheduler with one invocation, set these values in the application environment and start `./main`:

```dotenv
RUNTIME_ENABLED=true
RUNTIME_HTTP=true
RUNTIME_QUEUE=true
RUNTIME_SCHEDULE=true
```

With unified startup enabled, each service switch defaults to `true`. Set a switch to `false` to skip that service. The Worker uses the default queue connection and queue, and skips connections using the `sync` driver. The scheduler starts only when the application registers tasks. Other configured listeners, such as gRPC, still use their own settings. Startup fails if there is no service to run.

Ctrl+C, or SIGTERM on Unix, stops accepting HTTP requests and new queue or schedule work. The process waits for active work to finish. If a managed service exits, the process also shuts down the remaining services. A Worker configured to exit on job timeout passes its timeout exit code to the application process. The managed Worker's memory check is disabled because it shares the process with the other services. Use an independent Worker when you need a separate memory limit, restart policy, or command options. Jobs that run blocking code in the shared runtime can delay HTTP and scheduling. Keep such jobs in a separate Worker or use the queue’s isolated hard-timeout execution.
