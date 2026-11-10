# Installation

## Requirements

Install Rust 1.94.0 or newer with Cargo. OpenRust uses the Rust 2024 edition. The application and the framework crate under `vendor/openrust/framework/` are built together. The first build downloads the dependencies recorded in `Cargo.lock`.

## Starting the application

Run these commands from the application root. Copy the environment template when creating a new local environment.

```sh
cp .env.example .env
cargo +1.94.0 build --locked
./artisan inspire
cargo +1.94.0 run --locked
```

The HTTP listener defaults to `127.0.0.1:8000`. Visit `http://localhost:8000/` for the welcome page or `/up` for the health check. Stop the service with Ctrl+C.

The application can also start without an environment file. To choose a different port in `.env`, set `APP_PORT=9081`. The template expands `APP_URL="http://localhost:${APP_PORT}"`. An explicit process environment value overrides its file value. For example, `APP_PORT=9081 cargo +1.94.0 run --locked` selects port 9081. See [configuration](configuration.md) for URL overrides and environment selection.

On Windows, copy the template with `copy .env.example .env` and run `artisan.bat inspire`. Windows runtime verification remains outstanding.

## Building a standalone application

```sh
cargo +1.94.0 build --release --locked --features standalone
```

Place `target/release/main` in the deployment root and include `resources/views/welcome.html`. Also include any website files in `public/` and public disk files in `storage/app/public/`. The executable reads `.env` from its own directory. Set the absolute process environment value `APP_BASE_PATH` when the application root is elsewhere. Run `./main artisan inspire` from that directory, or use an absolute executable path. See [request lifecycle](../architecture-concepts/lifecycle.md) for source and standalone root resolution.

A locked offline build uses `--offline` after dependencies have been downloaded. It still requires the complete application and framework sources.
