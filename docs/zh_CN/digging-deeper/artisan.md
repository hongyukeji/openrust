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

名言库与固定版 Laravel 一致，共 41 条。支持颜色的终端会将正文显示为粗体、署名显示为灰色。重定向时默认输出纯文本。可以用 `--ansi` 强制开启样式，或用 `--no-ansi` 关闭。

Windows 上使用 `artisan.bat`。编译后的应用通过 `main artisan` 接收相同的命令。

列表和帮助成功时返回退出码 0。未知命令和非法参数返回 1。标记为 `Planned` 的条目可以查看帮助，执行时返回 1。

通过 `withCommands` 接入 `app/console/commands/mod.rs` 中的命令类型。在 `bootstrap/app.rs` 的 `withRouting` 中声明 `routes/console.rs` 的控制台路由。应用命令可以替换同名占位项。

`routes/console.rs` 导入 `openrust::support::facades::Artisan`，通过 `Artisan::command("inspire", …)` 向 `register()` 执行期间绑定的控制台注册闭包，通过 `purpose()` 设置帮助和命令列表中的说明。闭包接收命令输出对象，正常返回时退出码为 0。

`Application::handleCommand` 调用 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引导后加载延迟 Provider，再登记命令清单、框架内置命令和应用命令，最后分派。


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

运行满一秒后失败的进程每隔一秒重试，最多五次。成功退出的进程结束，一秒内失败则立即报告。用 `--no-restart` 关闭本次重试，或在应用里用 `DevCommands::disableAutoRestart()` 关闭。用 `DevCommands::withTimestamps()` 默认开启时间戳。第一个永久失败决定最终退出码，其他进程可继续运行。

Unix 上的可用终端默认进入标签视图，也可用 `--stream` 直接进入合并视图。重定向输出、尺寸不足的终端、Windows、`--inline` 和 `--json` 走内联输出路径。交互会话正常退出时返回 0，内联和 JSON 模式则报告第一个永久失败的退出码。

按 `1`–`9` 选择标签，也可用 Tab 聚焦侧栏后用方向键选择。按 `s` 切到流式视图、`t` 返回标签、`f` 筛选流、`r` 重启当前标签的进程、`c` 清空当前视图。用 `/` 搜索，再按 `n` 或 `N` 切换匹配。用方向键或 Page Up、Page Down 滚动日志。

`--buffer-size` 和 `--stream-buffer-size` 控制每个标签和流式视图保留的行数，分别默认 2,000 和 10,000 行。应用可用 `DevCommands::bufferSize()`、`streamBufferSize()` 配置，并用 `tabs()`、`stream()` 或 `inline()` 设置默认模式。

按 `q` 或 Ctrl+C 退出。Unix 终止信号也会请求关停，最多等子进程退出六秒。Artisan 恢复终端，并把保留的流式日志打印到对应的输出流。可选 [Pail 命令](../the-basics/logging.md#tailing-log-messages-using-pail)也可独立运行。

在 Windows 控制台按 Ctrl+C 请求关停。每条开发命令都运行在自己的 Windows 作业中。Artisan 向该命令的控制台进程组发送 Ctrl+Break，并在宽限结束后回收仍在运行的后代进程。启用 `--no-restart` 时，一条命令失败也会关停其他开发命令并返回失败码。

### 查看开发进程

用 `dev:list` 查看当前应用登记的开发进程。默认列出 [HTTP 开发服务](#development-http-server)和队列监听器。应用含有 `package.json` 时，也会按锁文件选择 Bun、pnpm、Yarn 或 npm 并列出 Vite。查询不会启动进程。加载可选 Pail Provider 后，会在 Vite 之前添加 `logs`，执行 `pail --timeout=0`。它属于框架默认进程，排除 vendor 不会移除它，`withoutDefaultCommands()` 会移除它。包或应用可以用同名 `logs` 条目覆盖。

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

Shell 命令用 `register` 登记，包管理器命令用 `node` 或 `nodeExec`。名称传 `None` 时使用命令首词。用 `only`、`except`、`order`、`withoutVendorCommands` 和 `withoutDefaultCommands` 调整应用清单。框架依据 Rust 调用者的源码路径识别包登记。

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

每个位置参数或选项单独传一个 `OsString`。含空格的值放在同一个参数中。布尔开关为真时传入开关选项，多值选项可重复传入。用 `Artisan::callSilently(name, arguments).await` 丢弃两个输出流并保留退出码。

### 将 Artisan 命令加入队列 {#queueing-artisan-commands}

用 `Artisan::queue` 准备命令。调用 `dispatch().await` 提交任务并取得派发结果。用 `onConnection` 和 `onQueue` 指定目标，也可以使用默认连接及其默认队列。

```rust
use openrust::{
    queue::{DispatchResult, QueueResult},
    support::facades::Artisan,
};

async fn queue_inspire() -> QueueResult<DispatchResult> {
    Artisan::queue("inspire", &[])
        .onConnection("redis")
        .onQueue("commands")
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

在源码项目运行 `./artisan init`，Windows 使用 `artisan.bat init`。向导读取已有值，引导应用、数据库、缓存、Session 和队列设置，密码隐藏输入，连接检查通过后保存。无交互模式读取项目环境。当前初始化支持 SQLite、MySQL／MariaDB 和 PostgreSQL，以及本地／数据库／Redis 服务和日志／SMTP 邮件和内置短信供应商。API 使用 Sanctum，同时安装频道和 gRPC 注册。Reverb 包自动安装，是否随主应用启动由 REVERB_ENABLED 控制；仅生成缺失的 Reverb 凭据，保留已有值。Passport 安装仍待对应安装器接通。

```sh
./artisan init --dry-run
./artisan init
./artisan init --no-interaction
./artisan init --extensions=api --no-interaction
```

使用 `--extensions=api,broadcasting,reverb,grpc` 选择扩展（也是默认集合），交互向导会询问同一逗号分隔清单。`api` 安装 Sanctum；`reverb` 自动选择广播频道。未选择的扩展保持原样；后续运行可补装缺失扩展，保留已有配置和密钥。已配置 Reverb 时须将它纳入选择。`--extensions=all` 检查完整发布计划，目前会在写文件前失败：Passport、Fortify、Passkeys、Socialite、Precognition、OIDC RP 和 SSO 仍需完成实现及共享安装器。错误会列出前置缺口；所选配置初始化完成不代表全部发布范围就绪。

缺少 `.env` 时从示例创建，保留已有密钥和凭据；缺少应用密钥时调用 `key:generate`。复用单项路由安装器，统一重建后调用原迁移命令。生产环境仍需确认或 `--force`。初始化成功后执行 `cargo run`，命令不自动启动长期服务。`--without-migration` 只报告准备完成，迁移仍待执行。`--force` 允许覆盖已有 API 脚手架并执行生产迁移，不重置数据库或轮换密钥。`--dry-run` 不连接、不写文件、不构建、不迁移。失败返回非零并保留已完成内容供重试，并发源码安装立即报告冲突。

选择 SMTP 后复用现有传输层检查连接、TLS 和认证，不发送测试邮件。向导收集内置短信供应商凭据；发送身份和业务模板仍按各供应商配置，不自动发短信，也不声称真实投递通过。启用 Reverb 时引导监听及发布地址，默认同进程运行；已有配置始终作为向导默认值。

使用 `--env=acceptance` 初始化 `.env.acceptance`；清理配置、生成密钥和迁移步骤使用同一环境，不修改 `.env`。

## 生成命令 {#generating-commands}

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
