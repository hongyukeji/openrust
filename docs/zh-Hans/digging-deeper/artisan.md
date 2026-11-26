# Artisan 命令行

## 简介 {#introduction}

Console Kernel [引导应用](../architecture-concepts/lifecycle.md)后分派 Artisan 命令。应用骨架的命令列表和帮助无需 HTTP 监听或数据库连接即可运行。

```sh
./artisan list
./artisan list cache --format=json --short
./artisan help list
./artisan list --help
./artisan inspire
```

默认控制台路由注册了 `inspire`。执行 `./artisan inspire` 会随机输出一条引言及其作者，退出码为 0。

名言库与固定版框架一致，共 41 条。支持颜色的终端会将正文显示为粗体、署名显示为灰色。重定向时默认输出纯文本。可以用 `--ansi` 强制开启样式，或用 `--no-ansi` 关闭。

Windows 上使用 `artisan.bat`。编译后的应用通过 `main artisan` 接收相同的命令。

列表和帮助成功时返回退出码 0。未知命令和非法参数返回 1。标记为 `Planned` 的条目可以查看帮助，执行时返回 1。

生成到 `app/console/commands` 的命令在构建时注册。需要显式注册时，使用 `with_commands` 和 `app/console/commands/mod.rs`。在 `bootstrap/app.rs` 的 `with_routing` 中声明 `routes/console.rs` 的控制台路由。应用命令可以替换同名占位项。

`routes/console.rs` 导入 `openrust::support::facades::Artisan`，通过 `Artisan::command("inspire", …)` 向 `register()` 执行期间绑定的控制台注册闭包，通过 `purpose()` 设置帮助和命令列表中的说明。闭包接收命令输出对象，正常返回时退出码为 0。

`Application::handle_command` 调用 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引导后加载延迟 Provider，再登记命令清单、框架内置命令和应用命令，最后分派。


### 在 Docker 中运行 Artisan {#laravel-sail}

