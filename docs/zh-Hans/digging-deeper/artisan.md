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

实现 `PromptsForMissingInput`，注册前用 `.prompting()` 保留该 Trait。必填参数按声明顺序收集，随后执行最终验证与隔离。已传参数不重复询问；`--no-interaction` 跳过询问，但仍拒绝缺失的必填参数。

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
