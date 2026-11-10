# 配置

在应用根目录将 `.env.example` 复制为 `.env`，即可自定义本地配置。`APP_HOST` 和 `APP_PORT` 配置 HTTP 监听地址，默认为 `127.0.0.1:8000`。未设置 `APP_URL` 时，默认 URL 使用 `http://localhost` 和已配置的 `APP_PORT`。模板沿用 Laravel 的本地开发默认值。应用将 `config/` 下的配置分区加载到同一个仓库，可以通过 `application.config()?.get("app.url")` 读取最终生效的值。应用、认证、数据库与 Redis、缓存、会话、日志、邮件、队列、文件系统、广播、CORS、并发、哈希、图像、第三方服务、Reverb、gRPC 和视图各有独立配置。加载配置时不连接外部服务。

## 当前配置仓储

配置在 boot 中、Provider register 前可用。与 Laravel LoadConfiguration 一样，boot 先绑定新仓储，再执行配置加载器并填充该仓储；`create()` 不提前放入空配置服务。`Application::config()` 每次解析当前 `config` 服务，返回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引导前访问或替换为不兼容类型后访问会返回错误。

`config` 是规范名称，`dyn openrust::contracts::config::Repository` 是其类型别名。替换应用仓储时，使用 `openrust::config::Repository::new(values)` 构造，再调用 `instanceNamed::<dyn Repository>("config", repository)?`。已经持有 `Arc` 的消费者保留原快照；需要跟随替换时重新解析，或使用重绑定/refresh 监听器。按原始类型键重新绑定会替换该别名，而不是规范配置服务，遵循容器的原键规则。

Provider 替换的仓储用于 HTTP 监听绑定、路由/静态磁盘装配及视图路径设置。健康页 HTML 在每次请求时读取当前仓储，Artisan 使用相同应用访问器。运行时替换配置不会自动重建已经装配的路由或视图查找器。容器错误沿相应的启动、HTTP 或 Artisan 错误路径传播。参见[重绑定](../architecture-concepts/container.md#重绑定与刷新消费者)。

数值和布尔配置在启动时校验。布尔值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空配置接受 `null`。运行驱动需要框架提供对应服务。

HTTP 绑定监听器时校验 `APP_PORT`。Artisan 无需有效的 HTTP 监听地址。端口 `0` 允许操作系统选择监听端口，需要稳定的对外 URL 时请显式设置 `APP_URL`。

HTTP 视图渲染器按顺序查找 `view.paths` 中的目录，默认从 `resources/views` 读取 HTML。相对目录以应用根目录为基准。

进程环境变量优先于文件中的值。在进程环境中设置 `APP_ENV` 可选择 `.env.local` 等文件。对应文件不存在时，应用读取 `.env`。部署路径和 `APP_BASE_PATH` 的用法见[应用根目录规则](../architecture-concepts/lifecycle.md)。

<a id="public-files"></a>

## 公共文件

| 环境变量 | 用途 | 默认值 |
|---|---|---|
| `PUBLIC_PATH` | `Application::publicPath()` 返回的应用公共目录 | `public` |
| `PUBLIC_URL_PREFIX` | 应用公共目录的 URL 前缀 | `/` |
| `PUBLIC_STORAGE_ROOT` | 公共本地磁盘的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公共本地磁盘的 `filesystems.disks.public.url` | `APP_URL` 加 `/storage` |

相对目录以应用根目录为基准。应用可以在引导前调用 `usePublicPath()` 覆盖 `PUBLIC_PATH`。默认 Web 路由通过 `Route::staticDisk("public")` 应用公共磁盘配置。挂载前缀取磁盘 URL 的完整路径，默认是 `/storage`。以 `/storage/tmp/` 结尾的同源 URL 或绝对 URL 路径 `/storage/tmp/` 都会挂到 `/storage/tmp`。跨源 URL 会导致路由注册失败。修改挂载配置后重启应用。