在应用根目录，HTTP 和 Artisan 使用同一份 [Docker Compose 配置](../getting-started/deployment.md#docker-deployment)。应用容器运行时用 `exec`；应用停止后如只需查看命令帮助，可用一次性的 `run`。

```sh
docker compose --env-file .env -f docker/docker-compose.yml exec -T app /app/main artisan list
docker compose --env-file .env -f docker/docker-compose.yml run --rm --no-deps app artisan help migrate
```

## 源码与发布入口 {#source-and-release-entrypoints}

源码 `artisan` 脚本先进入应用目录，再将参数原样交给 Cargo，保留空格、空参数和非 ASCII 文本。脚本返回命令的退出码。

独立构建使用 `./main artisan list`、`./main artisan help inspire` 或 `./main artisan inspire`，无需 Cargo。配置相对于可执行文件解析，从其他工作目录启动也可运行。参阅[部署](../getting-started/deployment.md)。

直接运行编译后的 `main` 会按配置[启动应用服务](../getting-started/deployment.md#一起运行服务)。用 `main artisan` 单独执行控制台命令，不启动这些服务。无法识别的首参数会在创建应用前显示程序用法并返回退出码 2。Artisan 分派错误返回 1。

在 CMD 中运行 `artisan.bat inspire`。在 PowerShell 中使用 `.\artisan.bat help inspire --format=json`。源码启动器需要在 `PATH` 中找到 `cargo.exe`，执行前会切换到应用目录。在 CMD 中用 `%ERRORLEVEL%` 读取退出码，在 PowerShell 中用 `$LASTEXITCODE`。[Windows 独立发布版](../getting-started/deployment.md#windows-standalone)使用 `.\main.exe artisan inspire`。

### 开发 HTTP 服务 {#development-http-server}

用 `./artisan serve` 单独启动 HTTP，默认监听 `127.0.0.1:8000`。可用 `SERVER_HOST` 和 `SERVER_PORT` 更改默认值，也可在本次运行时传入 `--host` 和 `--port`。未明确指定端口时，如果端口被占用，`--tries` 限制继续尝试的端口数，默认额外尝试 10 个。

```sh
./artisan serve
./artisan serve --host=127.0.0.1 --port=8080 --no-reload
```

选中的环境文件发生变化时，服务会在原监听端口重启。用 `--env=local` 选择环境，用 `--no-reload` 保持当前配置直到停止。按 Ctrl+C 停止服务。不带模式参数运行编译后的 `main` 可[一起启动已配置的服务](../getting-started/deployment.md#一起运行服务)。

### 退出码 {#exit-codes}

只接收输出的闭包正常返回时，退出码为 0。类型化命令和可失败闭包用 `Ok(0)` 表示成功，也可用 `Ok(code)` 指定退出码。`Artisan::call` 返回该值，命令行启动器也使用该值。

通过 `?` 传播 `Input::fail` 或 `CommandIo::fail` 的结果，也可以直接返回。Artisan 停止命令，通过错误组件输出消息并返回退出码 1。`fail_default` 使用当前应用语言。其他 `ConsoleError` 仍进入异常处理。

```rust
use openrust::console::{ConsoleError, Input, Output};
use openrust::support::facades::Artisan;

fn check_readiness(input: &Input) -> Result<(), ConsoleError> {
    if input.argument("state").and_then(|state| state.as_str().map(str::to_owned)).as_deref()
        != Some("ready")
    {
        input.fail("The job is not ready.")?;
    }
    Ok(())
}

fn register_exit_codes() -> Result<(), ConsoleError> {
    Artisan::command_with("jobs:check {state}", |input: &Input, output: &mut Output| {
        check_readiness(input)?;
        input.io(output).line("Ready")?;
        Ok(0)
    })?;
    Ok(())
}
```

## 开发进程 {#the-dev-command}

在应用目录下并发运行已登记的开发进程。默认的开发服务器和队列监听器直接复用 Artisan 的当前二进制。Vite 等 Node 进程按需使用 Node。

```sh
./artisan dev
./artisan dev --tabs
./artisan dev --stream
./artisan dev --inline --timestamps --no-restart
./artisan dev --json
```

输出按进程加上标签，保留各自的 stdout 和 stderr。`--timestamps` 给内联输出加 UTC 时间。`--json` 每行输出一条带版本号的事件，包括进程启动、输出、退出、重试和最后的 `done`。

### 重启失败进程 {#restarting-failed-processes}

运行满一秒后失败的进程每隔一秒重试，最多五次。成功退出的进程结束，一秒内失败则立即报告。用 `--no-restart` 关闭本次重试，或在应用里用 `DevCommands::disable_auto_restart()` 关闭。用 `DevCommands::with_timestamps()` 默认开启时间戳。第一个永久失败决定最终退出码，其他进程可继续运行。

Unix 上的可用终端默认进入标签视图，也可用 `--stream` 直接进入合并视图。重定向输出、尺寸不足的终端、Windows、`--inline` 和 `--json` 走内联输出路径。交互会话正常退出时返回 0，内联和 JSON 模式则报告第一个永久失败的退出码。

按 `1`–`9` 选择标签，也可用 Tab 聚焦侧栏后用方向键选择。按 `s` 切到流式视图、`t` 返回标签、`f` 筛选流、`r` 重启当前标签的进程、`c` 清空当前视图。用 `/` 搜索，再按 `n` 或 `N` 切换匹配。用方向键或 Page Up、Page Down 滚动日志。

`--buffer-size` 和 `--stream-buffer-size` 控制每个标签和流式视图保留的行数，分别默认 2,000 和 10,000 行。应用可用 `DevCommands::buffer_size()`、`stream_buffer_size()` 配置，并用 `tabs()`、`stream()` 或 `inline()` 设置默认模式。

按 `q` 或 Ctrl+C 退出。Unix 终止信号也会请求关停，最多等子进程退出六秒。Artisan 恢复终端，并把保留的流式日志打印到对应的输出流。可选 [Pail 命令](../the-basics/logging.md#tailing-log-messages-using-pail)也可独立运行。

在 Windows 控制台按 Ctrl+C 请求关停。每条开发命令都运行在自己的 Windows 作业中。Artisan 向该命令的控制台进程组发送 Ctrl+Break，并在宽限结束后回收仍在运行的后代进程。启用 `--no-restart` 时，一条命令失败也会关停其他开发命令并返回失败码。

### 查看开发进程

用 `dev:list` 查看当前应用登记的开发进程。默认列出 [HTTP 开发服务](#development-http-server)和队列监听器。应用含有 `package.json` 时，也会按锁文件选择 Bun、pnpm、Yarn 或 npm 并列出 Vite。查询不会启动进程。加载可选 Pail Provider 后，会在 Vite 之前添加 `logs`，执行 `pail --timeout=0`。它属于框架默认进程，排除 vendor 不会移除它，`without_default_commands()` 会移除它。包或应用可以用同名 `logs` 条目覆盖。

```sh
./artisan dev:list
./artisan dev:list --json
./artisan dev:list --no-interaction --filter=queue
```

非交互环境也会输出 JSON。`--filter` 按名称或命令的子串筛选，`--except-vendor` 和 `--only-vendor` 筛选包登记的进程。筛选无结果时退出码为 1。

### 自定义开发进程 {#customizing-dev-processes}

在应用服务提供者的 `boot` 方法中，通过 `openrust::foundation::DevCommands` 登记进程。同名进程会覆盖默认项，应用登记优先于包登记。

```rust
use openrust::{console::ConsoleError, foundation::DevCommands};

fn register_development_processes() -> Result<(), ConsoleError> {
    DevCommands::artisan("serve --port=9000", Some("server"))?.green();
    DevCommands::node("storybook", Some("storybook"))?.blue();
    DevCommands::node_exec("tailwindcss --watch", Some("tailwind"))?.color("#ff6347");
    DevCommands::register("echo ready", Some("ready"))?.yellow();
    DevCommands::only(&["server", "tailwind", "ready"])?;
    DevCommands::except(&["ready"])?;
    Ok(())
}
```

### 筛选开发进程 {#filtering-dev-processes}

示例保留 `server` 和 `tailwind`。在服务提供者的 `boot` 方法中调用注册函数。`only` 和 `except` 会同时生效，同一进程出现在两个列表时会被排除。

Shell 命令用 `register` 登记，包管理器命令用 `node` 或 `node_exec`。名称传 `None` 时使用命令首词。用 `only`、`except`、`order`、`without_vendor_commands` 和 `without_default_commands` 调整应用清单。框架依据 Rust 调用者的源码路径识别包登记。

## 命令帮助和输出格式 {#command-help-and-output-formats}

`./artisan help migrate` 与 `./artisan migrate --help` 从同一命令定义读取参数、选项和说明。查看帮助时不会执行目标命令。精确的命令名称和别名优先，可按完整名称或冒号分隔的各段使用唯一缩写。

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` 和 `help` 支持 `txt`、`json`、`xml` 和 `md`。普通文本列表会先显示全局选项，再列出命令。指定命令组时也会显示全局选项。`list --raw` 只输出平铺列表。`help --raw` 输出完整的纯文本帮助，启用 ANSI 时也会去掉样式。`list --short` 保留名称、别名和实现状态，省略命令明细。各语言的结构化输出使用相同的命令和选项标识。帮助和列表成功时退出码为 0。参数错误、歧义和未知命令的退出码为 1。

`--quiet` 隐藏普通输出。`--silent` 同时隐藏控制台错误。命令定义也可使用全局 `--no-interaction`、`--ansi`、`--no-ansi`、`--version` 和详细程度选项。

语言选择与覆盖见 [Artisan 翻译](./localization.md#artisan-translations)。

## 在代码中执行命令 {#programmatically-executing-commands}

在 HTTP 处理器或另一个 Artisan 命令中，用 `Artisan::call(name, arguments, &mut output).await` 执行已注册的命令。命令在当前应用内运行并返回退出码。未知命令和无效参数返回 `ConsoleError`。每次调用用独立的 `Output` 收集标准输出和标准错误。

```rust
use std::ffi::OsString;

use openrust::{
    console::{ConsoleError, Output},
    support::facades::Artisan,
};

async fn inspire_help() -> Result<(i32, String), ConsoleError> {
    let mut output = Output::default();
    let arguments = ["inspire", "--format=json"].map(OsString::from);
    let status = Artisan::call("help", &arguments, &mut output).await?;
    Ok((status, output.stdout))
}
```

令牌入口保留参数边界，包括空格和空值。完整命令字符串可用 `Artisan::call_line`，它解析字符串中的引号和转义，然后调用应用内已注册的命令。 用 `Artisan::call_silently(name, arguments).await` 丢弃两个输出流并保留退出码。

#### 传递数组值 {#passing-array-values}

使用 `Artisan::call_named` 传入参数名称与选项标识。选项声明支持多个值时可传数组，命令定义仍负责校验。

#### 传递布尔值 {#passing-boolean-values}

布尔开关可传入 `true` 或 `false`。具名入口按声明的选项动作编码开关。未知或重复的参数键会返回错误。

```rust
use openrust::{console::{ConsoleError, Output}, support::facades::Artisan};
use serde_json::json;

async fn send_reports(output: &mut Output) -> Result<i32, ConsoleError> {
    Artisan::call_named(
        "reports:deliver",
        &[
            ("user", json!("Ada Lovelace")),
            ("--id", json!([5, 13])),
            ("--urgent", json!(false)),
        ],
        output,
    ).await
}

async fn send_from_line(output: &mut Output) -> Result<i32, ConsoleError> {
    Artisan::call_line(
        r#"reports:deliver "Ada Lovelace" --id=5 --id=13 --urgent"#,
        output,
    ).await
}
```

### 将 Artisan 命令加入队列 {#queueing-artisan-commands}

用 `Artisan::queue` 准备命令。调用 `dispatch().await` 提交任务并取得派发结果。用 `on_connection` 和 `on_queue` 指定目标，也可以使用默认连接及其默认队列。

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};

async fn queue_inspire() -> QueueResult<DispatchResult> {
    Artisan::queue("inspire", &[])
        .on_connection("redis")
        .on_queue("commands")
        .dispatch()
        .await
}
```

用 `./artisan queue:work redis --queue=commands` 启动对应的 Worker。载荷以 JSON 保存命令名和 UTF-8 参数。Worker 在自身应用中解析命令，丢弃控制台输出，并确认正常返回的任务，即使命令退出码非零。需要走队列重试和失败记录时，在命令中返回 `ConsoleError`。

#### 排队命令的具名参数 {#queue-named-commands}

命令需要具名、数组或布尔值时，使用 `Artisan::queue_named`。派发时 Artisan 解析注册定义，将参数编码为 Worker 已使用的 CLI 令牌。未知参数名称会在派发前报错。

选用 Redis 时，调用 `queue_report("redis")`，然后运行 `./artisan queue:work redis --queue=commands`。任务执行时，Worker 按自己的应用重新解析命令。

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};
use serde_json::json;

async fn queue_report(connection: &str) -> QueueResult<DispatchResult> {
    Artisan::queue_named(
        "reports:deliver",
        &[
            ("user", json!("Ada Lovelace")),
            ("--id", json!([5, 13])),
            ("--urgent", json!(false)),
        ],
    )
    .on_connection(connection)
    .on_queue("commands")
    .dispatch()
    .await
}
```

## 从其他命令调用 {#calling-commands-from-other-commands}

用 `CommandIo::call` 在父命令当前输出中执行子命令。具名参数可用 `call_named`。用 `call_silently` 丢弃子命令输出并保留退出码。调用继承父命令的 ANSI、交互、静默及详细程度选项。子命令显式选项优先。

可把任意 `AsOutput` 适配器传给 `Artisan::call`，包括控制台分区或应用自定义包装器。普通输出保留各条流。选定分区接收本次调用的缓冲输出。用 `call_captured` 同时返回退出码和独立输出缓冲。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, Output},
    support::facades::Artisan,
};

fn register_nested_commands() -> Result<(), ConsoleError> {
    Artisan::command("reports:prepare", |out| out.stdout.push_str("Preparing\n"))?;
    Artisan::command_with("reports:parent", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let status = io.call("reports:prepare", &[]).await?;
            if status != 0 {
                return Ok(status);
            }
            io.call_silently("reports:prepare", &[]).await?;
            io.line("Ready")?;
            Ok(0)
        })
    }))?;
    Ok(())
}

async fn capture_report() -> Result<(i32, Output), ConsoleError> {
    Artisan::call_captured("reports:parent", &[]).await
}
```

## 按需安装路由 {#optional-route-installers}

在源码目录运行以下命令，生成路由文件并更新模块声明和启动配置，然后重新构建。已有路由文件会保留。

```sh
./artisan install:broadcasting
./artisan install:grpc
```

使用 `./artisan install:api` 创建并注册 API 路由。默认保留已有路由。功能开关和迁移选项见 [Sanctum 安装](../packages/sanctum.md#installation)。 广播接入后会注册 `/broadcasting/auth`，授权需要广播驱动和频道规则。gRPC 需要注册 Tonic 服务，并设置 `GRPC_HOST` 和 `GRPC_PORT` 才开启监听。

## 应用初始化 {#complete-installation}

在源码项目运行 `./artisan init`，Windows 使用 `artisan.bat init`。向导读取已有值，引导应用、数据库、缓存、Session 和队列设置，密码隐藏输入，连接检查通过后保存。无交互模式读取项目环境。当前初始化支持 SQLite、MySQL／MariaDB 和 PostgreSQL，以及本地／数据库／Redis 服务和日志／SMTP 邮件和内置短信供应商。API 使用 Sanctum，同时安装频道和 gRPC 注册。Reverb 包自动安装，是否随主应用启动由 REVERB_ENABLED 控制；仅生成缺失的 Reverb 凭据，保留已有值。初始化默认安装全部已支持扩展，应用骨架本身仍保持轻量。

```sh
./artisan init --dry-run
./artisan init
./artisan init --no-interaction
./artisan init --extensions=api --no-interaction
```

`--extensions=all` 是默认集合和向导推荐项，也可选择 `api,broadcasting,reverb,grpc,passport,fortify,passkeys,socialite,precognition,oidc,sso`。`api` 安装 Sanctum；`passport` 自动补选 `api`，`passkeys` 补选 Fortify，`sso` 补选 OIDC 和 Fortify，`reverb` 补选广播频道。后续运行可补装扩展，已配置 Reverb 时须纳入选择。API `/user` 路由保留 `auth:sanctum`，Passport 使用独立 `api` 守卫；已有不兼容守卫会保留并报告冲突。

Fortify 安装认证动作、应用 Provider、配置和迁移；初始化新生成的配置使用 `views=false` 并启用邮件验证，已有自定义配置保留。请通过 `ResetPassword::create_url_using` 配置前端密码重置地址，或提供名为 `password.reset` 的路由；无界面初始化不生成前端页面。按选择启用 Passkeys。新配置须将 APP_URL 设为带域名、无路径的精确 HTTPS Origin；开发环境允许 HTTP localhost，例如 `APP_URL=http://localhost:8000`。HTTP IP 地址会在写入前被预检拒绝。Socialite、OIDC 启用对应包，供应商凭据、获准回调、本地账户显式绑定及应用路由仍由应用配置；选择 SSO 不会创建身份提供方或配置全局退出。Precognition 已内建，在选定路由添加 `precognitive`。安装状态与这些外部前置分开报告。

缺少 `.env` 时从示例创建，保留已有密钥和凭据；缺少应用密钥时调用 `key:generate`。共享安装服务在源码锁内发布所选资源；当前程序未编入安装器时，集中重建一次加载安装服务，随后统一构建全部生成模块并执行一次原迁移入口，不递归运行单项安装命令。生产环境需要确认或 `--force`。初始化成功后执行 `cargo run`，命令不自动启动长期服务。`--without-migration` 报告准备完成、迁移待执行。`--force` 覆盖可发布配置和脚手架并授权生产迁移，保留 APP_KEY、Passport 密钥、无关应用模块及全部历史迁移。`--dry-run` 不连接、不写文件、不加锁、不构建、不迁移。必需步骤失败返回非零，停止依赖步骤并保留完成内容供重试；并发源码安装立即报告冲突。

选择 SMTP 后复用现有传输层检查连接、TLS 和认证，不发送测试邮件。向导收集内置短信供应商凭据；发送身份和业务模板仍按各供应商配置，不自动发短信，也不声称真实投递通过。启用 Reverb 时引导监听及发布地址，默认同进程运行；已有配置始终作为向导默认值。

使用 `--env=acceptance` 初始化 `.env.acceptance`；清理配置、生成密钥和迁移步骤使用同一环境，不修改 `.env`。

## 编写命令 {#writing-commands}

用 `make:command` 生成命令文件。先定义参数和选项，再实现业务逻辑。

## 生成命令 {#generating-commands}

破坏性命令可调用 `openrust::console::confirm_to_proceed(production, input.matches())?`，仅在返回 `true` 时继续。`production` 由应用环境或命令确认条件决定。复用的多语提示默认拒绝，非交互输入拒绝；命令声明的 `--force` 可跳过确认。

在源码检出目录生成命令。生成的类型实现 Artisan 命令契约。

```sh
./artisan make:command SendReport
./artisan make:command Reports/ArchiveReport --command=reports:archive
```

第一条命令写入 `app/console/commands/send_report.rs`，命令签名为 `app:send-report`。第二条在 `reports` 模块下生成命令并显式指定签名。使用 `-f` 或 `--force` 覆盖已有文件。

在生成的 `handle` 方法中填写命令逻辑，然后重新构建。默认模板声明 `COMMAND` 注册入口，构建时自动发现 `app/console/commands` 下的文件并注册到 Artisan。编译完成后可运行 `./artisan app:send-report` 或 `./artisan help app:send-report`。

在应用根目录放置 `stubs/console.rs.stub` 可自定义生成内容。生成器替换 `{{class}}` 和 `{{command}}`。

自定义模板保留顶层的 `COMMAND` 注册入口即可参与自动发现。没有这个入口的旧文件可以沿用显式模块和注册。可在 `build.rs` 的 `COMMANDS` 中添加应用命令目录。

### 生成配套测试 {#generating-an-accompanying-test}

添加 `--test` 可在生成命令时同时生成一个功能测试。

```sh
./artisan make:command SendReport --test
```

测试写入 `tests/feature/console/commands/send_report_test.rs`，嵌套命令保留原有子目录。按[测试接线](../testing/getting-started.md#creating-tests)声明模块后，用 Cargo 运行。模板沿用 `make:test` 的健康检查，可在生成的测试中加入命令断言。

`--force` 允许替换两个生成文件。命令生成后若测试生成失败，Artisan 以状态码 1 退出并保留命令文件。解决报错后，运行 `./artisan make:test Console/Commands/SendReportTest` 单独生成测试。

## 命令结构 {#command-structure}

命令类型实现 `Command`。用 `CommandDefinition::from_signature` 和 `with_description` 声明。Artisan 使用同一份定义解析输入和显示帮助。`handle` 方法可通过应用容器解析类型化依赖。

```rust
use std::sync::Arc;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container, foundation::Application as _},
    foundation::{Application, configuration::ApplicationBuilder},
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};

struct Reporter;

impl Reporter {
    fn report(&self, user: &str) -> String {
        format!("Report ready for {user}")
    }
}

struct SendReport(CommandDefinition);

impl Command for SendReport {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self, app: &'a Application, input: &'a Input, output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let reporter = app.make::<Reporter>().await.map_err(ConsoleError::from_source)?;
            let Some(user) = input.argument("user") else {
                return input.fail("A user is required.");
            };
            input.io(output).line(&reporter.report(user.as_str().unwrap_or_default()))?;
            Ok(0)
        })
    }
}

struct ReportProvider;

impl ServiceProvider for ReportProvider {
    fn register<'a>(
        &'a mut self, app: &'a Application,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container().instance(Arc::new(Reporter))?;
            let definition = CommandDefinition::from_signature("reports:send {user}")?
                .with_description("Send a report");
            app.container().bind::<SendReport, _>(move |_| {
                Ok(Arc::new(SendReport(definition.clone())))
            })?;
            Ok(())
        })
    }
}

fn configure_commands(builder: ApplicationBuilder) -> ApplicationBuilder {
    builder.with_providers(vec![Box::new(ReportProvider)]).with_command::<SendReport>()
}

async fn report_from_handler(user: &str, output: &mut Output) -> Result<i32, ConsoleError> {
    Artisan::call_type::<SendReport>(&[user.into(), "--no-ansi".into()], output).await
}
```

## 注册命令 {#registering-commands}

构建时发现默认目录和 `COMMANDS` 指定的额外目录。注册其他模块中的类型时，在 Provider 中绑定该类型，然后在应用构建器调用 `with_command::<Type>()`。可多次调用以登记多个类型。`with_commands` 同样支持显式注册函数。

Artisan 在控制台装配时从容器解析类型，之后同一应用复用该命令实例。可按签名名称调用，也可用 `Artisan::call_type::<Type>`。按类型调用会使用已注册实例对应的名称。

## 信号处理 {#signal-handling}

用 `io.trap` 监听一个信号，或用 `io.trap_many` 监听一组信号。收到信号后，最后注册的回调先运行。让回调保持简短，通知异步命令自行完成工作并释放资源。

```rust
use openrust::{
    console::{AsyncCommandCallback, CommandSignal, ConsoleError},
    support::facades::Artisan,
};
use std::sync::Arc;
use tokio::sync::Notify;

fn register_signal_command() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "reports:watch {--once}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut io = input.io(output);
                let stop = Arc::new(Notify::new());
                let signal_stop = Arc::clone(&stop);

                #[cfg(unix)]
                let signals = [CommandSignal::Terminate, CommandSignal::Quit];
                #[cfg(windows)]
                let signals = [CommandSignal::Interrupt, CommandSignal::Break];

                io.trap_many(signals, move |_| signal_stop.notify_one())?;

                if io.option("once") != Some(serde_json::Value::Bool(true)) {
                    stop.notified().await;
                }

                io.line("Report watcher stopped")?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
```

在 Unix 上，对运行中的 `./artisan reports:watch` 进程发送 `kill -TERM <pid>`。Windows 上对应的控制台事件为 Ctrl+C 和 Ctrl+Break。执行 `./artisan reports:watch --once` 可立即退出。

命令返回、失败或取消时会释放回调。调用 `io.untrap()` 可提前释放。嵌套命令与调用者共享信号订阅，结束时仅释放本次注册。进程级处理器由异步运行时管理，命令退出后可能仍处于安装状态。

## 事件 {#events}

### 启动 Artisan {#artisan-starting-event}

应用首次调用控制台时，Artisan 在定型命令注册表前派发 `ArtisanStarting`。同步监听器可调用 `with_artisan` 注册命令。用 `with_events(DiscoverEvents::new(&[register_console_listeners]))` 注册监听器。启动事件返回后，注册表可用于执行命令。

```rust
use std::sync::Arc;
use openrust::{
    console::events::ArtisanStarting,
    contracts::events::{Event, Listener},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse,
    },
};

struct RegisterReporting;

impl Listener for RegisterReporting {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(event) = event.downcast_ref::<ArtisanStarting>() {
                event.with_artisan(|artisan| {
                    artisan.command("reports:about", |output| {
                        output.stdout.push_str("Reporting commands available\n");
                    })?;
                    Ok(())
                })
                .map_err(|source| EventError::Listener { source: Box::new(source) })?;
            }

            Ok(None)
        })
    }
}

fn register_console_listeners(registry: &mut ListenerRegistry) -> EventResult<()> {
    registry.listen(ArtisanStarting::NAME, Arc::new(RegisterReporting), Delivery::Sync)
}
```

### 命令执行事件 {#command-execution-events}

Artisan 在已解析的命令运行前派发 `CommandStarting`，并在命令返回或失败后派发 `CommandFinished`。两种事件均包含命令的正式名称和已解析输入快照。结束事件还包含退出码、耗时和捕获的标准输出。缺少必填参数或输入无效时，快照保留解析器已识别的参数，命令体跳过执行。

CLI 和直接 `Artisan::call` 调用都遵循此生命周期。通过 `CommandIo::call`、`call_named` 或 `call_silently` 调用的命令在外层命令内执行，沿用外层的事件对。列表和帮助由命令处理，`--version` 直接显示框架版本。

注册监听器前，先在应用服务提供者中绑定一个 `CommandLog` 实例。

```rust
use std::sync::{Arc, Mutex};
use openrust::{
    console::events::{CommandFinished, CommandStarting},
    contracts::events::{Event, Listener},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse,
    },
    support::facades::App,
};

#[derive(Default)]
struct CommandLog(Mutex<Vec<(String, Option<i32>)>>);

struct RecordCommands;

impl Listener for RecordCommands {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let entry = event.downcast_ref::<CommandStarting>()
                .map(|event| (event.command.clone(), None))
                .or_else(|| event.downcast_ref::<CommandFinished>()
                    .map(|event| (event.command.clone(), Some(event.exit_code))));

            if let Some(entry) = entry {
                let log = App::make::<CommandLog>().await.map_err(EventError::Unavailable)?;
                log.0.lock().unwrap().push(entry);
            }

            Ok(None)
        })
    }
}

fn listen_to_commands(registry: &mut ListenerRegistry) -> EventResult<()> {
    let listener = Arc::new(RecordCommands);
    registry.listen(CommandStarting::NAME, listener.clone(), Delivery::Sync)?;
    registry.listen(CommandFinished::NAME, listener, Delivery::Sync)
}
```

## 自定义生成模板 {#stub-customization}

把生成器模板发布到应用的 `stubs/` 目录。命令会发布结构体、trait、枚举、命令、工厂、迁移、模型、策略、Seeder、功能测试和单元测试模板。编辑发布后的模板即可修改相应生成器下一次生成的内容。

```sh
./artisan stub:publish
./artisan stub:publish --existing
./artisan stub:publish --force
```

默认保留现有模板并补齐缺失文件。`--existing` 只覆盖已经存在的模板，与 `--force` 同用时也保持这个范围。单独使用 `--force` 会覆盖现有模板并补齐缺失文件。

同步的 `foundation.publishing_stubs` 监听器可通过 `PublishingStubs` 事件添加源文件、插入编译进产物的模板或移除模板。发布目标使用应用模板目录内的文件名。

## 生成 Rust 类型 {#generating-rust-types}

Artisan 可生成 Rust 结构体、trait 和枚举。使用生成的类型前请声明对应模块。

```sh
./artisan make:class Report
./artisan make:class SendReport --invokable
./artisan make:interface ReportStore
./artisan make:trait ReportFormatting
./artisan make:enum ReportState
./artisan make:enum ReportCode --int
./artisan make:enum ReportKind --string
```

`make:interface` 和 `make:trait` 均生成 Rust trait。接口依次查找 `app/contracts`、`app/interfaces`，trait 依次查找 `app/concerns`、`app/traits`，枚举依次查找 `app/enums`、`app/enumerations`。目录均不存在时写入 `app/`。名称以 `App/` 开头时，从 `app/` 开始解析。

可调用结构体提供显式的 `invoke()` 方法。整数和字符串后备枚举通过 `value()` 分别返回 `i64` 和静态字符串。使用 `-f` 或 `--force` 替换已生成的文件。可通过 [`stub:publish`](#stub-customization) 发布可自定义模板。接口命令使用内置模板。

## 命令签名与闭包输入 {#command-signatures}

位置参数支持必填 `{user}`、可选 `{user?}`、默认 `{user=Taylor}`、必填数组 `{user*}` 和可选数组 `{user?*}`。选项支持 `{--force}`、`{--queue=}`、`{--queue=default}`、`{--id=*}`；`{--Q|queue=}` 声明缩写。数组默认值写作 `{user=*Taylor,Dayle}` 或 `{--id=*1,2}`。用 ` : ` 分隔说明，签名可以换行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "mail:send {user : User ID} {--Q|queue=default} {--id=*} {--force}",
        |input: &Input, output: &mut Output| {
            let user = input.argument("user").unwrap();
            let queue = input.option("queue").unwrap();
            output.comment(&format!("{user}: {queue}"));
            Ok(0)
        },
    )?.purpose("Send a marketing email")
}
```

类型命令通过 `CommandDefinition::from_signature` 创建相同的定义。`Input::argument` 和 `option` 返回 JSON 标量／数组，未知或未设置的名称返回 `None`；`arguments` 和 `options` 返回各组输入。未设置的可选标量在整组中为 JSON null，数组保持数组。自定义 Clap 值解析器通过 `matches()` 读取其具体类型；这些 JSON 入口支持字符串、布尔、数值和 UTF-8 OS 字符串。

原有 `Artisan::command(signature, |output| …)` 继续可用，正常退出码为 0。`command_with` 接受显式类型的输入／输出闭包并返回 `Result<i32, ConsoleError>`，也接受异步回调 `fn(&Application, &Input, &mut Output) -> ConsoleFuture<Result<i32, ConsoleError>>`。在回调中用 `app.make::<Service>().await` 解析服务，读取所属应用，在命令执行时解析而不是注册时。明确的回调入口避免破坏既有 Rust 闭包类型推断；不提供 PHP 参数反射或未声明 Rust 类型的自动注入。

首个参数为 `Arc<Service>` 的异步回调会在执行时自动从当前应用解析服务。通过 Provider 或既有容器类型描述注册该服务；构造继续使用同一容器，查看列表不会提前执行构造。

```rust
use std::sync::Arc;
use openrust::{console::{ConsoleError, ConsoleFuture, Input, Output}, support::facades::Artisan};

struct Mailer;
impl Mailer {
    fn send(&self, user: &str) -> String { format!("Sent to {user}") }
}
fn send<'a>(mailer: Arc<Mailer>, input: &'a Input, output: &'a mut Output)
    -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
    Box::pin(async move {
        output.comment(&mailer.send(input.argument("user").unwrap().as_str().unwrap()));
        Ok(0)
    })
}
fn register_injected() -> Result<(), ConsoleError> {
    Artisan::command_with("mail:send-injected {user}", send)?.purpose("Send with an injected service")
}
```

## 命令输入与输出 {#command-io}

用 `input.io(output)` 同时访问命令解析后的值和原始输出。参见[类型化输入](#typed-command-input)、[交互问答](#interactive-questions)、[输出消息](#writing-output)、[表格](#tables)和[进度条](#progress-bars)。

## 类型化命令输入 {#typed-command-input}

`Input::input()` 返回 `CommandInput` 快照，`input_value(key, default)` 读取单个值。支持 `string`／`str`、`integer`、`float`、`boolean` 及其 `_or`／`_strict`、`clamp`、`date`、`interval`、backed `r#enum`／`enums`、`array` 和 `collect`，复用请求输入的转换规则。未指定日期时区时使用应用配置；无效日期返回错误，`ConsoleError::source` 保留原始错误来源。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};

fn register_typed() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "reports:send {--from=} {--limit=10} {--active}",
        |input: &Input, output: &mut Output| {
            let data = input.input();
            let date = data.date("from", Some("!Y-m-d"), Some("UTC"))?;
            let limit = data.integer_strict("limit", 10);
            output.comment(&format!("limit={limit}, active={}, date={}", data.boolean("active"), date.is_some()));
            Ok(0)
        },
    )?.purpose("Send a typed report")
}
```

`arguments()` 和 `options()` 保留各组。直接构造的 `CommandInput` 若有同名键，固定实现先合并选项再合并位置参数，位置参数优先。`all`／`to_array` 返回合并值；`all_keys` 将缺失的选中键保留为 null，空选择返回整组，而 `only` 忽略缺失路径。`get`、`has`／`exists`、`has_all`、`has_any`、`missing`、`filled`、`is_not_filled`、`any_filled` 和 `except` 使用既有点路径规则。false、零和空数组属于已填写，空字符串与 null 不属于。

条件入口 `when_has`、`when_filled`、`when_missing` 和 `when_enum` 返回 `Option<R>`（枚举入口还返回 `Result`）。`None` 表示未执行回调，用 `or_else` 提供回退；Rust 使用明确返回类型，不隐式混合 PHP 按真值决定的输入对象与回调返回值。动态属性使用 `get`／`exists`，这些适配不会执行任意 PHP 表达式。

## 隔离命令 {#isolatable-commands}

命令实现 `contracts::console::Isolatable`，通过 `Artisan::add_isolatable`、`console::Application::add_isolatable_command` 或 Console Kernel 契约的 `register_isolatable_command` 注册，自动提供 `--isolated[=code]`。Rust 的 `dyn Command` 擦除后不能自动发现另一 Trait，因而明确选择带 Trait 信息的注册入口。原有 `add` 和 `CommandDefinition::isolatable()` 保留；后者使用命令名及一小时锁。

```rust
use openrust::{
    cache::Ttl,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::console::Isolatable,
    foundation::Application,
    support::facades::Artisan,
};

struct SendReports(CommandDefinition);
impl Default for SendReports {
    fn default() -> Self {
        Self(CommandDefinition::from_signature("reports:isolated {user}").expect("valid signature"))
    }
}
impl Command for SendReports {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _app: &'a Application, input: &'a Input, output: &'a mut Output)
        -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.comment(&format!("Report for {}", input.argument("user").unwrap()));
            Ok(0)
        })
    }
}
impl Isolatable for SendReports {
    fn isolatable_id(&self, input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(input.argument("user").and_then(|value| value.as_str().map(str::to_owned)))
    }
    fn isolation_lock_expires_at(&self, _input: &Input) -> Result<Ttl, ConsoleError> {
        Ok(Ttl::Seconds(300))
    }
}
fn register_isolated() -> Result<(), ConsoleError> {
    Artisan::add_isolatable(Box::<SendReports>::default())
}
```

隔离按需启用，普通调用不获取锁。指定 `--isolated` 时，冲突跳过处理器并返回 0；`--isolated=12` 可指定冲突退出码。Trait 默认方法同样使用命令名和一小时期限。覆盖 `isolatable_id` 追加与输入有关的后缀，覆盖 `isolation_lock_expires_at` 返回 `Ttl::Seconds` 或 `Ttl::Until(SystemTime)`；空 ID 与未提供 ID 不同。

参与进程必须使用同一支持锁的缓存及命名空间。array 只在应用进程内协调，file 需要共享文件系统，database／Redis 使用其配置后端。正常完成、错误和运行时内取消会释放所属锁；进程被终止时依靠期限恢复，过期的旧拥有者不会删除新拥有者的锁。数据库整秒锁采用 Unix 秒加租期，亚秒期限保留既定原生取整策略。

## 全局选项与经过验证的输入修改 {#editing-command-input}

分派后的 `Input` 助手包含调用时的 `command` 参数（保留别名）及继承的全局选项：`verbose` 为布尔值，`env` 为可空字符串，`ansi` 为 null／true／false。`no-ansi` 合并到 `ansi`，不单列选项。`matches()` 保留原生 Clap 开关、环境值及详细程度计数，既有 CLI 解析和输出优先级不变。单独调用 `CommandDefinition::parse` 仍按其声明范围解析。`Input::clone` 保留注册定义身份及关停令牌，修改后的副本仍可交给 Console Kernel。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use serde_json::json;

fn register_edited() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:edit {user} {--queue=default}", |input: &Input, output: &mut Output| {
        let mut edited = input.clone();
        edited.set_argument("user", ["42"])?;
        edited.set_option("queue", json!("priority"))?;
        let data = edited.input();
        output.comment(&format!("user={}, queue={}", data.string("user"), data.string("queue")));
        Ok(0)
    })?.purpose("Edit parsed command input")
}
```

