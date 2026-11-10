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

`SIGINT` and, on Unix, `SIGTERM` request graceful HTTP shutdown. Use a process manager to restart the application. Production reverse proxy, TLS, database drivers and platform support must be validated for the deployment environment; this minimal delivery covers the macOS ARM64 startup path. Windows `artisan.bat` remains pending native acceptance.

The source `artisan` script runs Cargo. A standalone deployment does not require Cargo and runs commands with `./main artisan …`. See [Artisan](../digging-deeper/artisan.md) and [application configuration](configuration.md).
