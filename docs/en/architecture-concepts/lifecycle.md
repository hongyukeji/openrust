# Request Lifecycle

The Tokio entry point passes `bootstrap::createApplication` to `openrust::foundation::runApplication`. The framework selects the entry point before creating the application. With no arguments it calls `Application::start()` for HTTP. With `artisan` as the first argument it calls [`Application::handleCommand`](../digging-deeper/artisan.md).

The application entry declares its root as `basePath` and passes it to `Application::configure(basePath)`, matching Laravel’s `configure(basePath: ...)`. It then calls `create()` to validate the root and create the service container. The selected kernel boots the application. Boot loads environment variables and configuration, runs every provider’s `register` before any provider’s `boot`, and registers shared routes. Process environment variables take precedence over the environment file.

An absolute `APP_BASE_PATH` in the process environment takes precedence. Otherwise, source builds use the application crate directory recorded at build time, including when you run `cargo run --release`. Builds with the `standalone` feature use the executable’s directory as the application root. HTTP and Artisan both read `.env` from the selected root, regardless of the working directory.

Build with `cargo build --release --locked --features standalone` and place `target/release/main` (`target/release/main.exe` on Windows) in the deployment root. Include `resources/views/welcome.html` at that relative path. The deployment directory can then be moved as a unit. For a deployment that keeps the executable outside the application root, set `APP_BASE_PATH` to that root’s absolute path. Include the contents of `public/` at the same relative path. With the default public disk mount, include any public storage files under `storage/app/public/` or at the configured location.

The HTTP kernel configures middleware when it is created and resolves each route’s middleware stack after boot. It completes both steps before binding the listener. The Console Kernel boots the same application and then registers commands.

The HTTP kernel runs [middleware](../the-basics/middleware.md) and dispatches the request through the framework [router](../the-basics/routing.md). At `/`, the skeleton calls `view("welcome")`. The framework resolves the name to `resources/views/welcome.html` in the application root and reads it at request time. Editing the HTML takes effect on the next request. The skeleton also exposes the health check at `/up`.

`bootstrap/app.rs` declares the Web routes, console routes and health path with `withRouting`. The framework mounts Web routes in the `web` group and loads console routes after the Console Kernel boots. The default application loads Web and console routes. API, broadcasting and gRPC use separate optional registration paths. The built-in `web` group currently includes route binding. The framework’s `/up` endpoint returns `200 OK` as an HTTP liveness check. The health diagnostic event and the cookie, session and CSRF layers depend on their respective framework components.

When routing or middleware returns an HTTP error, the kernel applies the [exception rendering policy](../the-basics/errors.md#rendering-exceptions-as-json) declared in `bootstrap/app.rs`.
