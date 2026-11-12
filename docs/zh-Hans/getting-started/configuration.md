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

<a id="determining-the-current-environment"></a>

## 确定当前环境

应用在配置加载完成后、Provider 注册前检测当前环境。检测读取 `app.env` 字符串，缺省值为 `production`。默认配置从 `APP_ENV` 读取该值，HTTP 按此配置启动。

Artisan 支持在命令前后传入 `--env=staging` 或 `--env staging`。同一次调用传入多次时，取第一个值。加载器依次尝试 `--env` 对应文件、外部 `APP_ENV` 对应文件和 `.env`。进程环境变量优先于文件中的值。即使文件回退，本次启动检测到的环境也取 `--env` 的值，配置中的 `app.env` 保留加载后的值。

```sh
./artisan env --env=staging
```

向 `Application::environment` 传入 `None` 可读取检测到的环境名称。将单个模式或模式切片放入 `Some` 可获取布尔结果。匹配区分大小写，`*` 可匹配任意字符，空切片返回 false。应用启动时也将同一匹配规则安装为容器的环境解析器。

```rust
use openrust::{
    container::EnvironmentResult,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withConfig(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().currentEnvironmentIs(&["local", "staging"])?));
    assert!(!bool::from(app.container().currentEnvironmentIs(&[])?));
    Ok(())
}
```

引导阶段将检测到的环境名以 `Arc<str>` 保存到 `env` 服务。环境查询读取该服务。启动后修改 `app.env` 只更新配置。

<a id="public-files"></a>

## 公共文件

| 环境变量 | 用途 | 默认值 |
|---|---|---|
| `PUBLIC_PATH` | `Application::publicPath()` 返回的应用公共目录 | `public` |
| `PUBLIC_URL_PREFIX` | 应用公共目录的 URL 前缀 | `/` |
| `PUBLIC_STORAGE_ROOT` | 公共本地磁盘的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公共本地磁盘的 `filesystems.disks.public.url` | `APP_URL` 加 `/storage` |

相对目录以应用根目录为基准。应用可以在引导前调用 `usePublicPath()` 覆盖 `PUBLIC_PATH`。默认 Web 路由通过 `Route::staticDisk("public")` 应用公共磁盘配置。挂载前缀取磁盘 URL 的完整路径，默认是 `/storage`。以 `/storage/tmp/` 结尾的同源 URL 或绝对 URL 路径 `/storage/tmp/` 都会挂到 `/storage/tmp`。跨源 URL 会导致路由注册失败。修改挂载配置后重启应用。

## 维护模式 {#maintenance-mode}

部署时运行 Artisan 的 `down`，应用返回 HTTP 503。运行 `up` 恢复访问。运行中的服务会逐请求读取共享状态，可以再次执行 `down` 更新选项。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正数秒数或重试日期，`--refresh` 指定浏览器刷新间隔。未注明时区的日期时间使用应用时区，响应头统一使用 GMT。`--redirect` 接受站内路径。`--render` 在执行 `down` 时将命名视图渲染一次，并传入 `$retryAfter`。`--status` 可覆盖默认的 503。期望 JSON 的请求沿用常规异常渲染，跳过浏览器跳转和预渲染页面。

维护期间可打开 `--with-secret` 打印的 URL 检查站点。应用设置有效期为 12 小时的签名 Cookie，当前会话没有原目标时跳转至首页。可以用 `--secret` 指定自己的路径。需要保留端点时，在 `bootstrap/app.rs` 的 `withMiddleware` 中配置。

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["up", "status/*"]);
})
```

预渲染页面由运行中的 HTTP 进程提供。服务重启时，应用启动成功后开始监听。队列行为见[维护期间的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

### 共享维护状态 {#shared-maintenance-state}

应用通过 `maintenanceMode()` 提供维护状态。默认 `file` 驱动把数据写入 `storage/framework/down`。选择 `cache` 和共享缓存仓库，可让多台机器上的 Worker 读取同一状态。`array` 驱动将状态保存在当前应用实例内，可用于测试。

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenanceMode()?.activate(&payload).await?;
    Ok(())
}
```

用 `isDownForMaintenance().await` 读取状态，用 `app.maintenanceMode()?.deactivate().await?` 清除。队列 Worker 在预约任务前读取此状态。
