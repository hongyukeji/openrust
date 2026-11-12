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

`Application::handleCommand` 调用 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引导后加载待实现命令清单，再注册应用命令并分派。


## 源码与发布入口

源码 `artisan` 脚本先进入应用目录，再将参数原样交给 Cargo，保留空格、空参数和非 ASCII 文本。脚本返回命令的退出码。

独立构建使用 `./main artisan list`、`./main artisan help inspire` 或 `./main artisan inspire`，无需 Cargo。配置相对于可执行文件解析，从其他工作目录启动也可运行。参阅[部署](../getting-started/deployment.md)。

编译入口不带模式参数时启动 HTTP，使用 `artisan` 时执行控制台命令。其他首参数会在创建应用前显示外层用法并返回退出码 2。Artisan 分派错误返回 1。外层分流是 Rust 单二进制的语言适配；Laravel 使用独立的 HTTP 和 `artisan` 入口文件。Console Kernel 的 handle/terminate 顺序仍由 `Application::handleCommand` 负责。

命令清单同时包含待实现条目。名称和帮助用于保留预定契约，不代表对应模块已经完成。`artisan.bat` 尚未通过 Windows 实机验收。

## 命令帮助和输出格式 {#command-help-and-output-formats}

`./artisan help migrate` 与 `./artisan migrate --help` 从同一命令定义读取参数、选项和说明。查看帮助时不会执行目标命令。精确的命令名称和别名优先，可按完整名称或冒号分隔的各段使用唯一缩写。

```sh
./artisan list cache --format=json --short
./artisan help migrate --format=json
./artisan help migrate --format=xml
./artisan help migrate --format=md
```

`list` 和 `help` 支持 `txt`、`json`、`xml` 和 `md`。`--raw` 输出平铺列表或紧凑文本帮助。`list --short` 保留名称、别名和实现状态，省略命令明细。各语言的结构化输出使用相同的命令和选项标识。帮助和列表成功时退出码为 0。参数错误、歧义和未知命令的退出码为 1。

`--quiet` 隐藏普通输出。`--silent` 同时隐藏控制台错误。命令定义也可使用全局 `--no-interaction`、`--ansi`、`--no-ansi`、`--version` 和详细程度选项。

语言选择与覆盖见 [Artisan 翻译](./localization.md#artisan-translations)。

## 按需安装路由 {#optional-route-installers}

在源码目录运行以下命令，生成路由文件并更新模块声明和启动配置，然后重新构建。已有路由文件会保留。

```sh
./artisan install:broadcasting
./artisan install:grpc
```

应用骨架已包含 `routes/api.rs`。旧版源码可用 `./artisan install:api` 补充 API 路由文件和启动接线，已有路由会保留。广播接入后会注册 `/broadcasting/auth`，授权需要广播驱动和频道规则。gRPC 需要注册 Tonic 服务，并设置 `GRPC_HOST` 和 `GRPC_PORT` 才开启监听。

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
