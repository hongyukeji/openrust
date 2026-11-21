# Deployment

## Build the Application

Use Rust 1.94 or later and the checked-in lockfile. Build on the target operating system and architecture, or use a separately verified cross-compilation toolchain.

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.blade.html release/resources/views/welcome.blade.html
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

For MySQL or PostgreSQL, confirm the server version against the [release matrix](../prologue/releases.md). OpenRust requires TLS for MySQL connections. The application starts a database queue worker by default. Keep the queue tables migrated when using that worker.

## Start HTTP and Artisan

Start the application with the following command after preparing the environment and running any required migrations. HTTP and Artisan can run without pre-generated caches.

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

### Reload Services {#reloading-services}

After deploying a new version, request a graceful reload of independently managed services:

```sh
./main artisan reload
./main artisan reload --except=queue,reverb:restart
```

`reload` runs `queue:restart` and `schedule:interrupt`. The Reverb provider adds `reverb:restart` for standalone Reverb servers. Providers can register other tasks with `reloads`. Use `--except` (`-e`) with comma-separated task labels or Artisan command names.

Run the command with the same shared cache configuration as the workers. A process manager should restart standalone workers and Reverb after they exit. Restart the unified `main` process to replace its compiled code and restart its embedded services.

### Verify the Release {#verify-the-release}

Run these checks from the release directory while the application is listening. Use the HTTP port configured for the deployment.

```sh
curl --fail http://127.0.0.1:8088/up
./main artisan migrate:status
./main artisan schedule:list --json
```

The migration status identifies pending migrations. The schedule list returns a JSON array of registered tasks, including an empty array when there are none.

## Optional Deployment Optimizations

Use these commands if you want to pre-generate deployment caches.

### Cache Configuration {#cache-configuration}

After setting the deployment environment, you can cache configuration before starting the services:

```sh
./main artisan config:cache
```

Run the command from the final release directory using the environment the services will run under. The snapshot can contain absolute paths. If the release moves or its settings change, rebuild the cache in the new environment before restarting. Run `./main artisan config:clear` to load configuration from the environment at startup. See [Configuration Caching](configuration.md#configuration-caching).

### Cache Deployment Resources {#cache-deployment-resources}

```sh
./main artisan optimize
./main artisan optimize:clear --except=cache
```

`optimize` caches configuration, event discovery, the route matching index and compiled views. `optimize:clear` clears those artifacts, the package manifest and the default application cache. Use `--except` (`-e`) with comma-separated task names or Artisan command names. The example keeps the default application cache intact. Service providers can add tasks with `optimizes`.

If you override the route, event or package cache path, export `APP_ROUTES_CACHE`, `APP_EVENTS_CACHE` or `APP_PACKAGES_CACHE` in the service environment and in the shell that runs Artisan. Once configuration is cached, startup can skip reading `.env`. Use the same paths when generating, reading and clearing these files. The compiled view directory comes from the configuration snapshot.

See [Route Caching](../the-basics/routing.md#route-caching), [Event Discovery](../digging-deeper/events.md#event-discovery-in-production) and [Package Discovery](../digging-deeper/packages.md).

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

## Windows standalone deployment {#windows-standalone}

Build the release with Rust and the MSVC build tools for the target architecture. On a fresh SQLite deployment, stage the executable, resources, public files, and environment before creating the database and applying migrations.

```powershell
cargo +1.94.0 build --locked --release --features standalone
New-Item -ItemType Directory -Path release, release/database, release/storage/app/public
Copy-Item target/release/main.exe release/
Copy-Item resources, public release/ -Recurse
Copy-Item .env.example release/.env
New-Item -ItemType File release/database/database.sqlite
$env:APP_ENV = 'production'
$env:APP_DEBUG = 'false'
$app = Join-Path $PWD 'release/main.exe'
& $app artisan key:generate --force
& $app artisan migrate --force
& $app artisan inspire
& $app
```

For subsequent releases, preserve the application key, database, and writable storage. A service manager can run `main.exe` from another working directory. `APP_BASE_PATH` can select an absolute application root.

## TLS termination {#tls-termination}

Bind the application to a private address and terminate TLS at your reverse proxy or load balancer. Set `APP_URL` to the public HTTPS origin and configure the proxy to pass the public host and scheme. Verify the certificate chain and hostname from a client and check both the welcome page and health route over HTTPS.


## Docker deployment {#docker-deployment}

Use Docker Engine with Compose and Buildx on Linux, or Docker Desktop in Linux-container mode on macOS or Windows. The same Dockerfile builds Linux `amd64` and `arm64` images. Run the commands below from the repository root. On Windows PowerShell use `Copy-Item docker/.env.example docker/.env` for the first step.

On Windows ARM64 the Docker Desktop backend requires working WSL 2. A Windows virtual machine needs nested virtualization exposed by its hypervisor. See the [Docker Desktop requirements](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements).

For local development, start MySQL 8.4 and Redis and run the app on the host. Set the application's root `.env` to `DB_CONNECTION=mysql`, `DB_HOST=127.0.0.1`, `DB_PORT=33060`, `REDIS_HOST=127.0.0.1`, and `REDIS_PORT=16379`. Use the same MySQL database, username and password as `docker/.env`. Set `CACHE_STORE=redis` and `QUEUE_CONNECTION=redis` to use Redis. Generate an application key if it is empty, then run `./artisan migrate` and `cargo run`. On Windows PowerShell use `.\artisan.bat`.

For a containerized application on a single host:

1. Copy the template below. For production set `APP_ENV=production`, `APP_DEBUG=false`, a strong `DB_PASSWORD`, and the public `APP_URL` before initializing MySQL. Keep `docker/.env` private.
2. Start the backing services and build the application image. Obtain an application key using the container's Artisan and save it as `APP_KEY` in `docker/.env`. Keep the same key on upgrades and across replicas.

```sh
cp docker/.env.example docker/.env
docker compose --env-file docker/.env -f docker/docker-compose.yml --profile local up -d --wait mysql redis
docker compose --env-file docker/.env -f docker/docker-compose.yml build app
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan key:generate --show
```

3. The app runs as UID/GID 10001. Grant that identity write access to `docker/data/storage`. On Linux, for a fresh directory, use `sudo chown 10001:10001 docker/data/storage`. On Docker Desktop the directory must be shared with Docker and writable in the container. Run the migration once, then start the app.

```sh
docker compose --env-file docker/.env -f docker/docker-compose.yml run --rm app artisan migrate --force
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --wait app
```

Open `http://localhost:8000/up` or use the configured `APP_PUBLISHED_PORT`. The host port defaults to loopback for use with a reverse proxy. MySQL, Redis and application storage default to relative host directories under `docker/data/`. To bind-mount static files read-only, add `-f docker/docker-compose.public.yml` to the Compose command. The default source is the repository's `public/`. With that override the host directory supplies the complete contents served at `public/`.

The image sets `APP_PACKAGES_CACHE=storage/framework/cache/packages.json`, within the mounted storage. `package:discover` and `clear-compiled` use that path across one-off containers. If you override it, choose a writable path in the same mount.

For multiple hosts, deploy the same image and omit the `local` profile. Set `OPENRUST_IMAGE`, shared `DB_HOST` and `REDIS_HOST`, and matching application key, cache/queue settings and credentials. Deploy with `up -d --no-build --wait app` after pulling or loading that image. Run migration once per release and use a load balancer with TLS and shared storage (such as S3) for uploads. If only one instance should run the scheduler, set `APP_DISABLED_RUNNERS=schedule` on the others. For optional Cargo features such as `reverb` or `sanctum`, set `APP_BUILD_FEATURES` at build time.
