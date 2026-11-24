# 配置

应用名称和版本默认使用应用 `Cargo.toml` 中的 `name` 和 `version`，在编译时嵌入。可在 `.env` 中通过 `APP_NAME` 和 `APP_VERSION` 分别覆盖。骨架的 `.env.example` 显式设置了 `APP_NAME=OpenRust`，该值优先于包名。

在应用根目录将 `.env.example` 复制为 `.env`，即可自定义本地配置。`APP_HOST` 和 `APP_PORT` 配置 HTTP 监听地址，默认为 `127.0.0.1:8000`。模板沿用 本地开发默认值。应用将 `config/` 下的配置分区加载到同一个仓库，可以通过 `application.config()?.get("app.url")` 读取最终生效的值。应用、认证、数据库与 Redis、缓存、会话、日志、邮件、队列、文件系统、广播、CORS、并发、哈希、图像、第三方服务、Reverb、gRPC 和视图各有独立配置。加载配置时不连接外部服务。

`app.url` 优先使用显式设置的 `APP_URL`。未设置时，使用具体的 `APP_HOST` 和 `APP_PORT` 生成 URL，例如 `http://example.test:8080` 或 `http://[2001:db8::1]:8080`。未设置主机，或使用 `127.0.0.1`、`::1` 等回环地址或 `0.0.0.0`、`::` 等通配监听地址时，使用 `http://localhost:{port}`。环境模板设置了 `APP_URL="http://${APP_HOST}:${APP_PORT}"`，使用模板默认值时展开为 `http://127.0.0.1:8000`。

HTTP 请求内生成 URL 时，包括使用 `Url::for_request(request)`，优先使用显式设置的 `APP_URL`，其次是当前请求的来源。请求没有来源时，使用从监听配置推导的 `app.url`。Artisan 直接使用 `app.url`。

设置 `APP_VERSION` 可覆盖 `app.version` 中的应用版本。未设置时，默认取编译时由 `env!("CARGO_PKG_VERSION")` 读取的 Cargo 包版本。欢迎页显示 `app.version`。

## 框架版本与应用版本 {#framework-version}

`Application::VERSION` 和 `application.version()` 返回框架包版本，编译时取自 `vendor/openrust/framework/Cargo.toml`。应用作用域内可用 `App::version()?` 解析当前 Application 并取得相同版本；作用域外返回容器绑定错误。常量和实例方法无需先引导配置。

```rust
use openrust::{foundation::Application, support::facades::App};

let framework_version = Application::VERSION;
let current_framework_version = App::version()?;
```

`./artisan --version` 使用既有的 `artisan <version>` 格式显示框架版本。`Config::get("app.version")?` 则读取应用版本，默认取应用包版本，可通过 `APP_VERSION` 覆盖；欢迎页继续显示该应用版本。修改应用包版本、`app.version` 或 `APP_VERSION` 均不改变框架版本。更新框架包后需重新编译，才能嵌入新的框架版本。

## 当前配置仓储

配置在 boot 中、Provider register 前可用。与框架LoadConfiguration 一样，boot 先绑定新仓储，再执行配置加载器并填充该仓储；`create()` 不提前放入空配置服务。`Application::config()` 每次解析当前 `config` 服务，返回 `Result<Arc<dyn Repository>, BindingResolutionError>`；引导前访问或替换为不兼容类型后访问会返回错误。

在应用作用域内，导入 `openrust::support::facades::Config`，通过 `Config::get("app.name")?` 读取配置。门面每次调用都解析当前应用仓储。键缺失时返回 `None`，显式空值返回 `Some(Value::Null)`。在应用作用域外或仓储注册前调用会返回绑定错误。

`config` 是规范名称，`dyn openrust::contracts::config::Repository` 是其类型别名。替换应用仓储时，使用 `openrust::config::Repository::new(values)` 构造，再调用 `instance_named::<dyn Repository>("config", repository)?`。已经持有 `Arc` 的消费者保留原快照；需要跟随替换时重新解析，或使用重绑定/refresh 监听器。按原始类型键重新绑定会替换该别名，而不是规范配置服务，遵循容器的原键规则。