`set_argument` 接收 CLI 参数 token；`set_option` 接收 JSON 字符串、布尔开关、计数、数组，以及表示可选值未带参数的 null。每次修改复用原 Clap 解析器及约束，同时更新 `matches()` 与类型化助手；验证失败不改原输入，保留其他值、空字符串及非 UTF-8 OS 参数。

这些入口按 CLI 定义重新验证，不是任意 PHP 对象修改。定义无法表达的值、可选位置的空洞、清空非空默认数组都会返回错误，不悄悄重绑定其他字段或恢复默认值。读取或修改全局输入不会重新配置已经创建的输出流。

## 缺失必填参数的自动提示 {#prompting-for-missing-input}

实现 `PromptsForMissingInput`，注册前用 `.prompting()` 保留该 Trait。必填参数按声明顺序收集，随后执行最终验证与隔离。已传参数不重复询问；`--no-interaction`、`--quiet` 和 `--silent` 跳过询问，但仍拒绝缺失的必填参数。

```rust
use openrust::{
    console::{Command, CommandDefinition, CommandRegistration, ConsoleError,
        ConsoleFuture, Input, MissingInputPrompt, Output},
    contracts::console::PromptsForMissingInput,
    foundation::Application,
    support::facades::Artisan,
};
use std::collections::BTreeMap;

struct Deliver(CommandDefinition);
impl Command for Deliver {
    fn definition(&self) -> &CommandDefinition { &self.0 }
    fn handle<'a>(&'a self, _: &'a Application, input: &'a Input,
        output: &'a mut Output) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.stdout.push_str(&format!("user={}, queue={}",
                input.argument("user").unwrap().as_str().unwrap(),
                input.option("queue").unwrap()));
            Ok(0)
        })
    }
}
impl PromptsForMissingInput for Deliver {
    fn prompt_for_missing_arguments_using(&self) -> BTreeMap<String, MissingInputPrompt> {
        BTreeMap::from([("user".into(),
            MissingInputPrompt::text("Which user should receive the report?", "e.g. 123"))])
    }
}
fn register_prompted() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(CommandRegistration::new(Deliver(
        CommandDefinition::from_signature("reports:prompt {user} {--queue}")?
    )).prompting()))
}
```

默认问题使用参数说明或名称。`MissingInputPrompt::text` 提供问题和占位说明；`MissingInputPrompt::custom` 接收异步的应用／可变输入／输出回调，返回 CLI token（`Vec<OsString>`），数组参数可以返回多个值。自定义回调可以返回 `"0"`；默认文本问题拒绝空答案或 `"0"`，保持必填文本验证语义。每个缺失参数都会重新取得问题定义。

`after_prompting_for_missing_arguments` 只在至少收集一个缺失参数后执行，接收可变 `Input`，可调用 `set_option` 或 `set_argument`；命令执行前再做最终验证。类型同时实现两种 Trait 时，可在同一个 `CommandRegistration` 上组合 `.prompting().isolatable()`。适配器在类型擦除前保留 Trait，复用现有注册、缓存锁与生命周期管线。

默认文本问题要求连接终端；HTTP／程序化捕获输出不会读取进程标准输入。这些环境请显式传参或提供自定义回调。帮助信息不触发交互。取消与回调失败沿现有错误处理传播；等待输入时不持有隔离锁。

## 命令注册的生命周期 {#command-lifecycle}

Provider 就绪后，每个应用装配一次命令。后续 Facade 调用、HTTP 请求和受管 Runner 复用已注册的命令实例及闭包捕获。不同应用实例、不同 CLI 进程使用各自的注册表。下面的闭包在同一应用中连续调用两次，依次返回 `1`、`2`。

```rust
use openrust::{console::{ConsoleError, Input, Output}, support::facades::Artisan};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_counter() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:count", move |_: &Input, output: &mut Output| {
        output.stdout.push_str(&(calls.fetch_add(1, Ordering::SeqCst) + 1).to_string());
        Ok(0)
    })?.purpose("Count calls within this application")
}
```

每次调用独立持有输入、输出、详细程度和生命周期作用域。命令执行失败不会重建注册表；注册失败不会公开半成品，后续可以重试。执行命令时不持有注册表锁，因此支持嵌套调用。

并发请求可能执行同一个命令对象。共享字段须保证线程安全；请求输入、用户和请求级依赖在 `handle` 或当前 `command_with` 回调中解析，不在命令注册时捕获请求数据。

<a id="writing-output"></a>

## 写入命令输出 {#writing-command-output}

通过 `input.io(output)` 借用当前命令上下文。`line`、`info`、`comment`、`question`、`warn`、`error`、`alert` 和 `new_line(count)` 使用同一输出。`info` 输出绿色消息；`output_components()` 保留现有信息／警告／错误组件布局。输出失败可通过 `ConsoleError::source` 读取原始原因。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

fn register_output() -> Result<(), ConsoleError> {
    let calls = Arc::new(AtomicUsize::new(0));
    Artisan::command_with("reports:output", AsyncCommandCallback::new(move |_, input, output| {
        let calls = calls.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut io = input.io(output);
            io.info("Report ready")?;
            io.line("<options=bold>Summary</>")?;
            io.comment("Report generated")?;
            io.question("Review the report")?;
            io.warn("One record was skipped")?;
            io.error("A record needs correction")?;
            io.new_line(1)?;
            io.alert("Check complete")?;
            io.output_components().info("Saved")?;
            io.line(&format!("Calls: {}", calls.fetch_add(1, Ordering::SeqCst) + 1))?;
            Ok(0)
        })
    }))?.purpose("Display a report")
}
```

写入前解析命名样式、前景／背景色、样式选项及超链接。未知标签保持字面量，转义尖括号正常显示。未闭合的样式状态属于该输出对象，不传入另一输出；标签嵌套、颜色或选项非法时返回 `ConsoleFormatError`。`--ansi`／`--no-ansi` 控制颜色，`--quiet`／`--silent` 抑制普通消息。

普通 CLI 入口实时发送这些消息，并先发送尚未写出的组件；不会在 `stdout` 中保留无界终端历史。程序化 `Output::default()` 将文本捕获到 `stdout`，可用于 HTTP 响应或断言。原生命令事件的缓冲输出字段记录待写／捕获文本，不是原始终端输出的日志归档。

`AsyncCommandCallback::new` 为捕获状态的异步闭包明确 boxed-future 返回类型，仍将同一个回调交给既有 `command_with`，不增加第二个执行器或额外回调层。具名异步函数继续可用。`has_argument`／`has_option` 检查输入定义，已声明但未赋值的输入仍存在；`input()` 提供类型化输入，`input_value` 支持缺失时的回退值。

需要控制单条消息的详细程度或直接写入样式输出时，使用下面的接口。交互问答、表格和进度条不属于这些消息方法。

## 输出样式与详细程度 {#output-style-verbosity}

`get_output()` 返回真实的 `console::OutputStyle`，它的 `get_output()` 借用底层 `Output`。`write`、`writeln` 和 `new_line` 共用原格式器和输出流。消息可传字符串或字符串切片；`writeln` 给每条消息追加换行。`OutputFormat::Raw` 保留标签，`Plain` 先格式化再移除剩余标记。需要组合格式与详细程度时，传入 `OutputOptions`。 `write_iter`／`writeln_iter` 接收字符串迭代器，包括拥有的字符串和单次遍历迭代器，保留逐条写入及计数规则。

默认消息阈值为 `Normal`。使用 `_with` 方法指定阈值，原单参数方法保持默认行为；`line_with` 还接受可选样式。`Verbose`、`VeryVerbose`、`Debug` 分别需要 `-v`、`-vv`、`-vvv`；`Quiet` 在 `--quiet` 下仍显示，`--silent` 隐藏全部消息。`OutputVerbosity::from_name` 映射 `v`、`vv`、`vvv`、`quiet`、`normal`，未知名称按 `Normal` 处理。

`new_lines_written` 初始为一，`new_line_written` 初始为 false，记录经该样式对象写入的换行；直接编辑缓冲或原始写入不会更新它们。隐藏的 `write` 与 `writeln` 计数规则不同，应读取计数，不能只从捕获文本推断。`is_quiet`、`is_verbose`、`is_very_verbose`、`is_debug` 查询当前输出状态。`set_input` 替换命令读取的输入，`set_output` 接收借用替代流的真实 `OutputStyle`；Rust 借用保证替代输入和输出仍然存活。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputVerbosity},
    support::facades::Artisan,
};

pub fn register_styled() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styled", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        io.info_with("Details", OutputVerbosity::Verbose)?;
        io.warn_with("Essential", OutputVerbosity::Quiet)?;
        io.line_with("Summary", Some("info"), None)?;
        let style = io.get_output();
        style.write(&["<raw>", "text"], false, OutputFormat::Raw)?;
        style.new_line(1)?;
        Ok(0)
    })?;
    Ok(())
}
```

## 交互问答 {#interactive-questions}

通过异步命令 I/O 读取文本、确认、隐藏文本和补全。`confirm` 默认 false；`OutputStyle::confirm` 默认 true。

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_questions() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.ask_with("Name", Some("Guest")).await?;
            let proceed = io.confirm("Continue").await?;
            let token = io.secret("Token").await?;
            let color = io.anticipate("Color", CompletionCallback::values(vec!["blue".into(), "green".into()]), Some("blue")).await?;
            io.line(&serde_json::json!({"name": name, "proceed": proceed, "token_present": token.is_some(), "color": color}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report options")
}
```

`--no-interaction`、`--quiet` 和 `--silent` 关闭问答及缺失必填参数提示；缺少必填参数仍失败。HTTP 捕获调用不会读取进程标准输入。输入源耗尽时返回默认值，没有默认值则返回具体错误，并关闭该输入后续交互。

需要自定义校验时，创建 `Question`，通过 `set_normalizer`、`set_validator` 设置 `QuestionCallback`，再调用 `get_output().ask_question(&question).await`。校验器可返回结构化 JSON；`ask_with_validator` 和 `ask_hidden_with` 保留该返回值。非交互默认值只执行校验器，不执行规范化器。`set_max_attempts`、`set_trimmable`、`set_multiline` 和 `set_timeout` 控制重试和读取。

`Input::set_stream` 接收包装异步缓冲读取器的 `ConsoleInputStream`。输入克隆共享流位置，但独立保存交互开关；向 Kernel 显式传入 Input 时共享本次调用状态。补全回调在当前应用作用域运行，支持 await。`CompletionCallback::iter` 惰性缓存单次迭代器，`pairs` 按顺序提供字符串键及值。Ctrl+C 返回 130；取消、超时和校验失败均恢复终端模式。Windows 终端实测仍归发布矩阵。

## 选择题与多选 {#choice-questions}

`choice` 接收选项列表，不设置默认值。使用 `choice_with` 指定默认值、最大尝试次数及多选开关。多选输入用逗号分隔索引或选项值，结果为数组。`choice_pairs_with` 保留声明顺序并返回关联键；规范的整数字符串按数字键处理，其他字符串保持原写法。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_choices() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:choices", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let channel = io.choice_with("Channel", ["email", "sms"], Some(serde_json::json!(0)), Some(2), false).await?;
            let formats = io.choice_with("Formats", ["csv", "json"], Some(serde_json::json!("0,1")), Some(2), true).await?;
            let priority = io.choice_pairs_with("Priority", [("normal", "Normal"), ("urgent", "Urgent")], Some(serde_json::json!("normal")), Some(2), false).await?;
            io.line(&serde_json::json!({"channel": channel, "formats": formats, "priority": priority}).to_string())?;
            Ok(0)
        })
    }))?.purpose("Collect report choices")
}
```

文本、数字和布尔选项复用既有标量比较规则。重复且歧义的标签需要改用键选择。无默认值的问题在非交互模式返回 null；非法默认值仍校验失败。`OutputStyle::choice_with` 额外将默认选项值解析为键；命令 I/O 直接使用传入的默认值。

自定义提示或错误文案时，创建 `ChoiceQuestion::new` 或 `from_pairs`，使用 `set_prompt`、`set_error_message`（只有一个回答参数，支持 `%s`、可重复的 `%1$s`、宽度／精度及字面量 `%%`）、`set_multiselect`，再将 `as_question()` 传给 `ask_question`。`get_choices`、`get_prompt` 和 `is_multiselect` 读取实际定义。修改去空白开关后，已取出的默认校验器也跟随更新。原生 Question 克隆独立保存设置；显式回调及补全缓存句柄仍共享。样式标签应围绕完整字素；格式化可见文本按 NFC 和字素边界处理，Raw 输出、超链接目标及返回键保持原数据。


回调中的运行错误使用 `ConsoleError::from_runtime_source`：立即终止重试，同时保留原错误类型与来源链。普通校验错误仍可重试；输入缺失走已校验的默认值并关闭后续交互。终端在运行规范化器或校验器之前恢复。模板生成非 UTF-8 字节时返回带原始来源的 `ConsoleQuestionError::InvalidEncoding`，不将任意 PHP 二进制串当作 Rust 文本返回。

应用标签类型实现 `Display` 后，可用 `ChoiceLabel(value)` 传入选项。原生 JSON 回答模型保存标签文本快照，不携带 PHP 对象身份；关联选择仍返回原声明键，应用对象保留在调用方集合中。

## 表格 {#tables}

使用 `input.io(output).table(headers, rows)` 渲染表格。表头和数据行接受迭代器；JSON 标量单元格复用公共标量转换，不额外添加 JSON 引号。模型数据先按需要的顺序选择列，再将值传入表格。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table", |input: &Input, output: &mut Output| {
        let users = serde_json::json!([
            {"name": "Ada", "email": "ada@example.test"},
            {"name": "Li", "email": "li@example.test"}
        ]);
        let rows = users.as_array().unwrap().iter().map(|user| {
            [user["name"].clone(), user["email"].clone()]
        });
        input.io(output).table(["Name", "Email"], rows)?;
        Ok(0)
    })?;
    Ok(())
}
```

`table_with` 选择 `TableStyle`；`table_with_columns` 另外接受 `(列索引, 样式)`。`TableStyle::from_name` 提供 `default`、`markdown`、`borderless`、`compact`、`symfony-style-guide`、`box` 和 `box-double`。可通过边框、填充和格式设置方法定制样式。`TablePadding::{Left, Right, Both}` 表示填充所在侧，默认 `Right`。

直接渲染时使用 `Table::new(io.get_output())?`，设置表头、数据行和列样式后调用 `render()`。`TableCell::display(value)` 显式适配实现 `Display` 的业务类型；JSON 值和常用标量直接转换。多行单元格、Unicode 可见宽度、格式标签、ANSI 输出及 quiet／silent 过滤沿用原输出管线。此矩形表格接口不代表已支持整个继承辅助库。

## 进度条 {#progress-bars}

可计数的集合使用 `with_progress_bar_iter`。回调接收元素、当前进度条及原整数／字符串键；回调成功后才前进一步，最后返回原集合。`Vec`、数组、有序映射、基础集合、模型集合及其借用均实现 `ProgressIterable`，无需克隆模型。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, ProgressBar, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress() -> Result<(), ConsoleError> {
    ProgressBar::set_format_definition(
        "reports",
        "%task% %current%/%max% [%bar%] %percent:3s%%",
    )?;
    Artisan::command_with("reports:progress", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut processed = Vec::new();
        let items = io.with_progress_bar_iter(vec!["csv", "json", "html"], |item, _, _key| {
            processed.push(item.to_uppercase());
            Ok(())
        })?;
        io.new_line(1)?;

        let count = i64::try_from(items.len()).map_err(ConsoleError::from_source)?;
        let mut bar = io.get_output().create_progress_bar(count)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            Ok(bar.get_message().unwrap_or("work").to_owned())
        });
        bar.set_format("reports")
            .set_message("build")
            .set_placeholder_formatter("task", label);
        bar.start()?;
        let mut total = 0;
        for item in &items {
            total += item.len();
            bar.advance()?;
        }
        bar.finish()?;
        drop(bar);
        io.new_line(1)?;
        io.info(&format!("{}:{total}", processed.join(",")))?;
        Ok(0)
    })?;
    Ok(())
}
```

数字总步数使用 `with_progress_bar(steps, callback)`，由回调调用 `advance` 或 `advance_by`。手动控制时通过 `create_progress_bar(total)` 创建，再调用 `start`、`advance` 和 `finish`。这些回调同步执行；错误向上传播，不会悄悄补上完成状态。

工厂沿用当前输出：捕获调用把进度保留在缓冲中；真实控制台将进度流式写入 stderr，普通 stdout 保持独立。quiet／silent 关闭显示。默认刷新间隔为40毫秒，完成步仍会检查显示。空集合、未知总数及重复 `finish` 保留各自生命周期。`progress_start`、`progress_advance` 和 `progress_finish` 提供样式内保存的进度状态；`progress_finish` 另外写入两个换行并释放该状态。

可设置格式、消息、进度字符、宽度、刷新频率及占位符。共享命名格式和占位符定义属于当前应用，在命令注册或执行作用域内设置；进度条保留创建时所属应用的定义。独立进度条可直接使用字面格式及实例占位符。占位符回调借用实际进度条，返回 `Result<String, ConsoleError>`。内存占位符显示原生进程驻留内存，不是语言虚拟机堆大小。

数字与集合帮助方法拆开命名，使回调及返回类型明确。传入 `&items` 可在失败时保留调用方所有权。本节验收同步集合及手动进度；惰性迭代器适配、异步／可变任务回调和其余继承输出契约继续单独核对。

## 惰性进度迭代 {#lazy-progress-iteration}

使用 `OutputStyle::progress_iterate(items)` 逐项处理任务。首次 `next()` 才启动，取下一项时才推进，自然耗尽后才完成。每项返回 `Result`，用 `?` 传播输出错误。提前 `break`、处理失败或丢弃迭代器，不补写完成帧及样式末尾的两个换行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output},
    support::facades::Artisan,
};

fn register_progress_iteration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:iterate", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut sum = 0;
        for number in io.get_output().progress_iterate([2, 4, 6]) {
            sum += number?;
        }
        io.info(&format!("sum:{sum}"))?;
        Ok(0)
    })?;
    Ok(())
}
```

手动进度条提供 `iterate(items)` 和 `iterate_with_max(items, max)`，可在项之间通过 `iterator.progress_bar()` 访问同一条进度。样式入口另有 `progress_iterate_with_max`。`None` 与 `Some(0)` 从迭代器精确长度提示推导总数；无精确提示时保持未知，或保留手动进度条已有总数。显式正总数覆盖推导。元素、元组键及借用模型身份保持不变，转成底层迭代器的动作也推迟到首次取值。精确空迭代器直接完成，不绘制启动帧。异步和可变回调帮助方法见下节；这些入口不会阻塞执行异步任务或把任务放到后台。

## 异步与原地处理回调 {#async-progress-callbacks}

数字总数使用 `with_progress_bar_async`，只读集合使用 `with_progress_bar_iter_async`，显式原地修改使用 `with_progress_bar_iter_mut_async`。它们复用同一进度条和命令执行管线，等待每个装箱 Future 完成后才推进。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError},
    support::facades::Artisan,
};

fn register_progress_async() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:async", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            io.with_progress_bar_async(1, |bar| Box::pin(async move {
                tokio::task::yield_now().await;
                bar.advance()?;
                Ok(())
            })).await?;
            io.new_line(1)?;

            let mut values = vec![1, 2, 3];
            io.with_progress_bar_iter_mut_async(&mut values, |value, _, _key| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    *value *= 2;
                    Ok(())
                })
            }).await?;
            io.new_line(1)?;
            io.info(&format!("values:{}", values.iter().sum::<i32>()))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ProgressIterableMut` 支持切片、数组、向量、有序映射、基础集合、模型集合及可变借用，保留键与模型身份。可变模型回调可等待 `model.save()`，并用 `ConsoleError::from_source` 传播具体错误。同步原地处理另有 `with_progress_bar_iter_mut`。原地修改属于显式原生行为，默认回调保持只读；帮助方法不隐式开启事务或回滚已经完成的写入。错误和取消均不补完成帧；传入可变借用保留调用方所有权及已经发生的修改。

