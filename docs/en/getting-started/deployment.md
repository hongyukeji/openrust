# Deployment

## Build the Application

Use Rust 1.94 or later and the checked-in lockfile. Build on the target operating system and architecture, or use a separately verified cross-compilation toolchain.

```sh
cargo +1.94.0 build --locked --release --features standalone
mkdir -p release/resources/views release/public release/storage/app/public
cp target/release/main release/main
cp resources/views/welcome.blade.html release/resources/views/welcome.blade.html
```

Default translations are compiled into the binary. If your application supplies custom translations in `lang/`, deploy those files with the application.

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

### Complete setup with existing MySQL and Redis

For a new application, supply MySQL and Redis connection settings and run `./artisan init:full` once from the source directory. On Windows use `artisan.bat init:full`. The command builds the application, runs isolated migrations and checks HTTP, Artisan and a Redis worker. See [installation](installation.md#complete-initialization) for the configuration and preview commands.

Use the resulting application for a single instance or for multiple replicas. All processes in a deployment must share MySQL, Redis, the Redis key prefixes and `APP_KEY`. Configure shared storage for files that replicas must access. Coordinate migrations and scheduling when rolling out replicas, and configure load balancing and TLS at the deployment layer.

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

A Provider supplies runners through `ServiceProvider::runners`. The application can add custom runners with `with_runners(|| vec![...])`. If called more than once, the last callback supplies the custom list. The first registration for each signature wins, with Provider contributions before the custom list. The application applies the disabled list before evaluating `should_run`. Eligible runners run concurrently in the application process.

On Ctrl+C, Unix SIGTERM, or a Runner failure, the application shuts down runners in ascending `shutdown_priority` order, concurrently within each group. The default priority is zero. `run` receives a `RunnerContext` cancellation token so it can finish its work. A custom `shutdown` hook can release other resources. A successful return from `run` can mean that a Runner started its own background work and stays eligible for shutdown.

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

Use Docker Engine with Compose and Buildx on Linux, or Docker Desktop in Linux-container mode on macOS and Windows. The Dockerfile supports Linux `amd64` and `arm64`. Run these commands from the repository root. Use the single `docker/docker-compose.yml` and root `.env`. If `.env` does not exist, copy `.env.example` to `.env` (PowerShell: `Copy-Item .env.example .env`); edit an existing file in place.

Use native Docker Compose without a startup wrapper. Only the repository-root `.env` is loaded. The image is `${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`; use a valid lowercase repository name and set `APP_IMAGE_VERSION` for a release tag, otherwise `latest` is used. Compose does not read Cargo; the application version still uses its compiled Cargo default. Each development machine or deployment server maintains its own root `.env`; actual environment files stay ignored by Git.

Docker-specific examples are in `docker/.env.example`. Merge the settings you need into the root `.env`; this is a reference template, not another runtime environment file. Compose loads only the root `.env`.

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait postgres mysql redis
```

Run commands from the repository root with `--env-file .env`. Shell values retain native Compose interpolation precedence. The application loads this same file; explicit container settings keep the internal listener at `0.0.0.0:8000`, while `APP_HOST` and `APP_PORT` set the host binding.

Windows ARM64 requires a working WSL 2 backend. Windows virtual machines require nested virtualization. See the [Docker Desktop system requirements](https://docs.docker.com/desktop/setup/install/windows-install/#system-requirements).

### Local databases and cache

Set non-empty `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD` in the root `.env`. Local database credentials use `DB_USERNAME=root` and `DB_PASSWORD=root`. MySQL initializes its root account through `MYSQL_ROOT_PASSWORD`. Start PostgreSQL 18, MySQL 8.4 and Redis 8 with the commands below, or select services explicitly, for example `up -d --wait postgres redis`.

```sh
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait postgres mysql redis
docker compose --env-file .env -f docker/docker-compose.yml ps
```

The default project name is `openrust`; containers use `${APP_NAME}-service` names. The application image is `${APP_IMAGE_NAME:-openrust}:${APP_IMAGE_VERSION:-latest}`. PostgreSQL `5432`, MySQL `3306` and Redis `6379` bind to loopback by default. Data lives under `docker/data/postgres`, `docker/data/mysql` and `docker/data/redis`; PostgreSQL 18 mounts all of `/var/lib/postgresql`. `POSTGRES_PORT` and `MYSQL_PORT` change only host mappings; database container ports stay fixed. `REDIS_PORT` changes the Redis listener, host mapping, health check and application connection port together. Database credentials initialize empty data directories only; changing existing passwords requires updating actual accounts.

For an application running on the host, set `DB_CONNECTION=pgsql` or `mysql`, `DB_HOST=127.0.0.1`, the matching database port (PostgreSQL `5432`, MySQL `3306`), `REDIS_HOST=127.0.0.1` and `REDIS_PORT=6379` in the root `.env`. Keep the same database name and credentials. Set `CACHE_STORE=redis` and `QUEUE_CONNECTION=redis` as needed, generate the application key, run `./artisan migrate` and start the application. On Windows use `.\artisan.bat`.

### Application container

The application and all backing services start by default. To start only databases and Redis, append `postgres mysql redis`. For host development, keep `DB_HOST=127.0.0.1` and `REDIS_HOST=127.0.0.1`. In the same root `.env`, set `DOCKER_DB_HOST=postgres` and `DOCKER_REDIS_HOST=redis` for the application container. With MySQL use `DOCKER_DB_HOST=mysql` and `DOCKER_DB_PORT=3306`. `DOCKER_DB_PORT` is needed when the container port differs from host-side `DB_PORT`; PostgreSQL uses `5432`. For external services, omit these container overrides and set the actual `DB_HOST`, `DB_PORT` and `REDIS_HOST`, or override them explicitly when necessary. In production set `APP_ENV=production`, `APP_DEBUG=false`, strong credentials and the public `APP_URL`.

Keep an existing `APP_KEY`. Only run the generation command below when the key is missing, and save its output in the root `.env`. The application runs as UID/GID `10001`. Create `docker/data/storage` and make it writable; on Linux use `sudo chown 10001:10001 docker/data/storage`. The root `public/` directory is mounted read-only. Set `APP_PUBLIC_PATH` in the root `.env` to use another existing directory; relative paths are resolved from `docker/`.

```sh
docker compose --env-file .env -f docker/docker-compose.yml build app
docker compose --env-file .env -f docker/docker-compose.yml run --rm --no-deps app artisan key:generate --show
```

```sh
docker compose --env-file .env -f docker/docker-compose.yml run --rm --no-deps app artisan migrate --force
docker compose --env-file .env -f docker/docker-compose.yml up -d --wait
```

Open `http://localhost:8000/up`. Set `APP_PORT` to change the host port, which binds to loopback by default. The image uses `APP_PACKAGES_CACHE=storage/framework/cache/packages.json`; one-off commands and the application share writable storage. Use `up -d --wait` to start the application together with all backing services.

For multiple hosts, use the same `APP_IMAGE_NAME` / `APP_IMAGE_VERSION`, application key, external database and shared cache/queue settings, then run `up -d --no-build --no-deps --wait app`. Run migrations once per release, terminate TLS at the load balancer and use shared storage such as S3 for uploads. Set `APP_DISABLED_RUNNERS=schedule` on replicas that should not run the scheduler. Select optional features with `APP_BUILD_FEATURES` when building.

### Public Nginx entry point {#nginx}

Nginx starts by default and forwards HTTP requests to `app:8000`, listening publicly on `0.0.0.0:8080`. Set `NGINX_PORT` in the root `.env` to change the host port, and `NGINX_HOST` to restrict the bind address. Nginx always listens on container port `80`. For example, `NGINX_HOST=127.0.0.1` and `NGINX_PORT=18080` restrict access to the local host. Set the application's `APP_URL` to its actual public address, such as `http://localhost:8080`.

The proxy configuration in `docker/nginx/default.conf` preserves the request path and Host, forwards client and protocol information, supports WebSocket upgrades and disables response buffering for streaming. Nginx waits for a healthy application and resolves its address again through Docker DNS after recreation. Use the Nginx port for external access; `APP_HOST` and `APP_PORT` still control direct application access. Configure TLS at the HTTPS entry point used by your deployment.

Build settings use `APP_BUILD_FEATURES` and `APP_BUILD_JOBS` (default `4`), passed through Compose to the Dockerfile. Override `APP_STORAGE_PATH` for a different storage mount; relative paths start at `docker/` and must be writable by UID/GID `10001`.

When the build includes `webhook-client` or `webhook-server`, directly or as a dependency, the image includes that package's third-party notices and license texts under `/app/licenses/<package>/`.

Nginx permits request bodies up to 8 MiB, matching the framework input limit. Proxy read and send timeouts use the Nginx default of 60 seconds; streaming and WebSocket connections should exchange data or heartbeats promptly. Configure longer idle timeouts in `docker/nginx/default.conf` only when needed.

### Updates and rollback

Back up the database and storage and retain the existing `APP_KEY`. For a source release, run `build app`, then `run --rm --no-deps app artisan migrate --force`, and finally `up -d --no-build --wait`. Use the same Compose and environment-file arguments shown above for each command. For a published image, set a fixed `APP_IMAGE_NAME` / `APP_IMAGE_VERSION` and use `pull app` instead of `build app`. Run migrations once per release, rather than automatically in every container.

For rollback, select a retained previous image tag and run `up -d --no-build --wait` again. Image rollback does not undo database migrations; confirm schema compatibility and restore a backup when necessary. `stop` retains data; do not remove `docker/data/` or use `down -v` during updates.

When upgrading an existing installation, follow [Upgrading locale identifiers](../digging-deeper/localization.md#bcp47-upgrade) to migrate language resources, stored values and caches together.
