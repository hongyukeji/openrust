# 请求生命周期

Tokio 入口将 `bootstrap::createApplication` 交给 `openrust::foundation::runApplication`。框架先选择运行入口，再创建应用。无参数时调用 `Application::start()` 运行 HTTP，首个参数为 `artisan` 时调用 [`Application::handleCommand`](../digging-deeper/artisan.md)。

应用入口将选定的根目录命名为 `basePath`，传给 `Application::configure(basePath)`，对应 Laravel 的 `configure(basePath: ...)`。随后 `create()` 校验根目录并建立服务容器，由选定的 Kernel 引导应用。引导时加载环境和配置，执行所有普通 Provider 的 `register`，再执行各 Provider 的 `boot`，然后注册共享路由。进程环境变量优先于环境文件。

进程环境中的绝对路径 `APP_BASE_PATH` 优先。未设置时，源码构建使用构建时记录的应用 crate 目录，`cargo run --release` 也采用这条规则。启用 `standalone` 特性的构建使用可执行文件所在目录。HTTP 和 Artisan 都从选定的应用根目录读取 `.env`，启动时的工作目录不影响选择。

使用 `cargo build --release --locked --features standalone` 构建，把 `target/release/main`（Windows 上为 `target/release/main.exe`）与 `resources/views/welcome.html` 一并放入发布目录，保留视图文件的相对路径，即可整体迁移。二进制与应用分开部署时，将 `APP_BASE_PATH` 设为应用根目录的绝对路径。部署静态资源时，按原相对路径保留 `public/`。使用默认公共磁盘挂载时，将要公开的存储文件放在 `storage/app/public/` 或配置的目录中。

HTTP Kernel 创建时应用中间件配置，引导后解析路由中间件栈，完成后才绑定监听端口。Console Kernel 引导同一个应用，随后注册命令。

HTTP 内核运行[中间件](../the-basics/middleware.md)，并通过框架[路由器](../the-basics/routing.md)分发请求。应用骨架在 `/` 调用 `view("welcome")`。框架将视图名解析为应用根目录下的 `resources/views/welcome.html`，并在请求时读取。修改 HTML 后，下次请求即可看到结果。`/up` 提供健康检查。

默认应用通过 `bootstrap/app.rs` 中的 `withRouting` 声明 Web 路由、控制台路由及健康路径。Web 路由使用[默认 Web 中间件组](../the-basics/middleware.md)，控制台路由在 Console Kernel 引导后登记。API、广播和 gRPC 使用各自的可选注册入口。应用引导完成后，`/up` 处理器返回 `200 OK`。

路由或中间件返回 HTTP 错误时，内核应用 `bootstrap/app.rs` 中声明的[异常渲染策略](../the-basics/errors.md#将异常渲染为-json)。

延迟 Provider 在首次解析服务时加载。Console Kernel 在注册命令前加载所有剩余延迟 Provider；HTTP Kernel 保留未使用项。执行顺序及异步解析见[延迟服务提供者](providers.md#延迟服务提供者)。