## 占位符输出访问 {#progress-placeholder-output}

在 `ProgressFormatter` 中通过 `bar.get_output()` 借用实际 `ProgressOutput` 输出流。它提供 `write`、`writeln`、单次消费的迭代器变体、详细程度查询及 `is_decorated`；`OutputOptions` 控制格式和消息级别。真实 CLI 写入 stderr，捕获调用继续使用原缓冲。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, ProgressFormatter},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_progress_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:progress-output", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut bar = io.get_output().create_progress_bar(2)?;
        let label: ProgressFormatter = Arc::new(|bar| {
            let current = bar.get_progress();
            bar.get_output().writeln(format!("step:{current}").as_str(), OutputFormat::Raw)?;
            Ok(format!("stage:{current}"))
        });
        bar.set_format("%stage%")
            .set_placeholder_formatter("stage", label);
        bar.start()?;
        bar.advance_by(2)?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

回调接收进度条，输出仅在使用期间借出，避免同时取得进度条及其内部输出的重叠可变引用。先读取所需进度状态，再借用输出，借用结束后可继续操作进度条。写入复用既有格式器与错误链，不更新外层样式换行计数。占位符在排版时可能重复求值，业务副作用应放在回调之外。本视图不代表完整继承输出 SDK；格式器替换和输出配置见下文对应章节。

## 输出配置 {#output-configuration}

`Output` 与 `OutputStyle` 提供 `set_verbosity`、`set_decorated`；样式另有 `get_verbosity`、`is_decorated` 和 `is_silent`，操作实际后端。后续改成 Quiet 或 Silent 不会删除此前已经接受的消息。运行时设置器可以显式覆盖 CLI 级别；示例先尊重调用方最初的 Quiet／Silent。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputVerbosity},
    support::facades::Artisan,
};

fn register_output_configuration() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:output-config", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let style = io.get_output();
        let previous = style.get_verbosity();
        if previous <= OutputVerbosity::Quiet {
            return Ok(0);
        }
        style.set_verbosity(OutputVerbosity::Verbose);
        style.write("details;", false, OutputVerbosity::Verbose)?;
        style.set_verbosity(OutputVerbosity::Quiet);
        style.write("quiet;", false, OutputVerbosity::Quiet)?;
        style.set_verbosity(previous);
        let decorated = style.is_decorated();
        style.set_decorated(false);
        style.write("<info>plain</info>", false, OutputOptions::default())?;
        style.set_decorated(decorated);
        Ok(0)
    })?;
    Ok(())
}
```

进度输出视图也有 `set_verbosity` 和 `set_decorated`。真实控制台仅配置 stderr；主样式的设置器同时更新两条流。捕获调用的单一输出共享设置。进度排版读取所选流，嵌套命令的显式 CLI 标志临时作用于两条流；既有命令作用域在结束或取消时恢复父级配置。原生直接修改字符串缓冲仍受命令过滤约束，按级别写入应使用输出方法。显式替换后的共享规则见“输出格式器”。

## 样式与样式栈 {#formatter-styles}

低层 `OutputFormatterStyle` 提供前景色、背景色、选项及超链接配置，并实现公开的 `OutputFormatterStyleInterface`；`FormatterStyle` 是共享 Trait 对象句柄。`apply` 生成 ANSI 文本，不读取输出装饰开关。只有明确要写入预渲染控制序列时才使用 Raw。

```rust
use openrust::{
    console::{ConsoleError, FormatterStyle, Input, Output, OutputFormat,
        OutputFormatterStyle, OutputFormatterStyleStack},
    contracts::service::ResetInterface,
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:styles", |input: &Input, output: &mut Output| {
        let style = Arc::new(OutputFormatterStyle::new(Some("green"), None, &["bold"])?);
        let shared: FormatterStyle = style.clone();
        let mut stack = OutputFormatterStyleStack::default();
        stack.push(shared.clone());
        shared.set_foreground(Some("blue"))?;
        let rendered = stack.get_current().apply("ready")?;
        let mut io = input.io(output);
        io.get_output().write(rendered.as_str(), false, OutputFormat::Raw)?;
        let reset: &mut dyn ResetInterface = &mut stack;
        reset.reset();
        io.new_line(1)?;
        io.info(&format!("stack:{}", stack.get_current().apply("plain")?))?;
        Ok(0)
    })?;
    Ok(())
}
```

克隆句柄保留同一个对象，克隆具体样式得到独立配置。`OutputFormatterStyleStack` 保留样式引用，支持普通弹出和按样式匹配弹出，克隆栈仍共享原样式。`reset` 清空条目并保留配置的空样式，也可通过 `contracts::service::ResetInterface` 调用。匹配比较实际空文本样式输出，包含超链接行为。

设置器返回类型化错误；显示可继续使用此前已编译样式，后续修改会重新校验最新字段。用 `?` 传播失败，`ConsoleError` 保留具体来源。泛型迭代器及字符串转换在默认样式加锁前执行。这些类型复用既有格式化管线；命名样式注册和完整格式器替换见“输出格式器”。

## 输出格式器 {#output-formatters}

`OutputFormatter` 提供命名样式注册和查询、转义、可选文本格式化及按字素换行。宽度为零关闭换行。未关闭的样式状态会保留到下次调用；`get_style_stack()` 返回同一个实际栈的句柄。克隆具体格式器深复制命名样式，栈内样式仍保留引用。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputFormat, OutputFormatter,
        OutputFormatterStyle},
    support::facades::Artisan,
};
use std::sync::Arc;

fn register_formatter() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:format", |input: &Input, output: &mut Output| {
        let formatter = Arc::new(OutputFormatter::new(false));
        formatter.set_style("report", Arc::new(OutputFormatterStyle::new(
            Some("green"), None, &["bold"],
        )?));
        let wrapped = formatter.format_and_wrap(Some("one two three"), 7)?;
        let mut io = input.io(output);
        let style = io.get_output();
        style.set_formatter(formatter);
        style.write("<report>ready</report>", true, OutputFormat::Normal)?;
        style.write(wrapped.as_str(), true, OutputFormat::Raw)?;
        style.get_formatter().set_decorated(true);
        style.write("<report>done</report>", true, OutputFormat::Normal)?;
        Ok(0)
    })?;
    Ok(())
}
```

`OutputFormatterInterface` 支持自定义实现和可选输出；返回 `None` 不写文本。实现 `WrappableOutputFormatterInterface` 并由 `as_wrappable` 返回，便可通过类型擦除句柄访问换行能力；这个原生能力方法避免不安全的 Trait 对象转换。用 `?` 传播错误，保留具体来源。

`Output`、`OutputStyle` 和 `ProgressOutput` 均提供 `get_formatter`、`set_formatter`。克隆返回的 Arc 共享同一个对象。真实控制台显式替换主格式器时，stderr 也引用该对象；随后通过任一句柄修改装饰会影响两条流。未替换时，默认格式器彼此独立。只替换进度输出则仅影响 stderr；捕获命令共享单一流。Raw 绕过格式器，进度光标控制序列也走 Raw。

嵌套 CLI 标志在原输出对象仍安装时恢复装饰；显式替换则保留新格式器配置，不用旧对象标志覆盖。自定义样式回调可安全注册样式、复位同一栈或进行有限的嵌套格式化。格式器调用是同步的，排序锁期间不执行异步工作。

## 颜色与终端能力 {#colors-terminal}

`Color::new(foreground, background, options)` 编译 ANSI 序列；`set`、`unset` 返回起止序列，`apply` 与文本组合。`Color::default()` 为空样式。这些低层方法不读取输出装饰标志；用 Raw 写入预渲染文本前应尊重实际输出开关。无效颜色和选项返回类型化错误。

```rust
use openrust::{
    console::{Color, ConsoleError, Input, Output, OutputFormat, Terminal},
    support::facades::Artisan,
};

fn register_colors() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:colors", |input: &Input, output: &mut Output| {
        let color = Color::new("green", "", &["bold"])?;
        let terminal = Terminal::new();
        let mut io = input.io(output);
        let style = io.get_output();
        let text = if style.is_decorated() {
            color.apply("ready")
        } else {
            "ready".to_owned()
        };
        style.write(text.as_str(), true, OutputFormat::Raw)?;
        style.write(
            format!("terminal:{}x{}", terminal.get_width(), terminal.get_height()).as_str(),
            true, OutputFormat::Raw,
        )?;
        Ok(0)
    })?;
    Ok(())
}
```

`AnsiColorMode::{Ansi4, Ansi8, Ansi24}` 提供 `convert_from_hex_to_ansi_color_code`。`Terminal::get_color_mode()` 优先识别 COLORTERM，再识别 TERM，并缓存首次结果；默认 `Terminal::DEFAULT_COLOR_MODE` 为 Ansi4。`Terminal::set_color_mode(Some(mode))` 强制进程级模式，传 `None` 重置识别。应在启动时配置，不要按请求切换。已有 Color 对象保留已编译序列，后续创建对象或重建样式时使用当前模式。

`Terminal::get_width`、`get_height` 每次读取 COLUMNS、LINES，包括零、负数和数值前缀。没有覆盖值时缓存原生设备尺寸，缺失则分别回退到 80 列、50 行。Unix 直接读取标准输入终端设备，即使 stdout、stderr 被重定向也可获取；Windows 优先使用配置的 ANSICON，否则读取原生控制台。`has_stty_available()` 单独缓存 stty 查询结果，普通尺寸读取不启动外部命令。Windows 实际执行仍归平台发布验收。

## 表格列宽与标题 {#table-layout-controls}

用 `set_column_width` 设置单列最小宽度，`set_column_widths` 设置连续列列表；批量设置会替换此前的最小宽度。稀疏列可用 `set_column_widths_with_keys` 传入索引对。`set_column_max_width` 会立即检查当前格式器的换行能力，不支持时返回错误；内置格式器已支持。最大宽度优先于最小宽度。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:layout", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Count"])
            .set_rows([["longer name", "42"]])
            .set_column_widths([12, 8])
            .set_header_title(Some("Report"))
            .set_footer_title(Some("done"));
        table.set_column_max_width(0, 8)?;
        table.get_style_mut().set_header_title_format("[%s]");
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

表头／表尾标题使用 `set_header_title`、`set_footer_title`，传 `None` 清除。表头将空文本和 "0" 视为无标题，显式空表尾仍使用标题格式。`get_style_mut`、`get_column_style_mut` 提供可变访问，同时保留原来的只读 getter。未配置的列回退到表格本身的实际样式。

宽度保留有符号值，非正数按渲染规则处理，不静默替换为默认值。最大宽度为零不换行；不兼容的有效宽度可能在此前已有输出后才于渲染期失败，应传播 `render` 错误。换行复用当前 `WrappableOutputFormatterInterface`，也支持自定义实现。标题按可见宽度和格式居中、截断。命名样式、复杂行和合并单元格见后续对应章节。

表格仅在换行结果含 LF 时采用它来拆行；否则保留原单元格，原始换行也保持原样，不展开成表格行。自定义换行接口不是通用单元格内容转换钩子。

## 表格行与布局方向 {#table-rows-orientation}

`add_row`、`add_rows` 返回 `Result`，用 `?` 传播数字索引耗尽错误，不触发溢出 panic；此前成功追加的行保留。`set_row` 按整数或字符串键替换行，保留插入顺序；新键追加。规范数字字符串复用整数键，负数键影响后续数字索引，`set_rows` 同时重置行和索引。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table},
    support::facades::Artisan,
};

fn register_rows() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:rows", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["Name", "Value"])
            .set_rows([["alpha", "1"]]);
        table.add_rows([["beta", "2"]])?;
        table.set_row(0_i64, ["changed", "1"])
            .set_vertical(true);
        table.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_headers_rows` 接收多行表头，原 `set_headers` 保留单行形式。`set_horizontal(true)` 将第一行表头转成各行首部，`set_vertical(true)` 用同一行表头生成字段标签并在记录间绘制分隔。两个设置器传 false 都恢复默认布局，不恢复此前方向。每次渲染从保存的行派生布局，并保留列宽和标题设置。命名样式注册、显式分隔符对象、合并单元格和控制台分区另行跟踪。

## 命名表格样式 {#table-style-definitions}

在当前应用作用域内使用 `Table::set_style_definition` 和 `get_style_definition`。每个应用独立保存内置及自定义定义，同一应用内跨命令和请求保留。表格始终使用创建它的应用定义，不随外层作用域切换；独立表格具有自己的内置定义，应用外修改命名定义返回错误。

传入 `&style` 共享同一个可变样式对象，`style.clone()` 显式复制配置。修改共享定义也影响已使用它的表格；同名替换只影响后续查找，现有表格继续持有原对象。`TableStyle::from_name` 仍返回独立的内置预设副本。

`Table::new` 返回 `Result`，传播应用服务解析失败。样式文本及数组 getter 返回拥有所有权的快照，避免共享修改使借用失效。使用 `TableStyle::from(table.get_style())` 或列 getter 保留共享句柄；格式化回调在样式锁外执行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableStyle},
    support::facades::Artisan,
};

fn register_table_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:table-styles", |input: &Input, output: &mut Output| {
        let mut style = TableStyle::from_name("box")?;
        Table::set_style_definition("report", &style)?;
        let mut io = input.io(output);
        let mut table = Table::new(io.get_output())?;
        table.set_headers(["A", "B"]).set_rows([["one", "two"]]);
        table.set_style_named("report")?;
        style.set_horizontal_border_chars("!", None);
        table.render()?;
        Table::set_style_definition("report", TableStyle::from_name("compact")?)?;
        table.render()?;
        table.set_style_named("report")?.render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_style_named` 和 `set_column_style_named` 按名称解析，未定义名称返回错误；原类型 setter 也可接收借用样式。便利方法 `table` 使用应用当前默认定义，`table_named` 和 `table_with_named_columns` 支持命名表格与列样式。分隔单元格与跨度配置见[合并单元格](#table-cell-spans)。

## 显式整行分隔线 {#table-row-separators}

同时包含数据行与整行分隔线的集合使用 `TableRow`。`TableSeparator::new()` 表示分隔线，空数据数组仍是空行，二者不混用。普通数组和迭代器继续自动转换为数据行。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableRow, TableSeparator},
    support::facades::Artisan,
};

fn register_separators() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:separators", |input: &Input, output: &mut Output| {
        let rows = [
            TableRow::from(["one", "two"]),
            TableSeparator::new().into(),
            TableRow::from(["three"]),
        ];
        input.io(output).table_named(["A", "B"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

`set_rows`、`add_rows`、`add_row` 和表格便利方法接收这类异构行。`add_row`／`add_rows` 继续用 `?` 传播索引耗尽；`set_row` 可将同键分隔线替换为数据数组。开头、末尾和连续分隔线保留声明顺序。横向与纵向布局略过显式整行分隔线，纵向布局仍自行生成记录间分隔。单元格分隔选项与跨度配置见[合并单元格](#table-cell-spans)。

## 单元格样式与内部分隔 {#table-cell-styles}

使用选项键值对创建 `TableCellStyle`，或用 `Default` 取得 `fg=default`、`bg=default`、无文本选项、左对齐和无自定义格式。支持键为 `fg`、`bg`、`options`、`align` 和 `cellFormat`，最后一项保留配置拼写。`align` 接收 `left`、`center` 或 `right`；未知键和无效的非空对齐值在构造时失败。

`get_options`、`get_tag_options` 返回独立的映射，空值标签选项被略去。`get_pad_by_align`、`get_cell_format` 返回 `Result`，保留显式空对齐与非法格式类型延迟到读取时失败的行为；格式的普通标量沿用既有字符串转换。`DEFAULT_ALIGN` 为 `left`。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, TableCell, TableCellStyle, TableSeparator},
    support::facades::Artisan,
};

fn register_cell_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:cell-styles", |input: &Input, output: &mut Output| {
        let style = TableCellStyle::new([
            ("fg", serde_json::json!("green")),
            ("align", serde_json::json!("right")),
        ])?;
        let rows = vec![
            vec![TableCell::new("Ada").with_style(style), "42".into()],
            vec![TableSeparator::new().into(), TableSeparator::new().into()],
            vec!["Li".into(), "7".into()],
        ];
        input.io(output).table_named(["Name", "Score"], rows, "box-double")?;
        Ok(0)
    })?;
    Ok(())
}
```

用 `TableCell::with_style` 附加不可变样式，`get_style` 读取该样式。已有完整格式标签时保留其文本样式，单元格对齐仍生效；其他情况用 `cellFormat` 覆盖自动生成的颜色标签。按宽度换行与多行展开会生成普通文本行，可能丢弃原单元格样式；横向布局保留搬移的单元格样式，纵向布局将值转换为文本。

将 `TableSeparator` 转换为单元格后，按该列样式绘制内部边框，与整行 `TableRow::Separator` 不同。跨度与分隔构造选项见[合并单元格](#table-cell-spans)。

每条实际输出行在开始时读取行文本格式。格式化回调的修改会影响后续边框与下一行，不会回改正在渲染的本行单元格格式。

## 合并单元格与构造选项 {#table-cell-spans}

使用 `with_colspan`、`with_rowspan` 合并单元格。`get_colspan`、`get_rowspan` 复用现有纯量转换并返回有符号整数，默认均为 `1`。构造选项保留传入值，含零、负值；布局在无法使用这些值进行计算时传播错误。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, Table, TableCell},
    support::facades::Artisan,
};

fn register_spans() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:spans", |input: &Input, output: &mut Output| {
        let headers = vec![
            vec![TableCell::new("Summary").with_colspan(2)],
            vec!["Name".into(), "Score".into()],
        ];
        let rows = vec![
            vec![TableCell::new("Ada").with_rowspan(2), "42".into()],
            vec!["7".into()],
            vec![TableCell::new("Total").with_colspan(2)],
        ];
        let mut io = input.io(output);
        Table::new(io.get_output())?
            .set_headers_rows(headers)
            .set_rows(rows)
            .set_style_named("box-double")?
            .render()?;
        Ok(0)
    })?;
    Ok(())
}
```

`TableCellOptions` 汇集 `colspan`、`rowspan`、`style`，可传给单元格或分隔符的 `with_options`。跨度字段保存 JSON 值，`style` 为 `Option<TableCellStyle>`；未知字段与冒充样式对象的值由类型检查拒绝。`TableCell::default()` 创建空单元格，`TableCell::new` 包装既有单元格时重新使用默认选项。

`TableSeparator` 保留选项与继承 getter，并支持 `Clone`；同一分隔符放入多个拥有所有权的行时可克隆使用。跨度参与多表头、跨行填充、方向与列宽换行。布局可能更新列最小宽度，`set_column_widths([])` 清空这些最小值并保留最大宽度约束。分区重绘见下节。


## 控制台分区与表格更新 {#console-sections}

`Output::section()` 在当前输出流创建分区。`write`／`writeln`、`clear`、`overwrite`、`set_max_height`、`get_content` 和 `get_visible_content` 操作该分区。重写前面的分区时，后续分区按顺序清除并恢复；最大高度用于滚动可见内容。不启用装饰时按普通输出写入，清除不执行重绘。

`ConsoleSectionOutput` 暂时借用原输出。保存 `handle()`，写完其他分区后通过 `section_output(&handle)` 再次借用；句柄只属于一个输出，其他输出会拒绝它。`style(input)` 把既有样式／表格管线绑定到分区。显式借用替代共享可变流引用，不创建额外输出流。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, Table},
    support::facades::Artisan,
};

