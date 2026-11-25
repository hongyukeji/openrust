# Installation

## Requirements

Install Rust 1.94.0 or newer with Cargo. OpenRust uses the Rust 2024 edition. The application and the framework crate under `vendor/openrust/framework/` are built together. The first build downloads the dependencies recorded in `Cargo.lock`.

## Full initialization {#complete-initialization}

With an existing MySQL database and Redis service, export the connection settings or put them in the application environment file. Supply `DB_URL` or `DB_DATABASE` and `DB_USERNAME`, plus `REDIS_URL` or `REDIS_HOST`. Set host, port, password and TLS configuration as required by your services.

```sh
./artisan init:full --dry-run
./artisan init:full
cargo run --locked
```

`init:full` is non-interactive and checks configuration before writing files. It enables the complete preset with the web and API starter kit, installs package resources, builds the application and runs migrations and HTTP, Artisan and Redis worker checks. It uses Redis for cache, sessions and queues. Docker Compose and provisioning are independent of this command. Process-injected credentials stay in the process environment, so supply them to the application as well. Existing conflicting driver settings require explicit adjustment. `--force` on `init:full` authorizes production migration while preserving existing source customizations. Use `./artisan init` for interactive selection.

Single hosts and replicas use the same application configuration. All roles in one deployment share the configured MySQL and Redis services and application key. Deployment tooling coordinates process roles, migrations and shared files.

## Starting the application

Run these commands from the application root. Copy the environment template when creating a new local environment.

```sh
cp .env.example .env
touch database/database.sqlite
cargo +1.94.0 build --locked
./artisan key:generate
./artisan migrate
./artisan inspire
cargo +1.94.0 run --locked
```

The template uses SQLite for application data, sessions, cache and queues. Create the database file and run the migrations before using these services.

The HTTP listener defaults to `127.0.0.1:8000`. Visit `http://localhost:8000/` for the welcome page or `/up` for the health check. Stop the service with Ctrl+C.

The application can also start without an environment file. To choose a different port in `.env`, set `APP_PORT=9081`. With the template’s default listener host, `APP_URL` expands to `http://127.0.0.1:9081`. An explicit process environment value overrides its file value. For example, `APP_PORT=9081 cargo +1.94.0 run --locked` selects port 9081. See [configuration](configuration.md) for URL overrides and environment selection.

On Windows, copy the template with `copy .env.example .env` and run `artisan.bat inspire`. Use an MSVC Rust toolchain for the target architecture. For a release without Cargo, use the Windows standalone steps. [Windows standalone deployment](deployment.md#windows-standalone)

## Building a standalone application

```sh
cargo +1.94.0 build --release --locked --features standalone
```

Place `target/release/main` in the deployment root and include `resources/views/welcome.blade.html`. Also include any website files in `public/` and public disk files in `storage/app/public/`. The executable reads `.env` from its own directory. Set the absolute process environment value `APP_BASE_PATH` when the application root is elsewhere. Run `./main artisan inspire` from that directory, or use an absolute executable path. See [request lifecycle](../architecture-concepts/lifecycle.md) for source and standalone root resolution.

A locked offline build uses `--offline` after dependencies have been downloaded. It still requires the complete application and framework sources.
