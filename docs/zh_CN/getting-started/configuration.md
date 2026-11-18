# 配置

在应用根目录将 `.env.example` 复制为 `.env`，即可自定义本地配置。`APP_HOST` 和 `APP_PORT` 配置 HTTP 监听地址，默认为 `127.0.0.1:8000`。模板沿用 Laravel 的本地开发默认值。应用将 `config/` 下的配置分区加载到同一个仓库，可以通过 `application.config()?.get("app.url")` 读取最终生效的值。应用、认证、数据库与 Redis、缓存、会话、日志、邮件、队列、文件系统、广播、CORS、并发、哈希、图像、第三方服务、Reverb、gRPC 和视图各有独立配置。加载配置时不连接外部服务。

`app.url` 优先使用显式设置的 `APP_URL`。未设置时，使用具体的 `APP_HOST` 和 `APP_PORT` 生成 URL，例如 `http://example.test:8080` 或 `http://[2001:db8::1]:8080`。未设置主机，或使用 `127.0.0.1`、`::1` 等回环地址或 `0.0.0.0`、`::` 等通配监听地址时，使用 `http://localhost:{port}`。环境模板设置了 `APP_URL="http://${APP_HOST}:${APP_PORT}"`，使用模板默认值时展开为 `http://127.0.0.1:8000`。

HTTP 请求内生成 URL 时，包括使用 `URL::forRequest(request)`，优先使用显式设置的 `APP_URL`，其次是当前请求的来源。请求没有来源时，使用从监听配置推导的 `app.url`。Artisan 直接使用 `app.url`。

设置 `APP_VERSION` 可覆盖 `app.version` 中的应用版本。未设置时，默认取编译时由 `env!("CARGO_PKG_VERSION")` 读取的 Cargo 包版本。欢迎页显示 `app.version`。

## 当前配置仓储

配置在 boot 中、Provider register 前可用。与 Laravel LoadConfiguration 一样，boot 先绑定新仓储，再执行配置加载器并填充该仓储；`create()` 不提前放入空配置服务。`Application::config()` 每次解析当前 `config` 服务，返回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引导前访问或替换为不兼容类型后访问会返回错误。

在应用作用域内，导入 `openrust::support::facades::Config`，通过 `Config::get("app.name")?` 读取配置。门面每次调用都解析当前应用仓储。键缺失时返回 `None`，显式空值返回 `Some(Value::Null)`。在应用作用域外或仓储注册前调用会返回绑定错误。

`config` 是规范名称，`dyn openrust::contracts::config::Repository` 是其类型别名。替换应用仓储时，使用 `openrust::config::Repository::new(values)` 构造，再调用 `instanceNamed::<dyn Repository>("config", repository)?`。已经持有 `Arc` 的消费者保留原快照；需要跟随替换时重新解析，或使用重绑定/refresh 监听器。按原始类型键重新绑定会替换该别名，而不是规范配置服务，遵循容器的原键规则。