fn register_sections() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:sections", |_input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("other section", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut table = Table::new(&mut section)?;
        table.set_headers(["A", "B"]).set_rows([["x", "y"]]);
        table.append_row(["one", "two"])?;
        table.append_row(["longer", "last"])?;
        Ok(0)
    })?;
    Ok(())
}
```

`append_row` 要求分区输出；首次渲染后按计算行数清除表格区域，追加行并再次渲染，保留后续分区。普通输出会在添加行之前返回错误；只构建表格而不重绘请用 `add_row`。分区详细程度独立，初始格式器共享，也可为该分区替换。所选进度与问答输出见下节。


## 所选输出、进度条与问答 {#section-consumers}

`OutputStyle::get_output()` 返回携带当前输出和分区身份的 `OutputReference`。`OutputStyle::new`、`ProgressBar::new` 和 `new_with_redraw_interval` 同时接受它及原来的 `&mut Output`，普通调用语法不变。显式标注 `&mut Output` 参数的助手若要支持分区，应接受 `&mut impl AsOutput`。该借用允许读取原捕获缓冲，配置和写入转发到所选输出，不能通过可变解引用绕过分区。

进度条清除／重绘使用分区行数，并恢复后续分区。`bar.get_output()` 为回调写入和配置提供同一所选流；`as_section()` 提供分区操作，普通输出则返回错误。这种顺序借用替代在同一闭包中同时捕获可变流与进度条。

```rust
use openrust::{
    console::{ConsoleError, Input, Output, OutputOptions, OutputStyle, ProgressBar},
    support::facades::Artisan,
};

fn register_section_progress() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:section-progress", |input: &Input, output: &mut Output| {
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default())?;
            section.handle()
        };
        {
            let mut section = output.section();
            section.writeln("later", OutputOptions::default())?;
        }
        let mut section = output.section_output(&first)?;
        let mut style = OutputStyle::new(input, &mut section);
        let mut bar = ProgressBar::new(style.get_output(), 3)?;
        bar.set_format("<info>%current%/%max%</info>")
            .min_seconds_between_redraws(0.0)
            .max_seconds_between_redraws(0.0)
            .set_redraw_frequency(Some(1));
        bar.start()?;
        bar.advance()?;
        bar.display()?;
        bar.get_output().writeln("notice", OutputOptions::default())?;
        bar.advance()?;
        bar.clear()?;
        bar.display()?;
        bar.finish()?;
        Ok(0)
    })?;
    Ok(())
}
```

问答使用所选格式器，在随后空行之前计入提交输入的换行；校验重试只在问题开始前准备一次空行，不在每次重试重复执行。普通终端编辑直接回显输入，不把回显再次保存到分区内容。已验证流输入问答、确认、隐藏输入、重试、取消和普通交互 PTY；补全和多行行为见下节。


## 分区补全与多行输入 {#section-editors}

补全开始时没有选中候选。初次按上／下方向键选择最后／第一个候选，输入字符会筛选候选，Tab 接受当前候选，Enter 接受已选候选或输入值／默认值。候选可见时，第一次退格移除候选，下一次才删除已输入字符。单选和多选使用同一编辑器，支持逗号分隔候选。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputOptions, Question},
    support::facades::Artisan,
};

fn register_editor() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:editor", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut section = output.section();
            let mut style = section.style(input);
            let mut question = Question::new("Name", Some("Guest".into()))?;
            question.set_autocompleter_values(Some(vec![
                "Ada".into(), "Adam".into(), "Beta".into(),
            ]))?;
            let answer = style.ask_question(&question).await?;
            style.writeln(
                format!("answer={}", answer.as_str().unwrap_or_default()).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Question::set_multiline(true)` 读取到原生 EOF 键，保留中间换行；开始时直接按空 Enter 使用默认值。同时启用补全和多行时，补全优先，Enter 完成本次回答。普通多行回显使用原生异步编辑器，而非操作系统规范模式回显；终端重绘字节存在差异，答案与分区记账一致。

Ctrl+C 保持退出码 `130`，在命令处理完成前恢复终端模式。取消和超时沿用既有问答错误／默认值规则。Windows 终端实机验收仍属于发布检查。

## 嵌套输出样式 {#nested-output-styles}

`OutputStyle::new(input, &mut parent)` 包裹现有样式，写入和配置仍转发至同一输出，同时保留父级的换行与问答缓冲状态。助手通过 `console::contracts::NewLineAware` 读取真实换行契约。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    contracts::NewLineAware,
};
use openrust::support::facades::Artisan;

fn register_nested() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:nested", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut first = OutputStyle::new(input, output);
            first.new_line(1)?;
            {
                let mut second = OutputStyle::new(input, &mut first);
                second.write("A", false, OutputOptions::default())?;
                second.new_line(2)?;
                let inner = second.get_output().as_new_line_aware().unwrap();
                assert_eq!(NewLineAware::new_lines_written(&inner), 2);
                second.writeln("B", OutputOptions::default())?;
            }
            let count = NewLineAware::new_lines_written(&first);
            first.writeln(format!("count={count}").as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`new_lines_written()` 委托至最底层被包装的样式；`new_line_written()` 还保留任一包装层的真值。`new_line(n)` 向父级写入换行文本，不调用父级的 `new_line(n)`，两种操作的计数行为因此不同。

`get_output()` 借用当前直接选中的输出。`OutputReference::as_new_line_aware()` 仅在该目标是被包装样式时返回 `Some`，此时还能通过它的 `get_output()` 继续借用底层输出；普通流没有这两种能力。Rust 独占借用保证子级使用父级期间，不能并发从父级写入。

被包装样式不等于直接的控制台分区：表格追加重绘会拒绝它，进度条使用普通光标输出，也不会自动改走 stderr。需要分区专属行为时直接使用分区。一般自定义输出流及其余组件助手仍是独立能力。

## 标题、文本与列表 {#style-text-layout}

`OutputStyle` 提供 `title`、`section`、`text` 和 `listing`。主标题使用 `=` 下划线，分节标题使用 `-`，宽度按所选格式器与 Unicode 显示宽度计算。文本缩进一个空格，列表使用 ` * ` 前缀；块之间的空行取决于之前的实际输出。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_text_layout() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:text-layout", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.title("Hello")?;
            style.section("Part")?;
            style.text(&["one", "two"])?;
            style.listing(["A", "B"])?;
            style.title("")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`text` 接受字符串或借用的字符串数组，`listing` 接受字符串迭代器。空数组保留各助手自身的间距规则。行内格式标签继续生效，标题下划线忽略装饰，尾部反斜杠正常显示。无效格式返回既有的类型化控制台错误，保留失败前已经写出的内容。

`OutputStyle::section("Part")` 输出分节标题；`Output::section()` 创建可重绘输出分区，两者是独立接口。这四个助手支持所选分区与借用的嵌套样式，沿用同一格式器、详细程度和换行状态。

## 状态文本块 {#styled-message-blocks}

使用 `comment`、`success`、`error`、`warning`、`note`、`info` 和 `caution` 输出标准状态块。`block` 通过显式 `BlockOptions` 开放它们共用的渲染器。

```rust
use openrust::console::{AsyncCommandCallback, BlockOptions, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_blocks() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:blocks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.comment("Preparing report")?;
            style.success("Report ready")?;
            style.warning("Review the queue")?;
            style.block(&["Build finished", "Ready to deploy"], BlockOptions {
                kind: Some("BUILD"),
                style: Some("fg=blue"),
                padding: true,
                ..BlockOptions::default()
            })?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`BlockOptions::default()` 的 `kind`、`style` 为 `None`，`prefix` 为一个空格，`padding: false`、`escape: true`。`kind` 是可选的方括号标签；借用选项表达可选参数，不复制应用字符串。所有助手接受与 `text` 相同的借用字符串或字符串数组。

标准状态块转义消息标签，`comment` 保留行内格式；只有需要按格式器标记解释消息时，才设置 `escape: false`。`padding` 仅在装饰输出中添加首尾空行。块宽度在样式构造时确定，最多 120 列；配置为零时使用 120，Windows 还会预留一列。之后修改 `COLUMNS` 不改变该样式的块宽度。

`error` 输出错误块，命令退出码仍由返回值控制。无效格式或零宽度分区可能在部分输出后返回类型化控制台错误。问答校验重试复用同一错误块渲染器，包括格式器、转义与构造时宽度。

## 样式级问答助手 {#style-question-helpers}

样式对象也直接提供问答助手。简写方法采用默认参数，`_with` 变体传入可选参数；它们与 `ask_question` 共用 `Question`、`ChoiceQuestion`、`ConfirmationQuestion`、输入流和取消机制。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, QuestionCallback,
};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_style_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-questions", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let profile = style.ask_with_validator("Name", Some("Ada"), Some(
                QuestionCallback::sync(|value| Ok(json!({"name": value}))),
            )).await?;
            let _token = style.ask_hidden("Token").await?;
            let confirmed = style.confirm("Continue").await?;
            let color = style.choice_with(
                "Color", ["red", "green"], Some(json!("green")), false,
            ).await?;
            style.writeln(
                json!({"profile": profile, "confirmed": confirmed, "color": color})
                    .to_string().as_str(),
                OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ask`、`ask_with` 返回 `Option<String>`；`ask_with_validator` 保留校验器完整的 JSON 返回值。`ask_hidden` 返回隐藏输入的文本，`ask_hidden_with` 也保留结构化校验结果。示例使用令牌但不打印其内容。

`OutputStyle::confirm` 默认 `true`，命令级 `CommandIo::confirm` 默认 `false`，两者都可用 `confirm_with` 显式指定。样式的 `choice_with` 将匹配的默认值映射至选项键，`choice_pairs_with` 保留命名键，最后的布尔参数启用多选。

非交互模式下，非 null 默认值会执行校验器；null 默认值直接返回 null，不调用校验器。校验失败遵循现有重试规则，`ConsoleError::from_runtime_source` 显式标记需要立即终止的错误。需要设置次数、补全或超时时，创建 `Question` 并调用 `ask_question`。

## 表格输出目标与样式工厂 {#table-output-factory}

`Table::new` 接受现有 `AsOutput` 适配器：`Output`、`ConsoleSectionOutput`、`OutputReference`、`OutputStyle` 和盒装适配器。普通输出无需输入对象即可构造表格。表格现在只有一个输出借用生命周期，显式 `Table<'a, 'o>` 注解改为 `Table<'a>`，普通构造调用语法不变。

`OutputStyle::create_table` 选择底层输出，克隆命名的样式指南定义，并采用 `info` 表头格式。真实控制台使用独立分区，捕获输出仍使用捕获流；该方法不添加末尾空行。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::support::facades::Artisan;

fn register_factory_table() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:factory-table", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
        let mut style = OutputStyle::new(input, output);
        let mut table = style.create_table()?;
        table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
        table.render()?;
        Ok(0)
        })
    }))?;
    Ok(())
}
```

追加支持由实际构造目标决定。直接选择分区时，使用 `Table::new(&mut section)` 或 `Table::new(style.get_output())`。传入 `&mut style` 表示包装该样式，即使最终写往分区，`append_row` 也会拒绝；渲染与配置仍遵循同一格式器和详细程度规则。

修改工厂返回表格的样式不会修改应用的命名样式指南定义。上述构造适配器不代表其余通用自定义输出接口已经完成。

## 样式表格与定义列表 {#style-table-helpers}

`OutputStyle::table` 和 `horizontal_table` 使用现有表格工厂，仅在渲染成功后添加一个空行，不额外插入前置块间距。数据行接受 `TableRow`，包含显式 `TableSeparator`；普通表头数组直接传入，多层表头通过 `TableHeaders::rows` 与单元值区分。

`definition_list` 接受 `DefinitionListItem` 迭代器：标题跨两列，`entry` 提供单个键值，`entries` 保留有序数组输入并只渲染首项，`TableSeparator::new().into()` 添加分隔线。空 `Entries` 仍保留一个空项。枚举表达原始字符串／数组／分隔线联合类型，不支持的类型在编译时拒绝。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, DefinitionListItem, OutputStyle, TableCell, TableHeaders};
use openrust::support::facades::Artisan;

fn register_style_tables() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:style-tables", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let rows = vec![
                vec![TableCell::from("Ada"), TableCell::from(42)],
                vec![TableCell::from("Lin"), serde_json::Value::Null.into()],
            ];
            let mut style = OutputStyle::new(input, output);
            style.table(["Name", "Value"], rows)?;
            style.horizontal_table(TableHeaders::rows([["A", "B"], ["C", "D"]]), [["x", "y"]])?;
            style.definition_list([
                DefinitionListItem::from("Profile"),
                DefinitionListItem::entry("Name", "Ada"),
                DefinitionListItem::entry("Count", 42),
            ])?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

可空纯量单元使用 `serde_json::Value::Null.into()`。普通单元里的 null 和空字符串显示相同，水平表格仍须区分：跨列标题下的 null 被省略，空字符串保留为单元；显式样式／跨列单元仍作为单元对象处理。定义列表标题复用这一公共表格行为，不另造渲染器。格式错误保留既有 `ConsoleError` 来源链和部分输出，失败后不补成功空行。

## 样式接口 {#style-interface}

`openrust::console::style::StyleInterface` 是正式公开 Trait，`openrust::console::StyleInterface` 导出同一类型。它包含标题、分节、列表、文本、成功／错误／警告／备注／注意消息、表格、四类问答、换行及进度开始／推进／完成。`OutputStyle` 实现全部 18 个入口，服务可以接收泛型样式或 `&mut dyn StyleInterface`。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::StyleInterface;
use openrust::support::facades::Artisan;

fn render_report<S: StyleInterface + ?Sized>(style: &mut S) -> Result<(), ConsoleError> {
    style.title("Report")?;
    style.table(["Name", "Value"].into(), vec![["Ada", "42"].into()])?;
    style.success("Ready".into())
}

fn register_style_interface() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:interface", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            render_report(&mut style)?;
            let dynamic: &mut dyn StyleInterface = &mut style;
            render_report(dynamic)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

Trait 调用显式传入原可选参数：`ask` 接受可选默认值与校验器，`ask_hidden` 接受可选校验器，`confirm` 接受布尔默认值，`choice` 接受有序 `Vec<(ChoiceKey, Value)>` 与可选默认值；整数键表达顺序选择，字符串键保留关联结果。问答保留完整 `serde_json::Value` 返回值。`table` 接受 `TableHeaders` 与 `Vec<TableRow>`，消息复用 `OutputMessages`，`new_line`／`progress_advance` 显式传入次数。具体类型的既有便利方法继续可用。

四类异步 Trait 方法返回既有 `ConsoleFuture`，为支持动态派发，在 Trait 边界增加一次 Future 装箱；直接调用具体 `OutputStyle` 的异步方法不增加这一层。同步 Trait 方法不装箱 Future。Trait 为既有异步命令执行要求 `Send`，不要求实现 `NewLineAware`。连续问答共享同一输入与输出历史，包含隐藏输入和失败恢复。

## 错误样式与基础样式类型 {#error-style}

`OutputStyle::get_error_style` 返回基础类型 `ConsoleStyle`，也可从 `openrust::console::ConsoleStyle` 导入。基础类型提供同一套已有渲染助手并实现 `StyleInterface`，但不实现 `NewLineAware`，也不暴露派生类型的换行计数。也可以使用 `ConsoleStyle::new(input, output)` 直接构造。

直接包装真实控制台时选择 stderr；捕获流、分区和包装样式继续使用已选输出，不会一律切换到独立错误缓冲。格式器、装饰和详细程度跟随该目标；修改直接控制台错误样式的设置，不会改变 stdout 的设置。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle};
use openrust::console::style::ConsoleStyle;
use openrust::support::facades::Artisan;

fn register_error_style() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:error-style", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut original = OutputStyle::new(input, output);
            {
                let mut errors: ConsoleStyle<'_> = original.get_error_style();
                errors.warning("Review")?;
                errors.table(["Name", "Value"], [["Ada", "42"]])?;
            }
            original.info("Done")?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

返回的样式独占借用输出，结束其作用域后再通过原样式写入；它共享真实目标，不复制输出缓冲。换行计数查询在基础样式边界停止，实际写入仍到达底层目标。嵌套调用 `get_error_style` 仍返回基础类型。

`ConsoleStyle::write_iter` 和 `writeln_iter` 在请求下一项之前逐条写出；派生 `OutputStyle` 保留原有的换行统计准备。两种类型复用同一格式器与渲染引擎，不代表通用第三方 `OutputInterface` 已实现。树助手及其余组件 API 另行跟踪。

## 树节点与惰性子节点 {#tree-nodes}

`TreeNode` 保存只读标签和共享的节点身份。`new(value)` 创建空节点，`with_children(value, children)` 添加初始子节点，`default()` 使用空标签。`add_child` 接受节点、字符串或 `TreeChild::lazy`；克隆节点会共享身份，不复制整棵树。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle, TreeChild, TreeNode, TreeValue};
use openrust::support::facades::Artisan;

fn register_tree_nodes() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tree-nodes", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let root = TreeNode::from_values(
                [("languages", TreeValue::branch([(0_i64, "Rust"), (1_i64, "Go")]))],
                Some(TreeNode::new("Project")),
            )?;
            root.add_child("Ready");
            root.add_child(TreeChild::lazy(|| [Ok(TreeNode::new("Generated"))]));
            OutputStyle::new(input, output).writeln(
                format!("{}:{}", root.get_value(), root.count()?).as_str(),
                OutputOptions::default(),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`from_values` 接受有序键值对。`TreeValue::branch` 表达嵌套值；嵌套键成为标签，标量键被忽略。`TreeValue::Node` 本身可迭代，因此在所给键下展开其孩子，不保留该节点标签；要保留节点本身，使用 `add_child(node)`。`Some(root)` 向同一根节点追加，`None` 创建空根。

`get_children`、`get_iterator` 和 `&TreeNode` 的原生迭代均返回 `Result<TreeNode, ConsoleError>` 项。首次推进时取得子列表快照：此前的改动会包含在内，之后的追加需新遍历才能看到。工厂在节点锁外执行，每次遍历及 `count()` 都会重新调用；工厂返回拥有所有权的 `Send` 迭代器，可传播错误。`lazy_keyed` 保留 `ArrayKey` 键，通过迭代器的 `key()` 读取；`lazy` 对每个工厂从零编号，直接子节点使用单独的顺序编号。

节点使用标准 `Arc`，不提供循环垃圾收集。回调反向引用请用 `downgrade`／`WeakTreeNode::upgrade`；若主动创建强引用循环，结束使用后通过 `clear_children` 解除，`ptr_eq` 比较节点身份。通过 `from_values` 展开循环节点会返回错误，避免无限递归。普通根节点及其孩子在所有者和迭代器释放后回收。

可失败的输入生产者使用 `try_from_values`，逐项返回 `Result<(key, value), ConsoleError>`；失败前已追加的兄弟节点会保留。

嵌套的一次性生产者使用 `TreeValue::iterable` 或 `try_iterable`，拥有 `Send + 'static` 迭代器，后者接受可失败的项。输入按顺序消费，不提前收集；嵌套分支只有完整成功后才附加。`TreeValue` 不克隆或重放一次性生产者。

## 树输出与树游标 {#trees}

`OutputStyle` 和基础 `ConsoleStyle` 均提供 `tree(values, root)` 与 `create_tree(values, root)`，同直接构造入口共用 `TreeHelper`。默认空根传入空字符串；即使空树也写出根行，渲染结束后不额外追加空白行。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, TreeHelper, TreeNode, TreeStyle, TreeValue, TreeValues};
use openrust::support::facades::Artisan;

