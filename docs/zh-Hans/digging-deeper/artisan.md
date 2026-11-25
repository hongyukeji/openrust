# Artisan 命令行

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

通过 `with_commands` 接入 `app/console/commands/mod.rs` 中的命令类型。在 `bootstrap/app.rs` 的 `with_routing` 中声明 `routes/console.rs` 的控制台路由。应用命令可以替换同名占位项。

`routes/console.rs` 导入 `openrust::support::facades::Artisan`，通过 `Artisan::command("inspire", …)` 向 `register()` 执行期间绑定的控制台注册闭包，通过 `purpose()` 设置帮助和命令列表中的说明。闭包接收命令输出对象，正常返回时退出码为 0。

`Application::handle_command` 调用 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引导后加载延迟 Provider，再登记命令清单、框架内置命令和应用命令，最后分派。


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

在应用服务提供者的 `boot` 方法中，通过 `openrust::foundation::DevCommands` 登记进程。同名进程会覆盖默认项，应用登记优先于包登记。

```rust
DevCommands::artisan("serve --port=9000", Some("server"))?.green();
```

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

每个位置参数或选项单独传一个 `OsString`。含空格的值放在同一个参数中。布尔开关为真时传入开关选项，多值选项可重复传入。用 `Artisan::call_silently(name, arguments).await` 丢弃两个输出流并保留退出码。

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

## 生成命令 {#generating-commands}

破坏性命令可调用 `openrust::console::confirm_to_proceed(production, input.matches())?`，仅在返回 `true` 时继续。`production` 由应用环境或命令确认条件决定。复用的多语提示默认拒绝，非交互输入拒绝；命令声明的 `--force` 可跳过确认。

在源码检出目录生成命令。生成的类型实现 Artisan 命令契约。

```sh
./artisan make:command SendReport
./artisan make:command Reports/ArchiveReport --command=reports:archive
```

第一条命令写入 `app/console/commands/send_report.rs`，命令签名为 `app:send-report`。第二条在 `reports` 模块下生成命令并显式指定签名。使用 `-f` 或 `--force` 覆盖已有文件。

在 `app/console/commands/mod.rs` 声明新模块并注册命令。以 `SendReport` 为例，添加 `mod send_report;`，在 `register` 内调用 `artisan.add_command(Box::new(send_report::SendReport::default()))?;`。重新构建后运行 `./artisan app:send-report`。

在应用根目录放置 `stubs/console.rs.stub` 可自定义生成内容。生成器替换 `{{class}}` 和 `{{command}}`。

### 生成配套测试 {#generating-an-accompanying-test}

添加 `--test` 可在生成命令时同时生成一个功能测试。

```sh
./artisan make:command SendReport --test
```

测试写入 `tests/feature/console/commands/send_report_test.rs`，嵌套命令保留原有子目录。按[测试接线](../testing/getting-started.md#creating-tests)声明模块后，用 Cargo 运行。模板沿用 `make:test` 的健康检查，可在生成的测试中加入命令断言。

`--force` 允许替换两个生成文件。命令生成后若测试生成失败，Artisan 以状态码 1 退出并保留命令文件。解决报错后，运行 `./artisan make:test Console/Commands/SendReportTest` 单独生成测试。

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
            io.output_components().info("Saved");
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

直接渲染时使用 `Table::new(io.get_output())?`，设置表头、数据行和列样式后调用 `render()`。`TableCell::display(value)` 显式适配实现 `Display` 的业务类型；JSON 值和常用标量直接转换。多行单元格、Unicode 可见宽度、格式标签、ANSI 输出及 quiet／silent 过滤沿用原输出管线。此矩形表格接口不代表已支持跨行／跨列或整个继承辅助库。

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

宽度保留有符号值，非正数按渲染规则处理，不静默替换为默认值。最大宽度为零不换行；不兼容的有效宽度可能在此前已有输出后才于渲染期失败，应传播 `render` 错误。换行复用当前 `WrappableOutputFormatterInterface`，也支持自定义实现。标题按可见宽度和格式居中、截断。命名可变注册表、复杂行与合并单元格仍为单独的 SDK 入口。

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

`set_style_named` 和 `set_column_style_named` 按名称解析，未定义名称返回错误；原类型 setter 也可接收借用样式。便利方法 `table` 使用应用当前默认定义，`table_named` 和 `table_with_named_columns` 支持命名表格与列样式。显式分隔单元格、跨行跨列和分区重绘仍为独立能力。

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

`set_rows`、`add_rows`、`add_row` 和表格便利方法接收这类异构行。`add_row`／`add_rows` 继续用 `?` 传播索引耗尽；`set_row` 可将同键分隔线替换为数据数组。开头、末尾和连续分隔线保留声明顺序。横向与纵向布局略过显式整行分隔线，纵向布局仍自行生成记录间分隔。复杂分隔单元格选项、跨行跨列和分区重绘仍为独立能力。

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

将 `TableSeparator` 转换为单元格后，按该列样式绘制内部边框，与整行 `TableRow::Separator` 不同。跨行跨列、分隔构造选项和分区重绘仍单独跟踪。

每条实际输出行在开始时读取行文本格式。格式化回调的修改会影响后续边框与下一行，不会回改正在渲染的本行单元格格式。