Provider 替换的仓储用于 HTTP 监听绑定、路由/静态磁盘装配及视图路径设置。健康页 HTML 在每次请求时读取当前仓储，Artisan 使用相同应用访问器。运行时替换配置不会自动重建已经装配的路由或视图查找器。容器错误沿相应的启动、HTTP 或 Artisan 错误路径传播。参见[重绑定](../architecture-concepts/container.md#重绑定与刷新消费者)。

数值和布尔配置在启动时校验。布尔值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空配置接受 `null`。运行驱动需要框架提供对应服务。

HTTP 绑定监听器时校验 `APP_PORT`。Artisan 无需有效的 HTTP 监听地址。端口 `0` 允许操作系统选择监听端口，需要稳定的对外 URL 时请显式设置 `APP_URL`。

HTTP 视图渲染器按顺序查找 `view.paths` 中的目录，默认从 `resources/views` 读取 HTML。相对目录以应用根目录为基准。

进程环境变量优先于文件中的值。在进程环境中设置 `APP_ENV` 可选择 `.env.local` 等文件。对应文件不存在时，应用读取 `.env`。部署路径和 `APP_BASE_PATH` 的用法见[应用根目录规则](../architecture-concepts/lifecycle.md)。

## 访问配置值 {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::stringOr("app.name", "App")
}
```

用 `Config::set` 更新当前应用的配置。`string`、`integer`、`float`、`boolean`、`array` 和 `collection` 检查值的类型。`array` 接受 JSON 列表和对象，`collection` 保留集合键。

用 `Config::stringOr("app.name", "App")` 直接取得字符串。键缺失、值为 `null` 或类型不符、配置仓储不可用时，均使用传入的默认值。需要严格读取并处理错误时，用 `Config::string("app.name")?`。原始值读取 `Config::getOr("app.name", Value::from("App"))?` 仅在缺键时使用默认值，并保留显式空值。`Config::getMany([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` 按输入顺序读取多个键，各自使用默认值。读取整组配置时，为结构体派生 `serde::Deserialize`，再用 `Config::deserialize::<MailOptions>("mail")?`。

## 配置缓存 {#configuration-caching}

用 `config:cache` 保存应用重新启动后的配置，包括服务提供者设置的值。后续 HTTP 和 Artisan 启动会读取这份快照。更新配置后，用 `config:clear` 移除快照，再重启应用。

```sh
./artisan config:cache
./artisan config:clear
```

设置 `APP_URL` 并生成配置缓存后，普通链接和绝对签名链接都使用该地址。

刷新时先移除所选的旧快照，再引导新应用。若新应用引导失败，缓存保持为空。

默认路径为 `storage/framework/config.json`。可用 `APP_CONFIG_CACHE` 指定绝对路径或相对于应用根目录的文件路径。将它设置在进程环境中，可以在读取 `.env` 前选定路径。不同部署环境请使用各自的缓存文件。

如果在加载 dotenv 前命中缓存，应用会跳过 dotenv 解析。缓存包含生效配置，可能含有密钥等秘密，应与私有部署数据一起保管。无效 JSON 会中止启动。缓存损坏时，先移除该文件再重建。

用 `./artisan config:show app.name` 查看单个键，或用 `./artisan config:show app` 查看配置节。命令显示当前生效的值，其中可能包含凭据。

## 环境文件加密 {#environment-file-encryption}

使用 `env:encrypt` 创建加密副本。运行 `env:decrypt` 前在进程环境中设置 `ENV_ENCRYPTION_KEY`，或传入 `--key`。如果由加密命令生成密钥，将输出的 `base64:` 值保存到密钥管理系统，和加密文件分开保管。

```sh
./artisan env:encrypt --env=staging --no-interaction
./artisan env:decrypt --env=staging --force --no-interaction
```

默认密码算法为 AES-256-CBC。选用 AES-128-CBC、AES-128-GCM 或 AES-256-GCM 时，加密和解密需传入同一个 `--cipher`。`--env=staging` 选择 `.env.staging` 和 `.env.staging.encrypted`。省略 `--env` 时，加密源由启动时选择的环境文件决定，解密默认写入 `.env`。

使用 `--readable` 逐项加密，变量名保持可见。之后使用相同的密钥更新可读密文，值未改变的条目会复用原密文。`--force` 覆盖已有输出，`--prune` 在加密成功后删除明文。解密时可用 `--path` 和 `--filename` 指定目标目录和文件名。

命令不会回显手动传入的密钥。加密时使用 `--quiet` 或 `--silent` 需传入密钥，以便日后解密。

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
| `APP_PUBLIC_PATH` | `Application::publicPath()` 返回的应用公共目录 | `public` |
| `APP_PUBLIC_URL_PREFIX` | 应用公共目录的 URL 前缀 | `/` |
| `PUBLIC_STORAGE_ROOT` | 公共本地磁盘的 `filesystems.disks.public.root` | `storage/app/public` |
| `PUBLIC_STORAGE_URL` | 公共本地磁盘的 `filesystems.disks.public.url` | `app.url` 加 `/storage` |

相对目录以应用根目录为基准。应用可以在引导前调用 `usePublicPath()` 覆盖 `APP_PUBLIC_PATH`。默认 Web 路由通过 `Route::staticDisk("public")` 应用公共磁盘配置。挂载前缀取磁盘 URL 的完整路径，默认是 `/storage`。以 `/storage/tmp/` 结尾的同源 URL 或绝对 URL 路径 `/storage/tmp/` 都会挂到 `/storage/tmp`。跨源 URL 会导致路由注册失败。修改挂载配置后重启应用。

## 维护模式 {#maintenance-mode}

部署时运行 Artisan 的 `down`，应用返回 HTTP 503。运行 `up` 恢复访问。运行中的服务会逐请求读取共享状态，可以再次执行 `down` 更新选项。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正数秒数或重试日期，`--refresh` 指定浏览器刷新间隔。未注明时区的日期时间使用应用时区，响应头统一使用 GMT。`--redirect` 接受站内路径。`--render` 在执行 `down` 时将命名视图渲染一次，并传入 `$retryAfter`。`--status` 可覆盖默认的 503。期望 JSON 的请求沿用常规异常渲染，跳过浏览器跳转和预渲染页面。

维护期间可打开 `--with-secret` 打印的 URL 检查站点。应用设置有效期为 12 小时的签名 Cookie，当前会话没有原目标时跳转至首页。可以用 `--secret` 指定自己的路径。配置的健康路径（默认 `/up`）在维护期间仍可访问。需要保留其他端点时，在 `bootstrap/app.rs` 的 `withMiddleware` 中配置。

```rust
.withMiddleware(|middleware| {
    middleware.preventRequestsDuringMaintenance(["status/*"]);
})
```

使用文件驱动部署时，`./artisan down --render=errors::503` 可在 Provider 启动期间提供预渲染页面。可在 `resources/views/errors/503.blade.html` 自定义页面。监听配置需成功加载。JSON 请求、排除路径和有效绕过请求等待应用内核。缓存驱动由运行中的内核提供维护响应。队列行为见[维护期间的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

启动期间请使用 Artisan 打印的完整绕过 URL。启动前处理器比较原始请求地址，查询串和尾斜杠都会参与匹配。HTTP 内核启动后，维护中间件按路径匹配，忽略查询串及路径两端的斜杠。

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

## 测试环境与启动快照 {#testing-environment}

`Application::runningUnitTests()` 和应用契约返回 `Result<bool, BindingResolutionError>`。仅当容器中的 `env` 精确为 `testing` 时结果为 true，与是否正在运行测试程序无关。环境未绑定时返回 `Ok(false)`，不触发服务解析；解析错误沿 `Result` 传播。后续修改 `config("app.env")` 不会替换已检测的环境绑定。

启动后，通过 `app.make::<openrust::config::Environment>().await?` 读取当前应用不可变的环境快照。既有 `get`、`get_utf8` 返回原始字符串，具体旗标仍由对应包解析。快照遵守进程变量优先、环境文件选择及配置缓存规则，不重新加载文件，也不修改进程全局变量。

## 应用存储路径 {#application-storage-path}

`app.storagePath("")` 返回存储根目录，传入 `"pail"` 等字符串可获取子路径。`app.useStoragePath(path)?` 覆盖它并重绑 `path.storage`，应用契约也提供 getter。`openrust::support::storage_path(path)?` 读取当前应用作用域。只读的 `foundation::Paths` 服务让编译型包共享同一状态，避免通过容器强持有 Application。

优先级是非空且非 `0` 的显式覆盖、应用环境快照中的 `APP_STORAGE_PATH`、应用根目录下的 `storage`。该无品牌变量对应 Laravel 的 `LARAVEL_STORAGE_PATH`；环境中显式存在的空值仍会保留。Setter 传入相对路径时保持相对。拼接追加本机分隔符并去掉子路径开头的本机分隔符，保留重复分隔符和 `..`。Rust 没有默认参数，读取根目录时显式传 `""`。

构造时初始化 `path.storage`，后加载的环境文件可以改变 getter，但不会自动重写该绑定；`useStoragePath` 则显式重绑，沿用 Laravel 的顺序。重绑错误通过 `Result` 返回，此时路径状态已经更新。

日志、文件 Session、文件缓存数据与锁、编译视图以及默认 local/public 磁盘在加载配置时采用存储根目录。`PUBLIC_STORAGE_ROOT` 和 `VIEW_COMPILED_PATH` 仍可单独覆盖，公共 URL 和挂载顺序不变。`storage:link` 指向配置的公共磁盘。启动前设置绝对路径的 `APP_STORAGE_PATH`，或在加载配置前调用 `useStoragePath`。

编译型配置加载器使用 `env.storage_path(base_path, subpath)` 作为 Rust 上下文适配。弱引用选取所属应用的路径，不依赖全局 Facade，也不延长应用生命周期。独立环境快照按自身变量及传入的默认根目录解析。后续更改应用路径不会回写已经加载或缓存的配置值。

文件维护模式每次操作都解析当前路径，服务已解析后仍有效。启动前的预渲染检查使用 boot 前可用的路径：此时应提供进程变量或显式 setter，仅在 dotenv 中设置的值尚不可用。Pail 在首次解析 Files 时确定目录，应在首次使用前配置。配置、路由、事件和包缓存产物的位置继续遵守各自的缓存路径契约。