fn register_tree_output() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:trees", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            style.tree([(0_i64, "A"), (1_i64, "B")], "Project")?;
            {
                let mut tree = style.create_tree(
                    [("Group", TreeValue::branch([(0_i64, "Leaf")]))], "Root",
                )?;
                tree.render()?;
            }
            let root = TreeNode::with_children("Styled", ["Leaf"]);
            TreeHelper::create_tree(
                &mut style, root, TreeValues::new(), Some(TreeStyle::rounded()),
            )?.render()?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TreeHelper::create_tree(output, root, values, style)` 接受根标签、已有 `TreeNode` 或 `None`；空 `TreeValues` 保留已有孩子，样式传 `None` 使用默认值。值会追加到同一个根节点。样式工厂只有直接包装真实控制台时才新建分区；捕获流、已有分区和嵌套样式保留所选输出，错误样式也保留 stdout／stderr 选择。

助手提供 `current`、`key`、`advance`、`rewind`、`valid`、`has_children` 和 `get_children`。构造时预读首个孩子；`advance()` 仅推进，标准迭代器仍以 `next()` 返回元素，`current()` 返回共享同一身份的节点，`key()` 返回整数。无效游标的 current／key 访问返回 `ConsoleTreeError`；数字字符串键沿用已有弱标量规则，非数或溢出键失败。`for item in &mut helper` 遍历直接孩子；`recursive_iterator()` 深度优先返回 `TreeEntry { node, line }`，不包含根，也不写输出。`TreeStyle::apply_prefixes` 配置该迭代器，重复节点检测由 `render()` 执行。

游标为单次消费：越过首次 yield 前可以 rewind，空游标也可反复 rewind。已推进的非空游标重绕失败，包括第二次 `render()`；渲染会先写根行再报告该错误。再次遍历请创建新助手。`get_children()` 借用所选输出，需要结束子助手的作用域后再使用父助手。即使 `valid()` 为 false，最后的节点仍由游标持有，直到游标释放。

工厂可能在构造、子节点探测和创建子游标时执行，不应假设每个节点只调用一次。底层遍历将子树探测错误视为没有孩子；推进迭代器时的错误会传播，且可能已经写出部分输出。渲染中再次遇到同一节点身份会返回 `ConsoleTreeError::Cycle`，DAG 中复用节点也属于重复；不同节点可以具有相同标签。渲染期间保留已访问节点，避免临时节点分配地址复用造成误判。

`TreeStyle` 提供 `default`、`r#box`、`box_double`、`compact`、`light`、`minimal` 和 `rounded`。`box` 是 Rust 保留字，因此使用原始标识符 `r#box`。`TreeStyle::new` 的六个只读前缀依次为：有下一项的末端、最后末端、左侧、有下一项的中段、最后中段、右侧。预设使用静态字符串，自定义前缀在子助手间共享。

## 组件文本变换器 {#component-mutators}

四个公开变换器共用对象安全的 `Mutator::apply(&str) -> Result<String, ConsoleError>` 接口。显式 `apply` 对应可调用对象；直接实现 Rust 可调用 trait 需要不稳定语言特性。输入固定为字符串，不复现 PHP 对任意对象或 null 的弱转换。

```rust
use openrust::console::{ConsoleError, view::components::mutators::{EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation, EnsureRelativePaths, Mutator}};

fn transform_component_text(value: &str) -> Result<String, ConsoleError> {
    let value = EnsureDynamicContentIsHighlighted.apply(value)?;
    let value = EnsurePunctuation.apply(&value)?;
    let value = EnsureRelativePaths.apply(&value)?;
    EnsureNoPunctuation.apply(&value)
}
```

动态内容变换将非空 `[内容]` 包入粗体标记。标点判断只识别末尾 ASCII `.`, `?`, `!`, `:`，不裁剪空白、不规范化全角标点。`EnsureNoPunctuation` 只移除一个末尾字符。相对路径变换在当前应用作用域中移除所有精确匹配的 `base_path + "/"`，不规范化路径；没有应用作用域或缺少 `path.base` 绑定时保持原文。这是展示格式处理，不是路径安全校验。

## 行组件 {#line-components}

`Info`、`Success`、`Warn` 和 `Error` 添加标签、高亮方括号内的动态内容，并补齐句末标点。各类型通过 `new(output)` 借用所选输出，`render(text)` 使用普通输出级别，`render_with(text, verbosity)` 指定级别门槛。`render_with` 也接受 `OutputOptions`，保留输出级别门槛以及 raw／plain 输出模式。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Error as ErrorComponent, Info, Line, Success, Warn}};
use openrust::support::facades::Artisan;

fn register_line_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Info::new(&mut style).render("Creating [reports]")?;
            Success::new(&mut style).render("Ready")?;
            Warn::new(&mut style).render("Check [configuration]")?;
            ErrorComponent::new(&mut style).render("Missing [field]")?;
            Line::new(&mut style).render("info", "Custom [line]")?;
            Line::new(&mut style).render_with("warn", "Verbose detail", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Line` 接受 `info`、`success`、`warn`、`error` 样式键；未知键返回 `ConsoleError`，不写入新行。组件共用所选输出、格式器和换行状态。`Error` 在该输出上渲染 ERROR 标签；底层控制台支持时，通过 `get_error_style()` 选择 stderr。

组件文字会合并 ASCII 空格、制表符和换行；NUL 转为空格，其他 C0 控制字符被移除，包括原始 ANSI 转义字符，不换行空格保留。需要样式时使用格式器标签。长文本不自动折行。支持换行统计的样式决定前置间距，底层样式和原始捕获使用一行前置间距。

`OutputFormat::Normal` 保持原有默认值 0。`ExplicitNormal` 对应显式 normal 位 1，保留与输出级别组合时的换行计数。穷尽匹配 `OutputFormat` 的代码需要加入该分支。

## 提示框、列表与双列详情 {#display-components}

这些组件借用所选输出，保留其格式器以及 stdout／stderr 选择。`render` 使用普通输出级别，`render_with` 接受 `OutputOptions` 或输出级别门槛。终端宽度优先读取 `COLUMNS`。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle, OutputVerbosity,
    view::components::{Alert, BulletList, TwoColumnDetail}};
use openrust::support::facades::Artisan;

fn register_display_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:display", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            Alert::new(&mut style).render("Review [reports]")?;
            BulletList::new(&mut style).render(["CSV.", "JSON.", "HTML."])?;
            TwoColumnDetail::new(&mut style).render("Report [daily].", Some("Ready"))?;
            TwoColumnDetail::new(&mut style).render("Optional value.", None)?;
            Alert::new(&mut style).render_with("Verbose alert", OutputVerbosity::Verbose)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Alert` 使用整行背景，将内容转为大写并居中；内容超过可用宽度时按带样式文本规则截断，无法处理的宽度返回 `ConsoleError`。`BulletList` 对每项高亮方括号内容、移除一个末尾 ASCII 标点，并解析相对路径。输入只收集一次，随后逐阶段处理所有项目，再一次写入；空列表仍写入一个换行。

`TwoColumnDetail` 移除第一列的末尾标点，保留第二列标点。`None` 和 `Some("")` 省略第二列，其他输入在文字规范化后仍保留列位置。点状填充按至多 150 列的布局宽度计算，过长的列内容仍完整输出。组件构造后，通过原共享句柄变更格式器样式仍立即生效。

## 任务结果标识 {#task-results}

`console::view::TaskResult` 定义 `Success = 1`、`Failure = 2` 和 `Skipped = 3`。`value()` 返回整数背值，`cases()` 保留声明顺序。`TryFrom<i64>` 拒绝其他值，不把无效标识当成成功。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    view::TaskResult};
use openrust::support::facades::Artisan;