Provider 替换的仓储用于 HTTP 监听绑定、路由/静态磁盘装配及视图路径设置。健康页 HTML 在每次请求时读取当前仓储，Artisan 使用相同应用访问器。运行时替换配置不会自动重建已经装配的路由或视图查找器。容器错误沿相应的启动、HTTP 或 Artisan 错误路径传播。参见[重绑定](../architecture-concepts/container.md#重绑定与刷新消费者)。

数值和布尔配置在启动时校验。布尔值接受 `true`、`false`、`1` 和 `0`。`SESSION_DOMAIN` 等可空配置接受 `null`。运行驱动需要框架提供对应服务。

HTTP 绑定监听器时校验 `APP_PORT`。Artisan 无需有效的 HTTP 监听地址。端口 `0` 允许操作系统选择监听端口，需要稳定的对外 URL 时请显式设置 `APP_URL`。

HTTP 视图渲染器按顺序查找 `view.paths` 中的目录，默认从 `resources/views` 读取 HTML。相对目录以应用根目录为基准。

进程环境变量优先于文件中的值。在进程环境中设置 `APP_ENV` 可选择 `.env.local` 等文件。对应文件不存在时，应用读取 `.env`。部署路径和 `APP_BASE_PATH` 的用法见[应用根目录规则](../architecture-concepts/lifecycle.md)。

## 访问配置值 {#accessing-configuration-values}

```rust
use openrust::support::facades::Config;

pub fn application_name() -> String {
    Config::string_or("app.name", "App")
}
```

用 `Config::set` 更新当前应用的配置。`string`、`integer`、`float`、`boolean`、`array` 和 `collection` 检查值的类型。`array` 接受 JSON 列表和对象，`collection` 保留集合键。

语言切换使用 [App 语言方法](../digging-deeper/localization.md#配置语言)，以同步配置、译器和切换事件。在请求或队列任务中，`app.locale`、`app.fallback_locale` 的修改仅影响当前作用域；整体替换 `app` 配置组也仅影响当前作用域，并清除原有子键覆盖。直接修改 `Config` 不会自动更新已解析的译器。

整体替换 `app` 后，子键按调用顺序写入：替换 `app.ui` 会移除它原有的子键，之后写入 `app.ui.title` 则加入新父对象。写入数组下标保留兄弟元素；`get`、`all`、`has`、`push`、`prepend` 读取同一作用域值，稀疏数组转为带数字键的 JSON 对象。同一生命周期中的嵌套调用共享改动，其他请求或任务以及应用默认值不受影响。`push` 可把当前为 `null` 的配置初始化为列表；两种方法都接受关联数组，追加保留原键，前插会重排整数键并保留命名键。对 `null` 使用 `prepend`，或对其他标量执行这两种操作，均返回错误。

用 `Config::string_or("app.name", "App")` 直接取得字符串。键缺失、值为 `null` 或类型不符、配置仓储不可用时，均使用传入的默认值。需要严格读取并处理错误时，用 `Config::string("app.name")?`。原始值读取 `Config::get_or("app.name", Value::from("App"))?` 仅在缺键时使用默认值，并保留显式空值。`Config::get_many([("app.name", Value::from("App")), ("app.debug", Value::Bool(false))])?` 按输入顺序读取多个键，各自使用默认值。读取整组配置时，为结构体派生 `serde::Deserialize`，再用 `Config::deserialize::<MailOptions>("mail")?`。

严格读取方法另有 `*_with_default`、`*_or_else`：仅在缺键时使用默认值，显式 null 或已有值类型不符仍返回错误。惰性默认值在读取后最多执行一次；整数与浮点数保持类型区分。仓储不可用时，先返回绑定错误，不调用默认值。

```rust
use openrust::config::ConfigKey;
let name = Config::string_with_default("app.name", "App")?;
let retries = Config::integer_with_default("services.retries", 3)?;
let ratio = Config::float_or_else("services.ratio", || 1.5)?;
let selected = Config::get_many(["app.name", "app.locale"])?;
let custom = Config::get_many([
    ConfigKey::from("app.name"),
    ConfigKey::lazy("services.label", || Value::from("fallback")),
])?;
Config::set_many([("services.enabled", Value::Bool(true))])?;
```

六种严格读取均提供上述默认值入口；门面的字符串默认值可直接传 `&str`，数组与集合默认值使用 JSON 列表或对象。`get_many` 也保留已有 `(键, 默认值)` 写法，重复键保留首次位置并采用最后一次结果，已有 null 不执行惰性默认值。读取按输入顺序进行，前一默认值回调的修改可被后续键读取。`set_many` 按顺序写入，不构成事务。完整字面键优先于点分路径。

`openrust::config::Repository` 同样提供这些读取与批量操作；配置契约提供默认实现，替换仓储后公开入口仍可用，门面也会遵循自定义仓储对类型化读取的覆写。


下标访问使用具名入口 `offset_exists`、`offset_get`、`offset_set` 和 `offset_unset`，实际转发当前仓储及其自定义覆写。`offset_unset("key")?` 是将值设为 null：键仍存在，`offset_exists` 返回 true，`offset_get` 返回 `Some(Value::Null)`；原来不存在的键也会创建为 null。字符串键可以为空，也可以包含空的点分片段或字面 `*`；星号不是通配符。这些入口与 `set` 共用请求／任务的语言配置隔离。同步仓储无法提供稳定的可变下标引用，因此 Rust 使用具名方法。

## 配置宏 {#configuration-macros}

在应用启动时注册宏，通过公开门面直接调用：

```rust
Config::r#macro("feature", |repository, key: String| {
    repository.boolean(&format!("features.{key}"))
})?;
let enabled = Config::macro_call::<bool, _>("feature", "search".to_owned())?;
```

回调接收当前仓储，包括替换后的仓储及其读取覆写。`has_macro` 检查注册，`flush_macros` 清空注册。`mixin([(name, ConfigMacro::new(callback))], true)` 替换同名宏；传 false 则保留已有宏。`mixin_using` 按顺序执行工厂：传 false 时已有名称连工厂也不执行；工厂失败保留此前注册，后续工厂不再执行。回调和被替换的捕获值在注册锁外执行或释放，可以再次注册、调用和清空宏。

注册表归所属应用，替换默认仓储后仍保留，HTTP、命令和 Worker 共用该应用的注册；不同应用互不污染。应用上下文外的独立 `Repository::new` 拥有自己的注册表。自定义仓储可以覆写对象安全的注册、检查、调用或 `macro_registry` 契约。即使注册同名宏，`Config::get` 等普通方法仍按原行为执行；`macro_call` 显式调用指定宏。

长期保存的回调要求 `Send + Sync + 'static`，同步调用的参数和结果只要求 `Any`，可传递不可复制对象或 `Rc`。宏未注册、参数／结果类型不符和回调失败均明确报错。具类型调用替代动态方法语法，无需取得底层对象。

## 编辑环境文件 {#editing-environment-files}

`EnvironmentFile` 修改磁盘中的环境文件；`Config::set` 只修改当前配置仓储，`Environment` 则是不可变的启动快照。写文件不会自动更新后两者，也不会重启服务。

```rust
use openrust::config::EnvironmentFile;

// 应用引导后，使用实际选中的 .env 或 .env.<环境名>。
let file = EnvironmentFile::for_application(&app)?;
// 也可显式打开已有文件；create(path, content) 创建时不覆盖已有目标。
// let file = EnvironmentFile::open(project_root.join(".env"))?;
file.set("APP_NAME", "我的应用")?;
file.set_many([("MAIL_HOST", "smtp.example.test"), ("MAIL_PORT", "587")])?;
let name = file.get("APP_NAME")?;
let exists = file.contains("MAIL_HOST")?;
let raw = file.get_raw("APP_NAME")?; // 带引号的原始值，不展开引用
let values = file.all()?;
file.remove("OBSOLETE_SETTING")?;
```

需要一次保存多种变更时，创建独立编辑会话：

```rust
let mut editor = file.edit()?;
editor.set("APP_NAME", "我的应用")?
    .set_expression("MAIL_FROM_NAME", "${APP_NAME}")?
    .remove("OBSOLETE_SETTING")?;
let changed = editor.save()?;
```

`set` 写入字面字符串，转义美元符号、引号和反斜线。需要 dotenv 变量展开时显式使用 `set_expression`，不会执行 shell 命令。`get`、`all` 使用应用的 dotenv 加载器解析，展开引用时也遵循其进程环境回退规则；结果均为字符串，`null`、`false` 仍是文本。`get_raw` 返回带原有引号的值。目标变量重复定义时拒绝操作；`all` 对重复项沿用加载器的最后定义规则。未修改项的引用、注释、`export`、空白、UTF-8 BOM 和换行格式保留原样。

直接写入先取得文件锁再读取；编辑会话保存时发现原文件变化返回 `Conflict`，锁被占用返回 `Busy`，待对方完成后重试。批量操作失败不改文件，无变化时不更新修改时间。同目录临时文件完成写入后原子替换，保留权限及 Unix 所有者并在 Unix 同步落盘；Windows 使用 `ReplaceFileW` 保留目标 ACL。Unix 新文件仅所有者可读写，Windows 遵循目录 ACL。稳定的 `<文件名>.lock` 锁文件在可能存在写入者时必须保留，日常清理不要删除。

仅支持不超过 8 MiB 的普通 UTF-8 文件，拒绝目标符号链接及 Windows 重解析点。新变量名以 ASCII 字母或下划线开头，后续允许字母、数字、下划线和点。只读文件或语法错误不会被替换。协作写入者通过同一锁串行处理；绕过该锁的外部编辑器仍可能与保存竞争。`DurabilityUncertain` 表示替换已完成但目录同步失败，重试前应先核查磁盘内容。Windows 返回 `ReplacementUncertain` 时会保留恢复文件并报告路径，重试前应核对目标及恢复文件。 错误及编辑器调试输出不含变量值，`content`、`all` 和读取结果含明文，不应直接记录到日志。

生产环境改完后应显式执行 `artisan config:cache` 并重启常驻 HTTP、队列和调度进程；未缓存时，新启动的应用读取新文件。已有快照不变，进程环境覆盖的优先级保持原样。`key:generate` 和 `init` 共用文件写入后端，继续保留各自的密钥校验、生产确认和初始化规则。

### 插入位置与空行 {#environment-file-layout}

```rust
use openrust::config::{EnvironmentDocument, EnvironmentWriteOptions};

let options = EnvironmentWriteOptions::new()
    .after("DB_*")
    .blank_lines(1);
file.set_with_options("SMS_DRIVER", "log", &options)?;
```

使用 `before(key)`、`after(key)`、`at_start()` 或 `at_end()` 指定位置。`blank_lines(1)` 在前后各留一行空行；`blank_lines_before(n)`、`blank_lines_after(n)` 分别控制两侧。零表示没有额外空行，声明之间必要的换行仍然存在。未指定的间距保持原样；显式数量只整理紧邻的空白行，不删除注释。文件首尾也可显式保留指定数量的空行。继续保留 LF／CRLF 和 UTF-8 BOM。

已有变量默认原位更新，显式空行设置仍然生效；需要重新定位时加上 `.move_existing(true)`。新增和显式移动才解析位置锚点：找不到锚点或锚点重复时拒绝修改，相对于自身定位也会报错。移动会保留声明的 `export` 和行尾注释，独立注释留在原位置。重复执行不会积累空行。移动声明可能影响变量引用的展开顺序。表达式使用 `set_expression_with_options`，多次编辑可放在同一文件编辑会话中，最后统一保存。

锚点还支持 `*` 通配零个或多个字符，按完整键名匹配并区分大小写。例如 `after("DB_*")` 放在文件中最后一个匹配声明后，`before("DB_*")` 放在第一个匹配声明前。支持前缀、后缀及多个星号；点号按字面匹配，`DB.*` 可匹配 `DB.host`。它不是正则表达式，不支持 `?` 或字符组 `[]`。注释、变量值及多行值中的文本都不会成为锚点。

显式移动时，通配匹配会排除被移动变量自身，重复操作不会改变位置或累积空行。没有其他匹配项时返回 `MissingAnchor`，不修改文档；匹配到同一个键的重复定义时返回 `DuplicateKey`，多个不同键则正常作为一组。已有变量仍默认原位更新，只有 `move_existing(true)` 才移动。

Windows 已知替换窗口内的瞬态缺失会等待写入者，最长 100 ms；锁仍被占用则返回 `Busy`，真正缺失的文件仍报错。

同一编辑引擎也可以脱离应用、容器和磁盘文件使用：

```rust
let mut document = EnvironmentDocument::parse("APP_NAME=Example\n")?;
document.set_with_options(
    "SMS_DRIVER", "log",
    &EnvironmentWriteOptions::new().after("APP_NAME").blank_lines(1),
)?;
let output = document.content();
```

`EnvironmentDocument` 支持同样的读取、字面值／表达式写入、批量修改和删除，也可用 `new()` 创建空文档。它只修改自身文本；二次开发可将 `content()` 交给自己的存储适配器，并自行处理持久化与并发策略。需要内置文件锁和原子保存时使用 `EnvironmentFile::open(path)`。`for_application` 只是可选的快捷入口，不要求固定文件名 `.env`，也不要求应用容器。

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
        .with_config(|_, _| {
            Ok([("app".into(), serde_json::json!({"env": "staging"}))].into())
        })
        .create()?;
    app.boot().await?;

    assert_eq!(app.environment(None)?, EnvironmentResult::from("staging"));
    assert!(bool::from(app.environment(Some("stag*".into()))?));
    assert!(bool::from(app.container().current_environment_is(&["local", "staging"])?));
    assert!(!bool::from(app.container().current_environment_is(&[])?));
    Ok(())
}
```

引导阶段将检测到的环境名以 `Arc<str>` 保存到 `env` 服务。环境查询读取该服务。启动后修改 `app.env` 只更新配置。

<a id="public-files"></a>

## 公共文件

| 环境变量 | 用途 | 默认值 |
|---|---|---|
| `APP_PUBLIC_PATH` | `filesystems.disks.web.root` / `Application::public_path()` | `public` |
| `FILESYSTEM_WEB_ROOT` | 网站磁盘根目录，优先于 `APP_PUBLIC_PATH` | `app.public_path` |
| `FILESYSTEM_WEB_URL` | `filesystems.disks.web.url` | `app.url` |
| `FILESYSTEM_LOCAL_ROOT` | 私有本地磁盘的 `filesystems.disks.local.root` | `storage/app/private` |
| `FILESYSTEM_PUBLIC_ROOT` | 公共本地磁盘的 `filesystems.disks.public.root` | `storage/app/public` |
| `FILESYSTEM_PUBLIC_URL` | 公共本地磁盘的 `filesystems.disks.public.url` | `app.url` 加 `/storage` |

`FILESYSTEM_WEB_ROOT` 优先指定网站磁盘根目录；未设置时使用 `APP_PUBLIC_PATH`（默认 `public`）。相对路径基于应用根目录，绝对路径直接使用。空字符串或全空白值会导致配置加载失败。

未设置 `APP_PUBLIC_PATH` 时，`config/app.rs` 明确将 `app.public_path` 设为 `public`，`web` 磁盘复用该值作为默认根目录。网站文件配置在 `filesystems.disks.web`，公开上传文件配置在 `filesystems.disks.public`。默认路由按顺序调用 `Route::static_disk("web")` 和 `Route::static_disk("public")`。引导前调用 `use_public_path()` 会覆盖网站目录并同步 `web.root`。相对目录以应用根目录为基准。挂载前缀取磁盘 URL 的完整路径，例如 `/storage/tmp/` 挂到 `/storage/tmp`。绝对 URL 必须与 `app.url` 同源。修改挂载配置后重启应用。 显式设置为空字符串或全空白的 `APP_PUBLIC_PATH` 会导致配置加载失败。

## 维护模式 {#maintenance-mode}

部署时运行 Artisan 的 `down`，应用返回 HTTP 503。运行 `up` 恢复访问。运行中的服务会逐请求读取共享状态，可以再次执行 `down` 更新选项。

```bash
./artisan down --retry=60 --with-secret
./artisan up
```

`--retry` 接受正数秒数或重试日期，`--refresh` 指定浏览器刷新间隔。未注明时区的日期时间使用应用时区，响应头统一使用 GMT。`--redirect` 接受站内路径。`--render` 在执行 `down` 时将命名视图渲染一次，并传入 `$retryAfter`。`--status` 可覆盖默认的 503。期望 JSON 的请求沿用常规异常渲染，跳过浏览器跳转和预渲染页面。

维护期间可打开 `--with-secret` 打印的 URL 检查站点。应用设置有效期为 12 小时的签名 Cookie，当前会话没有原目标时跳转至首页。可以用 `--secret` 指定自己的路径。配置的健康路径（默认 `/up`）在维护期间仍可访问。需要保留其他端点时，在 `bootstrap/app.rs` 的 `with_middleware` 中配置。

```rust
.with_middleware(|middleware| {
    middleware.prevent_requests_during_maintenance(["status/*"]);
})
```

使用文件驱动部署时，`./artisan down --render=errors::503` 可在 Provider 启动期间提供预渲染页面。可在 `resources/views/errors/503.blade.html` 自定义页面。监听配置需成功加载。JSON 请求、排除路径和有效绕过请求等待应用内核。缓存驱动由运行中的内核提供维护响应。队列行为见[维护期间的 Worker](../digging-deeper/queues.md#workers-during-maintenance)。

启动期间请使用 Artisan 打印的完整绕过 URL。启动前处理器比较原始请求地址，查询串和尾斜杠都会参与匹配。HTTP 内核启动后，维护中间件按路径匹配，忽略查询串及路径两端的斜杠。

### 共享维护状态 {#shared-maintenance-state}

应用通过 `maintenance_mode()` 提供维护状态。默认 `file` 驱动把数据写入 `storage/framework/down`。选择 `cache` 和共享缓存仓库，可让多台机器上的 Worker 读取同一状态。`array` 驱动将状态保存在当前应用实例内，可用于测试。

```rust
use openrust::foundation::Application;
use serde_json::{Map, json};

async fn activate_maintenance(
    app: &Application,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let payload = Map::from_iter([("status".into(), json!(503))]);
    app.maintenance_mode()?.activate(&payload).await?;
    Ok(())
}
```

用 `is_down_for_maintenance().await` 读取状态，用 `app.maintenance_mode()?.deactivate().await?` 清除。队列 Worker 在预约任务前读取此状态。

### 维护模式门面与驱动 {#maintenance-facade}

`MaintenanceMode` 使用 HTTP、Artisan 和调度器共用的应用管理器。`get_default_driver` 读取 `app.maintenance.driver`，缺失时默认 `file`。`driver(None)` 选择默认驱动；`driver(Some("file"))`、`driver(Some("array"))` 和 `driver(Some("cache"))` 显式选择内置驱动。已解析驱动在应用内共享，不同应用相互隔离。

```rust
use openrust::{foundation::maintenance::MaintenanceError, support::facades::MaintenanceMode};
use serde_json::{Map, json};

pub async fn update_maintenance() -> Result<(), MaintenanceError> {
    MaintenanceMode::activate(&Map::from_iter([("status".into(), json!(503))])).await?;
    assert!(MaintenanceMode::active().await?);
    let _payload = MaintenanceMode::data().await?;
    MaintenanceMode::deactivate().await
}
```

`extend(name, creator)` 注册创建器，接收 `&Container` 并返回 `Arc<dyn contracts::foundation::MaintenanceMode>`。已有驱动继续使用，直到 `forget_drivers` 清空实例。创建器不持注册表锁，可解析其他驱动；并发解析同一驱动只执行一次创建。循环依赖返回 `MaintenanceError::DriverResolutionCycle`。`get_drivers` 返回已解析句柄，清空后旧句柄仍有效。后续 HTTP 请求、命令和调度器解析当前绑定，也遵循显式替换的 MaintenanceMode Contract。

`get_container` 返回所属容器。`set_container(&Arc<Container>)` 影响后续创建，保留管理器构造时取得的配置、已解析驱动和创建器。容器使用弱持有，所属应用释放后访问会报错。这些入口用于嵌入应用的生命周期控制，不宜作为逐请求配置。

缓存驱动记录 `app.maintenance.store`，未指定时回退 `cache.default`，每次操作重新解析该具名仓储，因此能跟随仓储替换。写入使用 `put_with_default`：初始有效期为 3600 秒，仓储默认时长设为 `None` 时永久保存。文件维护状态保留至显式清除。没有状态时 `data()` 返回 `None`，后端、解码及拒绝写入的错误向上传递。`array` 仅限应用进程内，不能协调独立的服务器或 Worker 进程。

## 测试环境与启动快照 {#testing-environment}

`Application::running_unit_tests()` 和应用契约返回 `Result<bool, BindingResolutionError>`。仅当容器中的 `env` 精确为 `testing` 时结果为 true，与是否正在运行测试程序无关。环境未绑定时返回 `Ok(false)`，不触发服务解析；解析错误沿 `Result` 传播。后续修改 `config("app.env")` 不会替换已检测的环境绑定。

启动后，通过 `app.make::<openrust::config::Environment>().await?` 读取当前应用不可变的环境快照。既有 `get`、`get_utf8` 返回原始字符串，具体旗标仍由对应包解析。快照遵守进程变量优先、环境文件选择及配置缓存规则，不重新加载文件，也不修改进程全局变量。

## 应用存储路径 {#application-storage-path}

`app.storage_path("")` 返回存储根目录，传入 `"pail"` 等字符串可获取子路径。`app.use_storage_path(path)?` 覆盖它并重绑 `path.storage`，应用契约也提供 getter。`openrust::support::storage_path(path)?` 读取当前应用作用域。只读的 `foundation::Paths` 服务让编译型包共享同一状态，避免通过容器强持有 Application。

优先级是非空且非 `0` 的显式覆盖、应用环境快照中的 `APP_STORAGE_PATH`、应用根目录下的 `storage`。环境中显式存在的空值仍会保留。Setter 传入相对路径时保持相对。拼接追加本机分隔符并去掉子路径开头的本机分隔符，保留重复分隔符和 `..`。Rust 没有默认参数，读取根目录时显式传 `""`。

构造时初始化 `path.storage`，后加载的环境文件可以改变 getter，但不会自动重写该绑定；`use_storage_path` 则显式重绑，沿用 顺序。重绑错误通过 `Result` 返回，此时路径状态已经更新。

日志、文件 Session、文件缓存数据与锁、编译视图以及默认 local/public 磁盘在加载配置时采用存储根目录。`FILESYSTEM_LOCAL_ROOT`、`FILESYSTEM_PUBLIC_ROOT` 和 `VIEW_COMPILED_PATH` 仍可单独覆盖，公共 URL 和挂载顺序不变。`storage:link` 指向配置的公共磁盘。启动前设置绝对路径的 `APP_STORAGE_PATH`，或在加载配置前调用 `use_storage_path`。

编译型配置加载器使用 `env.storage_path(base_path, subpath)` 作为 Rust 上下文适配。弱引用选取所属应用的路径，不依赖全局 Facade，也不延长应用生命周期。独立环境快照按自身变量及传入的默认根目录解析。后续更改应用路径不会回写已经加载或缓存的配置值。

文件维护模式每次操作都解析当前路径，服务已解析后仍有效。启动前的预渲染检查使用 boot 前可用的路径：此时应提供进程变量或显式 setter，仅在 dotenv 中设置的值尚不可用。Pail 在首次解析 Files 时确定目录，应在首次使用前配置。配置、路由、事件和包缓存产物的位置继续遵守各自的缓存路径契约。

升级已有项目时，请按[语言标识升级说明](../digging-deeper/localization.md#bcp47-upgrade)同步迁移语言资源、持久化值与缓存。