fn register_task_results() -> Result<(), ConsoleError> {
    Artisan::command_with("tasks:states", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            for state in TaskResult::cases() {
                style.writeln(format!("{state:?}:{}", state.value()).as_str(), OutputOptions::default())?;
            }
            let failure = TaskResult::try_from(2)?;
            assert_eq!(failure, TaskResult::Failure);
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`TaskResult` 序列化为整数背值，不是 Rust 变体名。JSON 反序列化要求整数且属于已知变体。请求的 `enum::<TaskResult>` 通过 `BackedEnum` 读取同一类型，沿用既有请求标量转换与默认值机制。例如查询值 `2.0` 解析为 `Failure`，未知整数使用请求默认值。JSON 反序列化与请求输入转换是两个不同入口。

## 任务显示 {#task-components}

`Task` 借用所选输出。`render` 不运行回调、不显示耗时，直接显示任务完成。`render_task` 运行一次同步回调，并安全借出同一输出供回调使用。对应的 `_with` 方法接受输出选项；隐藏显示不会跳过回调。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputStyle,
    view::{TaskResult, components::Task}};
use openrust::support::facades::Artisan;

fn register_task_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:tasks", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut task = Task::new(&mut style);
            task.render("Ready")?;
            task.render_task("Build [report].", |selected| {
                selected.write("CSV ", false, openrust::console::OutputOptions::default())?;
                Ok(TaskResult::Success)
            })?;
            task.render_task("Optional [CSV].", |_| Ok(TaskResult::Skipped))?;
            task.render_task("Boolean [false].", |_| Ok(false))?;
            task.render_task("Failed status.", |_| Ok(TaskResult::Failure))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

任务回调中的 `TaskResult` 转换为整数背值。只有整数 `2` 显示 FAIL、整数 `3` 显示 SKIPPED；`false`、null（`()`）、浮点数及字符串均显示 DONE。返回值显式转换为 `serde_json::Value`，不会隐式序列化任意对象。显示 FAIL 本身不会改变命令退出码。回调返回 `Err(ConsoleError)` 时先显示 FAIL，再传播错误；Rust 展开栈时同样先收尾显示，再继续 panic。

耗时使用墙上时钟。不超过一秒时显示带千位分组、两位小数和 half-up 舍入的毫秒；超过一秒后使用参考默认短单位（`yr`、`mo`、`w`、`d`、`h`、`m`、`s`），舍弃不足一秒的部分。此显示中一个月按四周、一年按十二个这样的月进位。`run_time_for_humans(start, Some(end))` 接受明确的 Unix 秒数用于格式化。`set_run_time_formatter(Some(Box::new(...)))` 通过组合替代耗时方法覆盖，传 `None` 恢复默认。格式器接受开始时间及可选结束时间，返回 `Result<String, ConsoleError>`，可按应用需要自定义耗时措辞。此显示入口不提供通用日期区间或语言工厂。

## 问答帮助器与临时作用域 {#question-helpers}

`QuestionHelper` 使用组件提示：两个前导空格、粗体文本、末尾标点和 `❯` 光标。`DefaultQuestionHelper` 使用普通样式提示。二者均提供 `ask(input, output, question)` 和 `get_name()`。直接调用帮助器时沿用传入的输入及所选输出；裸控制台输出选取 stderr，已有样式则保留自己的流。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputOptions, OutputStyle,
    QuestionHelper};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_question_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-helpers", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let first = {
                let mut scoped = style.using_question_helper(Arc::new(QuestionHelper::new()));
                scoped.ask_with("Name", Some("Guest")).await?
            };
            let second = style.ask_with("Next", Some("Guest")).await?;
            style.writeln(format!("Users:{}/{}", first.unwrap_or_default(), second.unwrap_or_default()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`using_question_helper` 借用同一样式，作用域结束时恢复原帮助器实例，包括错误、展开栈及取消。此前未初始化帮助器时，恢复普通默认帮助器。`OutputStyle` 和 `ConsoleStyle` 均支持此作用域。`QuestionStyle` 通过 trait 提供相同操作，`QuestionHelperScope::new` 可接收动态样式。

实现 `QuestionHelperBehavior` 可自定义标点、提示或重试错误，再用 `set_question_helper(Some(Arc::new(...)))` 安装或使用临时作用域。`write_prompt` 写入提示，并返回已格式化的末尾光标文本供原生行编辑重绘。`write_error` 接收原输出是否为样式的标记。`Question::get_kind()` 提供借用的 `QuestionKind` 视图，可区分文本、确认和选择题，并读取原选择键值。这些帮助器尚未公开通用帮助器集合注册表。

既有原生输入引擎保留完整 UTF-8 长行，使用原生终端 API，不执行 shell 命令。`disable_stty()` 保持不调用外部 stty 的保证，不关闭原生补全或隐藏输入；这与依赖 shell 的终端实现不同。多行提示继续使用应用译器。

## 问答组件 {#question-components}

`Ask`、`AskWithCompletion`、`Choice`、`Confirm` 和 `Secret` 借用当前 `QuestionStyle`，支持 `OutputStyle` 和 `ConsoleStyle`。每次 render 临时选取组件帮助器，结束后恢复原帮助器；构造、校验失败及取消均走相同恢复路径。

```rust
use openrust::console::{AsyncCommandCallback, CompletionCallback, ConsoleError, OutputOptions,
    OutputStyle, view::components::{Ask, AskWithCompletion, Choice, Confirm, Secret}};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_question_components() -> Result<(), ConsoleError> {
    Artisan::command_with("reports:question-components", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let name = Ask::new(&mut style).render_with("Name", Some(json!("Guest")), false).await?;
            let color = AskWithCompletion::new(&mut style).render_with_callback(
                "Color", CompletionCallback::sync(|typed| {
                    Ok(["red", "green"].into_iter().filter(|value| value.starts_with(&typed)).map(str::to_owned).collect())
                }), Some(json!("red")),
            ).await?;
            let confirmed = Confirm::new(&mut style).render("Continue").await?;
            let format = Choice::new(&mut style).render_with(
                "Format", ["CSV", "JSON"], Some(json!(0)), None, false,
            ).await?;
            let secret = Secret::new(&mut style).render("Password").await?;
            style.writeln(format!("User:{name}; Color:{color}; Confirmed:{confirmed}; Format:{format}; Secret supplied:{}", !secret.is_null()).as_str(), OutputOptions::default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`Ask::render` 无默认值，读取一行；`render_with` 接受标量 JSON 默认值及多行开关。`AskWithCompletion::render` 接收候选字符串，`render_with` 增加默认值，`render_with_callback` 使用既有支持异步的补全回调。候选输入只遍历一次。这些组件返回 `serde_json::Value`；`Confirm` 返回 `bool`，默认 false。`Secret::render_with(question, false)` 在原生隐藏不可用时禁止明文回退，请勿打印返回的秘密。

`Choice::render` 使用列表，无默认值及尝试次数上限。`render_with` 增加默认值、最多尝试次数及多选开关，次数为零时报错。`render_pairs_with` 保留关联键和稀疏整数键。组件把非 list 键布局视为关联选择：稀疏数字键返回 `"1"` 这样的字符串键；标准列表返回选中值。普通 `ChoiceQuestion::from_pairs` 仍沿用原来的字符串键分类机制。

`Choice::get_choice_question` 和 `get_choice_question_pairs` 提供已准备的问题定义，供组合使用；`is_assoc` 检查 `ChoiceQuestion::get_choices` 返回的规范键值顺序。`ChoiceQuestion::from_pairs_with_associative` 接收 `Send + Sync` 键分类函数，参数是规范键值；分类结果在构造或重建校验器时保存，不在每次回答时重新计算。补全候选保留构造时的快照。克隆保留分类，同时沿用已有独立 trim 设置。


### 组件工厂 {#component-factories}

`CommandIo::components()` 和 `output_components()` 返回借用的 `Factory`，不再返回普通输出流。类型化方法复用对应组件：`info`、`success`、`warn`、`error`、`line`、`alert`、`bullet_list`、`two_column_detail`、`task`、`ask`、`ask_with_completion`、`choice`、`confirm` 和 `secret`。显示方法返回 `Result<(), ConsoleError>`；问答方法异步执行。需要直接操作流时使用 `get_output()` 或 `AsOutput`。

```rust
use openrust::{console::{AsyncCommandCallback, ConsoleError}, support::facades::Artisan};
fn register_component_factories() -> Result<(), ConsoleError> {
    Artisan::command_with("components:save", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let name = io.components().ask_with("Name", Some("Guest".into()), false).await?;
            io.output_components().info(&format!("Saved {name}"))?;
            io.components().task_callback("Index", |_| Ok(true))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

工厂保留命令最初的输出样式和问答输入。`set_output` 只替换普通输出，多次替换不会迁移组件输出或问答输入；`set_input` 更换命令输入访问，不替换原样式。独立的 `Factory::new(&mut output)` 可在 `Output`、输出引用和分区上显示组件。问答需要 `OutputStyle`、`ConsoleStyle` 或自定义 `ComponentOutput::question_style` 实现；普通流会在打印问题前返回运行时错误。任务回调接收同一所选输出引用，保留任务失败显示与错误传播行为。


### 动态组件调用 {#dynamic-component-calls}

组件名称在运行时确定时使用 `Factory::call(name, parameters).await`。名称保留动态字符串契约：`askWithCompletion`、`bulletList` 和 `twoColumnDetail`，不区分大小写，但不把下划线名称作为别名。未知名称返回 `ConsoleError`。普通 Rust 方法继续使用蛇形命名。

```rust
use openrust::{
    console::{AsyncCommandCallback, CompletionCallback, ConsoleError, view::components::ComponentArgument},
    support::facades::Artisan,
};
use serde_json::json;
fn register_dynamic_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:dynamic", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let color = io.components().call("askWithCompletion", vec![
                "Color".into(), CompletionCallback::sync(|_| Ok(vec!["red".into(), "green".into()])).into(),
                json!("red").into(),
            ]).await?;
            io.components().call("info", vec!["Saved".into()]).await?;
            io.components().call("task", vec!["Index".into(), ComponentArgument::task(|_| Ok(true))]).await?;
            io.line(&format!("Choice:{color}"))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ComponentArgument::Value` 承载 JSON 数据。显示调用返回 JSON null，问答返回既有值，确认返回 JSON 布尔值。`Completion` 接收现有 `CompletionCallback`；`ComponentArgument::task` 持有编译后的 `FnOnce` 回调，可在调用期间借用 `Send` 状态。回调只执行一次，错误传播前仍完成任务显示收尾。字符串是数据，不作为可执行函数名。`Options` 接收 `OutputOptions`。整数选项接受文档常量中的单个详细级别与单个格式标志；冲突位掩码和非法参数返回错误。既有所有权、问答取消及临时帮助器恢复规则仍适用。

文本参数误传数组时在渲染前拒绝，不会因错误把未完成编译的 HTML 模板遗留在进程输出缓冲中。


### 自定义控制台组件 {#custom-console-components}

在应用 Provider 中注册编译后的构造器。每次动态调用都新建 `RenderableComponent`，借出原输出上下文，等待 `render` 后释放实例。构造器由所属应用的 `ComponentRegistry` 共享，可变组件实例不共享。将 `BadgeProvider` 加入正常 Provider 清单，将 `register_custom_components` 接入控制台路由。

```rust
use openrust::{
    console::{AsOutput as _, AsyncCommandCallback, ConsoleError, ConsoleFuture, OutputFormat,
        view::components::{ComponentArgument, ComponentConstructor, ComponentContext, ComponentRegistry, RenderableComponent}},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};
use serde_json::{Value, json};
use std::sync::Arc;
struct Badge<'a>(ComponentContext<'a>);
struct BadgeConstructor;
impl ComponentConstructor for BadgeConstructor {
    fn make<'a>(&'a self, output: ComponentContext<'a>) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError> {
        Ok(Box::new(Badge(output)))
    }
}
impl RenderableComponent for Badge<'_> {
    fn render<'a>(&'a mut self, parameters: Vec<ComponentArgument<'a>>) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async move {
            let label = match parameters.first() {
                Some(ComponentArgument::Value(Value::String(label))) => label,
                _ => return Err(ConsoleError::new("需要文本标签")),
            };
            self.0.as_output().writeln(&format!("Badge:{label}"), OutputFormat::Raw)?;
            Ok(json!({"label": label}))
        })
    }
}
struct BadgeProvider;
impl ServiceProvider for BadgeProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            app.container().make::<ComponentRegistry>()?.register("Badge", Arc::new(BadgeConstructor))?;
            Ok(())
        })
    }
}
fn register_custom_components() -> Result<(), ConsoleError> {
    Artisan::command_with("components:badge", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let result = io.components().call("Badge", vec!["Ready".into()]).await?;
            io.line(&result.to_string())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

名称可以包含命名空间分隔符，例如 `Widgets\Badge`。注册不区分大小写；重复、非法及已有内置组件名称会被拒绝。构造器和其析构器在注册表锁外执行。构造及渲染错误继续传播；取消调用后释放组件及输出借用。

`Factory::new` 记录当前应用容器，用于查找自定义组件；进入另一个应用作用域不会更换这项注册归属。独立消费者可用 `Factory::with_registry(&mut output, registry)` 显式传入 `Arc<ComponentRegistry>`。数据与回调继续通过 `ComponentArgument` 传递。

`ComponentContext` 实现 `AsOutput`。`question_style()` 返回原问答样式，或在打印提示前返回运行时错误。自定义输出适配器实现 `ComponentOutput::component_context`：返回 `ComponentContext::Style` 保留问答输入，普通流返回 `ComponentContext::Output`。默认 `ComponentOutput::question_style` 跟随该上下文；已有显式问答样式覆盖仍适用于内置问答组件。


### 组件数据与问答作用域 {#component-data-scopes}

`Component` 通过组合复用组件能力。可借用 `ComponentOutput` 构造，也可在已注册组件的构造器中使用 `from_context`。`get_output` 和 `AsOutput` 借用原输出；`question_style` 提供已有问答能力，不替换输入。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputFormat, OutputStyle, Question,
        view::components::{Component, DefaultMutatorConstructor, mutators::EnsureNoPunctuation}},
    support::facades::Artisan,
};
use serde_json::json;
fn register_component_data() -> Result<(), ConsoleError> {
    Artisan::command_with("components:data", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut style = OutputStyle::new(input, output);
            let mut component = Component::new(&mut style);
            let mutator = DefaultMutatorConstructor::<EnsureNoPunctuation>::new();
            let data = component.mutate(json!({"message": "Created [report].", "active": true}), &[&mutator])?;
            let mut calls = 0;
            let name = component.using_question_helper(async |mut style| {
                calls += 1;
                let question = Question::new("Name", Some(json!("Guest")))?;
                style.ask_question(&question).await
            }).await?;
            component.get_output().writeln(
                format!("Data:{data}; Name:{name}; Calls:{calls}").as_str(), OutputFormat::Raw,
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`mutate` 按顺序执行构造器，每轮为每项变换器创建一次对象，再应用于各个顶层值。数组保留位置，对象保留键。构造器清单为空时返回原数据。`DefaultMutatorConstructor<M>` 使用 `M::default`；自定义 `MutatorConstructor` 可创建有状态对象或返回构造错误。发生错误后不再执行后续管线。

`Mutator::apply` 继续作为字符串接口；`apply_value` 处理 JSON 值，自定义变换器可覆盖。去标点时，无须变化的值保留原类型，不会把数字、布尔值和数组默默转为字符串。高亮变换会将值转为文本。未绑定应用根路径时，相对路径变换保留原值；绑定后，数组目标保留键并转换其直接成员，遵循字符串替换行为。

`using_question_helper` 接受可借用局部状态的异步闭包，临时安装组件帮助器，在成功、错误、panic 展开及取消后恢复同一个原帮助器。原帮助器尚未初始化时恢复普通默认帮助器。闭包内可以更新借用的局部状态；普通输出会在调用闭包前失败。可恢复的失败优先使用 `Result`。

闭包接收 `QuestionStyleReference`，即原样式的短期借用，支持 `QuestionStyle` 和 `AsOutput`。使用 `async |mut style|` 绑定。具名引用让要求 `Send` 的命令处理器能明确识别借用生命周期，不另分配样式对象，也不复制输入。


### 编译组件标记 {#component-markup-compilation}

`Component::compile(view, data)` 返回组件 HTML 标记，不写入终端输出。默认编译器复用 Blade，内嵌四个模板；不可变编译结果共享，每次调用的数据独立。运行时不执行源文件，也不写编译视图目录；修改内嵌模板需要重新构建二进制。

| 视图 | 必需数据键 |
| --- | --- |
| `alert` | `content` |
| `line` | `marginTop`, `bgColor`, `fgColor`, `title`, `content` |
| `bullet-list` | `elements` |
| `two-column-detail` | `first`, `second` |

这些名称是模板数据键，保持原拼写。内容执行 HTML 转义；行样式值与标题使用模板的原始插值。`elements` 接受标量列表或具名条目。缺少变量、类型非法及未知内置视图返回 `ConsoleError`。

```rust
use openrust::{
    console::{AsyncCommandCallback, ConsoleError, OutputFormat,
        view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}},
    support::facades::Artisan,
};
use serde_json::{Value, json};
struct HeadingCompiler;
impl ComponentViewCompiler for HeadingCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        if view == "heading" {
            let text = data["text"].as_str().ok_or_else(|| ConsoleError::new("需要标题文本"))?;
            return Ok(format!("<strong>{}</strong>", text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")));
        }
        BuiltinComponentViewCompiler::new().compile(view, data, output)
    }
}
fn register_component_compilation() -> Result<(), ConsoleError> {
    Artisan::command_with("components:compile", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            let html = component.compile("alert", json!({"content": "Compiled <report>"}))?;
            component.get_output().writeln(format!("Builtin:{html}").as_str(), OutputFormat::Raw)?;
            component.set_view_compiler(Some(Box::new(HeadingCompiler)));
            let html = component.compile("heading", json!({"text": "Ready"}))?;
            component.get_output().writeln(format!("Custom:{html}").as_str(), OutputFormat::Raw)?;
            component.set_view_compiler(None);
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`ComponentViewCompiler` 由组件持有，接收原输出上下文，可以保存实例状态、返回错误或委托 `BuiltinComponentViewCompiler`。`set_view_compiler(None)` 恢复默认编译器。编译为同步操作；异步数据应先准备好再编译标记。

视图选择参数与模板数据分开，数据键 `view` 不会重定向编译，也不能覆盖编译器局部变量。自定义视图来源通过自定义编译器接入，不作为路径传给内置模板目录。

### 渲染组件标记 {#component-markup-rendering}

`render_view` 先使用组件当前编译器，再把实际 HTML 布局写入原输出。`render_view_with` 将详细级别和格式选项传给同一个输出。此层数据已经准备完成，不会再次执行具体组件的标点或高亮变换。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputFormat, view::components::Component};
use openrust::support::facades::Artisan;
use serde_json::json;
fn register_component_rendering() -> Result<(), ConsoleError> {
    Artisan::command_with("components:render", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.render_view("line", json!({
                "marginTop": 1, "bgColor": "blue", "fgColor": "white",
                "title": "info", "content": "Rendered <report>"
            }))?;
            component.render_view_with("bullet-list", json!({"elements": ["First", "Second"]}), OutputFormat::Plain)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

默认渲染器复用 HTML 解析器和现有输出格式器，支持四个内置模板、行内／块级／弹性布局、边距、内距、固定／完整／分数宽度、最小／最大宽度、对齐与大小写、固定预设颜色及色阶、粗体／斜体／下划线／删除线、链接及弹性重复填充。它还不是完整的终端 HTML/CSS 引擎：表格／代码／预格式／列表渲染、自定义样式仓库仍未支持。未支持的样式返回错误，不会静默丢弃布局。宽度和填充有安全上限，嵌套上限为 256 层。

`ComponentViewRenderer` 收到实际编译结果、`OutputOptions` 和原 `ComponentContext`。通过 `set_view_renderer(Some(Box::new(renderer)))` 安装组件拥有的渲染器，可保留实例状态、委托 `BuiltinComponentViewRenderer` 或返回错误；`set_view_renderer(None)` 恢复默认。自定义编译结果也沿同一路径，不按内置视图名称替换 HTML，不切换全局终端输出。编译失败不会调用渲染器；渲染失败保留已经发生的写入并向调用者传播。

后端按 HTML5 解析。任意畸形标记的恢复行为及 HTML4 特有兼容性尚未因内置模板测试而闭合。显式 `strong`、`italic`、`underline`、`line-through` 和 `invisible` 样式直接生成终端转义序列，在 Raw 输出或关闭装饰时也会保留；`font-bold` 则使用输出格式器。

### 组件颜色与断点 {#component-color-breakpoints}

终端渲染器支持固定预设调色板，包括 `text-amber-200`、`bg-slate-900`、基本颜色及 `text-brightgreen` 等亮色名称。名称与色阶按固定集合提供，不接受任意 CSS 颜色表达式。无效颜色在布局写入前返回错误。下面的 Raw 模式用于检查生成的格式器标签。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct PaletteCompiler;
impl ComponentViewCompiler for PaletteCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        if view == "palette" {
            return Ok("<span class='bg-slate-900 text-amber-200 px-1 sm:px-2'>Palette</span>".into());
        }
        BuiltinComponentViewCompiler::new().compile(view, data, output)
    }
}
fn register_component_colors() -> Result<(), ConsoleError> {
    Artisan::command_with("components:colors", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(PaletteCompiler)));
            component.render_view("palette", json!({}))?;
            component.render_view_with("palette", json!({}), OutputFormat::Raw)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

响应式前缀按终端列宽启用：`sm:64`、`md:76`、`lg:102`、`xl:128`、`2xl:153`。先应用无前缀类，再按断点升序应用已激活的前缀；未激活的前缀不解析对应样式，因此其中的未知样式不会触发错误。这里使用终端列，不是浏览器像素。

类名拆分保留方括号内的空白，例如 `content-repeat-[. ]`；不换行空格不是类名分隔符。原始类名会保留：带前缀的 `sm:flex-1` 不会生成用于判定弹性增长的无前缀 `flex-1` 标记。自定义样式仓库及其他待补的布局功能仍不属于当前支持子集。

### 组件尺寸与分布对齐 {#component-sizes-and-alignment}

使用 `w-1/2`、`w-2/3` 或 `w-full` 设置分数宽度，使用数值 `min-w-10`／`max-w-30` 设置限制。分数按继承宽度计算，并处理元素原始水平边距和内距。分母为零会返回错误，不会静默采用完整终端宽度。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct ColumnsCompiler;
impl ComponentViewCompiler for ColumnsCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "columns" => Ok("<div class='w-20 justify-between'><span>A</span><span>B</span></div>".into()),
            "half" => Ok("<span class='w-1/2'>Half</span>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_sizes() -> Result<(), ConsoleError> {
    Artisan::command_with("components:sizes", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(ColumnsCompiler)));
            component.render_view("columns", json!({}))?;
            component.render_view("half", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

`justify-between`、`justify-evenly`、`justify-around` 和 `justify-center` 在子节点间分配剩余空间。第一个示例固定容器为 20 列，在 A 与 B 之间放置 18 个空格；非 flex 容器也可使用这些规则。剩余空间不足时保留子内容，不额外添加分布填充。取整保留渲染器既定的浮点 floor／ceil 规则。

宽度参数保留数值或文本表示。分数与最小宽度混用时，先比较这些表示，再解析列数：`w-full min-w-2` 会选择两列。建议使用数值限制并验收组合布局，不把所有组合当作 CSS 计算后宽度比较。

弹性项目先渲染并缓存嵌套内容，再调整自身宽度、重新格式化该缓存。因此嵌套的完整宽度子节点，可能不同于先分配所有 flex 宽度再渲染后代的结果。每次调用使用独立的树和继承属性快照。无效布局沿原渲染器传播错误，不影响后续请求。完整样式方法分发及其他专用 HTML 渲染器仍未完成。

### 组件文本变换 {#component-text-transforms}

`snakecase` 在驼峰边界插入下划线，保留原有空白。`prepend-[X]` 和 `append-[Y]` 保留方括号：示例输出 `[X]mixed_words[Y]`。重复同一种变换只更新参数，不改变首次登记的执行位置。`truncate-4` 的四列限制包含默认 `…` 后缀，输出 `Mix…`；没有正数限制时使用元素宽度减去内距。分数宽度在截断前解析。

样式方法名按 ASCII 大小写不敏感分发：`W-1/2` 和 `FONT-BOLD` 可用。布局标记仍保留原始类名语义，例如修改 `flex-1` 大小写或加响应式前缀，不会创建该精确标记。数字类参数先转整数，再检查目标参数类型，所以 `prepend-1` 返回错误，不会静默转成文本。`font-normal` 明确关闭继承的粗体，`<b>` 标签则在类样式之后设置自身粗体。

默认文本管线不代表已提供渲染依赖的独立样式对象或自定义样式／颜色仓库。表格和代码等专用元素仍不属于已实现的 HTML 子集。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct TextCompiler;
impl ComponentViewCompiler for TextCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "text" => Ok("<span class='snakecase prepend-[X] append-[Y]'>MixedWords</span>".into()),
            "short" => Ok("<span class='truncate-4'>MixedWords</span>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_text() -> Result<(), ConsoleError> {
    Artisan::command_with("components:text", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(TextCompiler)));
            component.render_view("text", json!({}))?;
            component.render_view("short", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 列表、定义与分隔线 {#component-list-elements}

`ul`／`ol` 的直接子元素使用 `li`；文本节点保留，但不占编号。无序列表默认圆点，可用 `list-square`／`list-none`。有序列表只给实际编号项递增，所以示例输出 `1. First` 和 `2. Second`，跳过项不带编号。标记检查使用原始类名。父列表在构造时追加样式，会重新应用子项默认的 block；子项已有文本变换仍保留首次登记的执行顺序。

`dl` 接受 `dt` 和 `dd`，默认术语粗体、定义左边距四列。无效的直接子元素返回错误。`hr` 在宽度处理后生成边框：`w-12` 输出十二个线字符；内容宽度为零时，后续边框阶段仍可能填充继承宽度。`border-t` 的数字参数不设置粗细。表格与代码渲染器、自定义样式／颜色仓库仍是独立未完成能力。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct ListCompiler;
impl ComponentViewCompiler for ListCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "list" => Ok("<div><ul><li>Alpha</li><li>Beta</li></ul><ol><li>First</li><li class='list-none'>Skipped</li><li>Second</li></ol><dl><dt>Term</dt><dd>Definition</dd></dl><hr class='w-12'></div>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_lists() -> Result<(), ConsoleError> {
    Artisan::command_with("components:lists", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(ListCompiler)));
            component.render_view("list", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 预格式输出 {#component-preformatted-output}

`pre` 保留行内容，不折叠空白。最多移除首尾各一个空行，将其余行按最大 UTF-8 字节长度加三个空格补齐，再通过普通 div 样式逐行渲染。因此初始填充对多字节文本使用字节长度，而不是终端列宽。示例输出首个换行以及 `One   `／`Two   `。

内部标签作为原样标记保留，不递归注册成组件；最终格式器如何处理它们仍取决于输出选项。预格式结果不继承父样式，布局可测量其已渲染内容。空结果和隐藏结果仍是有效元素。未被 HTML 恢复移到列表外的直接预格式子元素，会被列表子类型检查拒绝。CRLF、注释、显式空属性和省略值的布尔属性保留既定序列化区别。通用异常 HTML 恢复仍是独立未完成能力。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct PreCompiler;
impl ComponentViewCompiler for PreCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "pre" => Ok("<pre>One\nTwo</pre>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_pre() -> Result<(), ConsoleError> {
    Artisan::command_with("components:pre", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(PreCompiler)));
            component.render_view("pre", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### HTML 表格输出 {#component-html-tables}

组件标记复用内置 Table 管线。`style` 选择命名表格样式；`thead`、`tbody` 和 `tfoot` 提供表头、正文、标题与表尾分隔行。单元格支持 `align`、`rowspan`、`colspan` 及既有文本／颜色类。表头单元格设置 strong，单元格内 `<br>` 转成换行。无效样式或对齐沿原机制返回错误。

表格先经过独立的彩色缓冲输出，再进入组件布局，所以外层关闭装饰时，内部 ANSI 输出仍可能保留。HTML 标题样式使用所选命名样式的本次副本，避免临时颜色修改长驻应用的后续请求；直接 Table API 的共享样式行为不变。自定义样式／颜色仓库及通用异常 HTML 恢复仍未完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct TableCompiler;
impl ComponentViewCompiler for TableCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "table" => Ok("<table style='box'><thead title='Report'><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>Alpha</td><td align='right'>12</td></tr></tbody></table>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_table() -> Result<(), ConsoleError> {
    Artisan::command_with("components:table", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(TableCompiler)));
            component.render_view("table", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 代码片段 {#component-code-snippets}

`code` 输出行号，并在 `line` 指定的绝对行放置标记；`start-line` 默认 1，最小为 1。非零标记即使超出片段范围，也会保留标记占位。首尾空行会修剪，CRLF 归一化；若内容有只包含空白的行，以该行确定缩进前缀，并从匹配的各行移除。代码节点的 `class` 不设置令牌主题。

固定片段契约使用 PHP 词法分类，包括短标签、字符串与注释。其他文本按内联文本显示；这不是 Rust 专用或通用多语言高亮器。词法处理完全运行于 Rust，无须 PHP 程序或运行时。令牌组、行号与标记复用组件样式和原输出格式器；部分 strong／italic ANSI 修饰在无装饰输出中仍会保留。极大行偏移保留整数／浮点文本规则，无法表示的行数组键返回错误。自定义样式／颜色仓库和其他独立终端扩展接口仍未完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError,
    view::components::{BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler}};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};
struct CodeCompiler;
impl ComponentViewCompiler for CodeCompiler {
    fn compile(&mut self, view: &str, data: Value, output: &mut ComponentContext<'_>) -> Result<String, ConsoleError> {
        match view {
            "code" => Ok("<code start-line='8' line='9'>first line\nsecond line</code>".into()),
            _ => BuiltinComponentViewCompiler::new().compile(view, data, output),
        }
    }
}
fn register_component_code() -> Result<(), ConsoleError> {
    Artisan::command_with("components:code", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(CodeCompiler)));
            component.render_view("code", json!({}))?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 时间与延迟辅助方法 {#component-time-helpers}

`InteractsWithTime` 提供 `seconds_until`、`available_at`、`parse_date_interval`、`current_time` 和 `run_time_for_humans`。Task 同时直接提供继承的四个延迟／日期方法；既有耗时覆盖通过 Trait 调用仍然生效。实现者选择可接收 `DateError` 的关联错误类型，保留命令既有的错误处理。

`TimeDelay` 接收 `i64`、`Date` 或 `DateInterval`。整数延迟保留正负；绝对日期的剩余秒数最低为零，可执行时间则保留原时间戳。日期区间相对于日期工厂当前值解析，保留月份及夏令时前后的本地日历天，不简单折算固定秒数；年／月／日分量共同归一化，再叠加固定时长分量。解析后返回日期或秒数，不改变原值。纪元前时间按向下取整的整秒处理。

默认可执行偏移显式传 `0`，因为 Rust 没有省略位置参数。`available_at` 的相对秒数使用日期工厂，不复用覆写的 `current_time`；只有绝对日期的 `seconds_until` 才减去该钩子。冻结时钟沿用应用／请求／任务作用域，耗时显示仍使用真实墙钟。日历区间超出既有原生日期范围返回受检错误。整数可执行时间戳保留有符号 64 位回绕规则；最小整数沿用日期修改表达式的回退路径。这些助手不代替队列调度，也不表示整个日期库已经完成。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, view::components::Task};
use openrust::support::{DateError, InteractsWithTime, facades::Artisan};
struct RetryClock;
impl InteractsWithTime for RetryClock { type Error = DateError; }
fn register_time_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("components:time", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let clock = RetryClock;
            assert_eq!(clock.seconds_until(-5)?, -5);
            let mut task = Task::new(output);
            let current = task.current_time()?;
            let available = task.available_at(60)?;
            assert!(available >= current + 60);
            let runtime = InteractsWithTime::run_time_for_humans(&mut task, 0.0, Some(0.1))?;
            drop(task);
            output.comment(&format!("delay=-5; runtime={runtime}"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 控制台辅助工具 {#console-helper-tools}

使用 `Helper` 计算显示宽度、字素数量和子串。`width` 采用固定控制台宽度表，连续控制字符只计一次。`length` 和 `substr` 使用 Unicode 17 字素边界。负起点从末尾计算，负长度从末尾裁掉对应数量，子串采用 NFC 规范化。字节入口同时接受非 UTF-8 数据，此时按字节计算宽度、长度和偏移。

`format_time` 默认精度为 1，`format_time_with` 接收显式精度。`format_memory` 格式化有符号字节数。`remove_decoration` 使用传入的格式器，去除 SGR 和终端超链接，成功后恢复原装饰设置。格式化失败时返回错误并保留关闭装饰的状态，遵循固定版助手的执行顺序。QuestionHelper 类型也直接提供这些继承工具。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, Helper, OutputFormatter, QuestionHelper};
use openrust::support::facades::Artisan;

fn register_helper_tools() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:measure", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let text = Some("界e\u{301}");
            let width = Helper::width(text);
            let length = QuestionHelper::length(text);
            let last = Helper::substr(text, -1, None);
            let elapsed = Helper::format_time_with(61.001, 2)?;
            let formatter = OutputFormatter::new(true);
            let label = Helper::remove_decoration(&formatter, Some("<info>Ready</info>"))?;
            output.comment(&format!("{label} {width}/{length} {last} {elapsed}"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 控制台助手集合 {#console-helper-sets}

`HelperSet` 按标准名称注册每个 `HelperInterface` 实例，也可以指定区分大小写的别名。替换同名条目时保留该名称的位置，已有别名仍可指向旧实例。`get_iterator` 获取调用时的注册顺序和实例引用。名称不存在时返回 `HelperNotFound`。批量注册带别名或无别名的条目可用 `from_helpers`。

自定义助手可用 `Helper` 保存关联。集合拥有注册实例，助手只持有最后一次关联集合的弱引用。手动改变实例关联时保留现有注册项。默认问答助手和组件问答助手都提供这一关联入口。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, DefaultQuestionHelper, HelperSet,
};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_helper_set() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:lookup", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let helpers = HelperSet::new();
            let question = Arc::new(DefaultQuestionHelper::new());
            helpers.set(question.clone(), Some("ask"));
            let resolved = helpers.get("ask").map_err(ConsoleError::from_source)?;
            assert!(question.get_helper_set().unwrap().same_set(&helpers));
            output.comment(&format!("{} has {} entries", resolved.get_name(), helpers.get_iterator().count()));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 格式化与调试助手 {#console-formatter-helpers}

`FormatterHelper` 为节标题和文本块生成带样式标记的文本。节标题默认使用 `info`。块会转义输入，并把各行补齐至相同宽度。`format_block_with` 可选择在上下增加空行的宽松样式。`truncate_with` 先比较显示宽度，再按字素切片并附加后缀。

`DebugFormatterHelper` 按会话 ID 生成起始行、输出与错误前缀及结果行。重新启动同一 ID 会重置输出流状态并轮换边框颜色。切换输出流后的第一行添加对应前缀。访问未知会话时返回错误。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, DebugFormatterHelper, FormatterHelper};
use openrust::support::facades::Artisan;
use openrust::console::{AsOutput, OutputFormat};

fn register_formatter_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("helpers:format-example", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let formatter = FormatterHelper::new();
            output.as_output().writeln(&formatter.format_section("check", "Ready"), OutputFormat::Normal)?;
            let debug = DebugFormatterHelper::new();
            output.as_output().writeln(&debug.start("report", "Running"), OutputFormat::Normal)?;
            output.as_output().writeln(&debug.progress("report", "Processing", false)?, OutputFormat::Normal)?;
            output.as_output().writeln(&debug.stop("report", "Done", true)?, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 进程助手 {#console-process-helper}

将 `ProcessHelper` 和 `DebugFormatterHelper` 加入同一个助手集合。以下示例需要能在可执行文件搜索路径中找到 Git。可以传入字面参数或已配置的 `PendingProcess`。

`run` 在进程非零退出时也会返回结果。`must_run` 在底层进程错误中保留该结果。默认的 `VeryVerbose` 级别显示命令及结果，`Debug` 还会显示实时输出。终端诊断写入 stderr，主动选中的分区或捕获缓冲保留原输出目标。使用 `run_with` 和 `must_run_with` 设置级别、失败提示和原始输出字节回调。独立包装回调时，先在已注册的调试助手上启动会话，再将 ID 交给 `wrap_callback`。关闭捕获的进程沿用其输出设置。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, DebugFormatterHelper, HelperSet, ProcessHelper,
};
use openrust::support::facades::Artisan;
use std::sync::Arc;

fn register_process_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:git-version", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let helpers = HelperSet::new();
            let process = Arc::new(ProcessHelper::new());
            helpers.set(Arc::new(DebugFormatterHelper::new()), None);
            helpers.set(process.clone(), None);
            let result = process.must_run(output, ["git", "--version"]).await?;
            output.comment(String::from_utf8_lossy(result.output()).trim());
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 应用与命令的助手集合 {#command-helper-sets}

每个控制台应用都会建立自己的助手集合，包含 `formatter`、`debug_formatter`、`process` 和 `question`。运行中的命令通过 `Input::get_helper` 或 `CommandIo::get_helper` 按名查询助手。以下示例需要能在可执行文件搜索路径中找到 Git。

注册时，`Artisan::get_helper_set` 返回当前应用的集合。`Artisan::set_helper_set` 更换后续命令继承的集合，已有命令保留其集合。使用 `Artisan::set_command_helper_set` 按名称或别名更换单个命令的集合。修改共享集合的内容会影响持有该集合的命令。查询无名助手时返回错误。

```rust
use openrust::console::{AsyncCommandCallback, ConsoleError, ProcessHelper};
use openrust::support::facades::Artisan;
use std::{any::Any, sync::Arc};

fn register_default_helpers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:helpers", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let helper: Arc<dyn Any + Send + Sync> = input.get_helper("process")?;
            let process = helper.downcast::<ProcessHelper>()
                .map_err(|_| ConsoleError::new("进程助手类型不匹配"))?;
            let result = process.must_run(output, ["git", "--version"]).await?;
            output.comment(String::from_utf8_lossy(result.output()).trim());
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 自定义终端样式与颜色 {#terminal-style-repository}

创建 `StyleRepository` 并交给内置渲染器。克隆后的句柄共享已注册样式，独立仓库可为同名样式配置不同行为。使用 `create` 或 `create_with` 注册自定义类。回调接收当前样式上下文，其后依次执行注册句柄上通过 `apply` 附加的样式类。

颜色使用 `#a855f7` 或 `#a5f` 等十六进制值。通过 `text-brand-500` 或 `bg-brand-500` 使用颜色时，注册完整键名 `brand-500`。自定义名称区分大小写，精确注册名称优先于内置处理，包括响应式前缀。重复注册替换该名称的条目，已有句柄继续持有原定义。 自定义样式的嵌套达到渲染深度上限时返回错误，循环引用也可安全退出。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{
        BuiltinComponentViewRenderer, ComponentContext, ComponentViewRenderer, StyleRepository,
    },
};
use openrust::support::facades::Artisan;

fn register_style_repository() -> Result<(), ConsoleError> {
    let styles = StyleRepository::new();
    styles.create("brand-500").color("#a855f7")?;
    styles
        .create_with("status", |element, _| {
            element.uppercase();
            Ok(())
        })
        .apply("font-bold text-brand-500");

    Artisan::command_with("tools:status-style", AsyncCommandCallback::new(move |_, _, output| {
        let styles = styles.clone();
        Box::pin(async move {
            BuiltinComponentViewRenderer::with_styles(styles).render(
                "<span class='status'>Ready for work</span>",
                OutputFormat::Normal.into(),
                &mut ComponentContext::Output(output.as_output()),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 类型化终端样式方法 {#typed-terminal-style-methods}

注册回调中的 `TerminalStyles` 提供颜色、间距、尺寸、文本变换和布局的类型化方法。整数宽度使用 `StyleArgument::Integer`，分数宽度使用 `StyleArgument::Text`。这些设置用于当前元素。

`set_style` 记录类名，供 `has_style` 查询。使用 `add_style` 解析并应用一组样式类。直接调用 `font_bold` 等方法只修改相应格式，不添加类名标记。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{
        BuiltinComponentViewRenderer, ComponentContext, ComponentViewRenderer, StyleArgument,
        StyleRepository,
    },
};
use openrust::support::facades::Artisan;

fn register_terminal_styles() -> Result<(), ConsoleError> {
    let styles = StyleRepository::new();
    styles.create_with("report-label", |style, _| {
        style.uppercase().font_bold();
        style.w(StyleArgument::Integer(24))?.px(1)?.text("cyan")?;
        Ok(())
    });

    Artisan::command_with("tools:report-label", AsyncCommandCallback::new(move |_, _, output| {
        let styles = styles.clone();
        Box::pin(async move {
            BuiltinComponentViewRenderer::with_styles(styles).render(
                "<span class='report-label'>Report ready</span>",
                OutputFormat::Normal.into(),
                &mut ComponentContext::Output(output.as_output()),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 终端元素 {#typed-terminal-elements}

用 `TerminalElement::new` 创建普通元素，或用 `from_styles` 解析默认和传入的样式类。元素可包含文本或有序子项。通过 `from_styles_with` 传入自定义样式仓库。使用 `configure` 可调用当前样式的类型化方法。

`set_content` 创建新元素，沿用同一份样式设置并使用自己的内容。用 `to_markup` 取得带样式标记的文本，或用 `render` 将其写入选定的输出。两个方法都会向调用方返回渲染错误。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalChild, TerminalElement, TerminalElementKind},
};
use openrust::support::facades::Artisan;

fn register_terminal_elements() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:terminal-element", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let prefix = TerminalElement::from_styles(TerminalElementKind::Span, "Status", "font-bold")?;
            let mut line = TerminalElement::from_styles(
                TerminalElementKind::Div,
                vec![TerminalChild::from(prefix), TerminalChild::from(" ready")],
                "text-cyan",
            )?;
            line.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 独立终端样式 {#standalone-terminal-styles}

构造独立的 `TerminalStyles` 可直接格式化文本。`with_default_styles` 保存默认类名，`add_style` 同时应用默认类和传入的类。`format` 返回带样式标记的文本。

`get_length` 忽略格式标记并测量显示列数。独立样式没有内容时，`get_inner_width` 从零减去左右边距。`get_parent_width` 按从外到内的顺序接收祖先元素的宽度约束。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::TerminalStyles,
};
use openrust::support::facades::Artisan;

fn register_standalone_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:standalone-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut styles = TerminalStyles::new().with_default_styles(["font-bold", "text-cyan"]);
            styles.add_style("ml-2")?;

            let message = "Available";
            let width = styles.get_length(Some(message));
            let formatted = styles.format(&format!("{message}: {width} columns"))?;

            output.as_output().writeln(&formatted, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 独立样式属性 {#terminal-style-properties}

`get_properties` 读取独立样式的当前属性。`with` 递归替换给定的键和数组位置。传入空数组会保留已有项。`set_properties` 替换整个属性映射，同时保留文本修饰器。

属性保留格式化使用的键，包括 `isFirstChild`、`colors`、`options` 和 `styles`。默认样式已包含 `isFirstChild`。替换整个属性映射后若要格式化文本，请同时提供这个键。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::TerminalStyles,
};
use openrust::support::facades::Artisan;
use serde_json::{Map, json};

fn register_style_properties() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:style-properties", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut styles = TerminalStyles::new();
            styles.with(Map::from_iter([
                ("styles".into(), json!({ "ml": 2 })),
                ("colors".into(), json!({ "fg": "blue" })),
            ]))?;
            styles.with(Map::from_iter([
                ("colors".into(), json!({ "bg": "white" })),
            ]))?;

            output.as_output().writeln(&styles.format("Properties ready")?, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 元素样式关联 {#terminal-style-attachment}

使用 `TerminalElement::with_styles` 将独立样式及其属性和默认类关联到元素。`get_properties`、`set_properties` 和 `with` 操作元素实际使用的样式。经 `set_content` 创建的元素共享该样式。

`get_length(Some(text))` 测量给定文本。`get_length(None)` 和 `get_inner_width` 使用最近关联的存活元素。渲染仍使用接收者自己的内容。最近关联的元素释放后，测量使用接收者自己的内容。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalElement, TerminalElementKind, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_attached_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:attached-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let style = TerminalStyles::new().with_default_styles(["text-cyan", "font-bold"]);
            let mut label = TerminalElement::with_styles(TerminalElementKind::Span, "Ready", style);
            label.configure(|styles| styles.add_style("ml-1").map(|_| ()))?;

            let mut next = label.set_content("Attached ready");
            let width = label.get_length(None)?;
            next.render(output, OutputFormat::Normal)?;
            output.comment(&format!("{width} columns"));
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 显式样式继承 {#terminal-style-inheritance}

`inherit_from_styles` 将祖先属性应用到独立样式或终端元素。子项保留已设置的颜色和粗体选项，缺少的值由父项补齐。

计算分数宽度时按祖先顺序使用其宽度、边距和内边距。父级间距作用于首项之后的元素，首项保留自身边距。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{StyleArgument, TerminalElement, TerminalElementKind, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_inherited_styles() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:inherited-style", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut parent = TerminalStyles::new();
            parent.text("cyan")?.w(StyleArgument::Integer(24))?.px(1)?;
            parent.font_bold();

            let mut element = TerminalElement::new(TerminalElementKind::Span, "Inherited ready");
            element.inherit_from_styles(&parent)?;
            element.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 自定义文本与样式修饰器 {#terminal-style-modifiers}

`TerminalStyleModifiers` 保存两组有序回调。用 `set_text` 注册文本回调，回调在计算宽度前收到当前文本、样式属性和祖先属性。用 `set_style` 注册的回调在封装输出标记前收到调整后的文本和样式属性。

通过 `TerminalStyles::with_modifiers` 关联回调。同组回调重名时替换原回调并保留位置。每个阶段均先执行自定义回调，再执行内置变换。回调返回错误时，格式化会停止并将错误返回调用方。

```rust
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalStyleModifiers, TerminalStyles},
};
use openrust::support::facades::Artisan;

fn register_style_modifiers() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:style-modifiers", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut modifiers = TerminalStyleModifiers::new();
            modifiers.set_text("prefix", |value, _, _| Ok(format!("Result: {value}")));
            modifiers.set_style("punctuate", |value, _| Ok(format!("{value}!")));

            let mut style = TerminalStyles::new().with_modifiers(modifiers);
            style.text("green")?;
            let text = style.format("ready")?;
            output.as_output().writeln(&text, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 按名称调用元素方法 {#terminal-element-dispatch}

用 `TerminalElement::call` 传入方法名称和 JSON 参数。`get_length` 等查询返回 `TerminalElementResult::Value`。修改方法和未知方法返回同一元素的借用，后续调用可以读到更新后的样式。

参数无效或调用私有方法时会返回 `ConsoleError`。传入 Rust 样式对象时，使用类型化的 `TerminalElement::with_styles` 构造入口。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError, OutputFormat,
    view::components::{TerminalElementKind, TerminalElementResult, TerminalStyles, TerminalElement},
};
use openrust::support::facades::Artisan;
use serde_json::json;

fn register_element_dispatch() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:element-dispatch", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut element = TerminalElement::with_styles(
                TerminalElementKind::Span, "ready", TerminalStyles::new(),
            );
            element.call("prepend", &[json!("Result: ")])?;
            if let TerminalElementResult::Value(length) = element.call("get_length", &[json!("ready")])? {
                assert_eq!(length, json!(5));
            }
            element.render(output, OutputFormat::Normal)?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 终端 HTML 片段恢复 {#terminal-html-recovery}

内置终端视图渲染器按 HTML4 规则恢复片段。自闭合的样式 span 保持为空，后续文本保留自己的样式。框架也处理错配标签和标准 HTML4 实体引用。

含标记的片段需要有可渲染的正文首子节点。片段准备失败时，渲染器向命令返回 `ConsoleError`。

```rust
use openrust::console::{
    AsyncCommandCallback, ConsoleError,
    view::components::{Component, ComponentContext, ComponentViewCompiler},
};
use openrust::support::facades::Artisan;
use serde_json::{Value, json};

struct Markup;
impl ComponentViewCompiler for Markup {
    fn compile(
        &mut self, html: &str, _: Value, _: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError> {
        Ok(html.to_owned())
    }
}

fn register_html_recovery() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:html-recovery", AsyncCommandCallback::new(|_, _, output| {
        Box::pin(async move {
            let mut component = Component::new(output);
            component.set_view_compiler(Some(Box::new(Markup)));
            component.render_view(
                r#"<div><span class="text-red"/>Result: ready</div>"#, json!({}),
            )?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```

### 绑定命令组件工厂 {#command-component-factory}

在应用容器中绑定 `dyn CommandComponentFactory`，通常放在服务提供者里。Artisan 每次执行时解析构造器，并传入已解析的输入和输出流借用。命令内取得的各个组件句柄共享返回的 `ComponentDispatch`。

用 `handles_call` 选择接管的动态调用，用 `display_line` 自定义同步的 `info`、`success`、`warn` 和 `error` 方法。命令之后更换普通输出时，组件回调仍使用原来的输出上下文。构造失败会在命令正文执行前返回给调用方。

```rust
use std::sync::Arc;
use openrust::console::{
    AsOutput, AsyncCommandCallback, ConsoleError, ConsoleFuture, Input, OutputOptions,
    OutputReference,
    view::components::{CommandComponentFactory, ComponentArgument, ComponentContext, ComponentDispatch},
};
use openrust::contracts::{container::Container, foundation::Application as _};
use openrust::foundation::Application;
use openrust::support::facades::Artisan;
use serde_json::{Value, json};

struct Labels;

impl CommandComponentFactory for Labels {
    fn make(
        &self, _: &Input, _: OutputReference<'_>,
    ) -> Result<Arc<dyn ComponentDispatch>, ConsoleError> {
        Ok(Arc::new(Self))
    }
}

impl ComponentDispatch for Labels {
    fn handles_call(&self, name: &str) -> bool {
        name == "label"
    }
    fn call<'a>(
        &'a self, _: &'a str, _: Vec<ComponentArgument<'a>>, _: ComponentContext<'a>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(async { Ok(json!("ready")) })
    }
    fn display_line(
        &self, name: &str, text: &str, options: OutputOptions, mut output: ComponentContext<'_>,
    ) -> Option<Result<(), ConsoleError>> {
        (name == "info").then(|| output.as_output().writeln(&format!("Result: {text}"), options))
    }
}

fn bind_component_factory(app: &Application) -> Result<(), ConsoleError> {
    app.container().instance::<dyn CommandComponentFactory>(Arc::new(Labels))
        .map_err(ConsoleError::from_source)?;
    Ok(())
}

fn register_component_factory() -> Result<(), ConsoleError> {
    Artisan::command_with("tools:component-factory", AsyncCommandCallback::new(|_, input, output| {
        Box::pin(async move {
            let mut io = input.io(output);
            let label = io.components().call("label", vec![]).await?;
            io.output_components().info(label.as_str().unwrap_or_default())?;
            Ok(0)
        })
    }))?;
    Ok(())
}
```
