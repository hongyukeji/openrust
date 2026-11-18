# 06-08 Artisan 帮助、命令文档与本地化

依赖 00-03 Artisan 入口和 03-10 翻译器。对照固定 [Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md)、[Symfony HelpCommand](https://github.com/symfony/console/blob/v7.4.0/Command/HelpCommand.php)、[描述器测试](https://github.com/symfony/console/blob/v7.4.0/Tests/Descriptor/JsonDescriptorTest.php) 与 [Goravel Command](https://github.com/goravel/framework/blob/v1.18.0/contracts/console/command.go)。复核旧版 `tests/unit/console/command_help.rs`，沿用两种帮助入口读取同一注册对象且跳过执行的回归。

帮助与列表共用 Clap 命令元数据，支持文本、JSON、XML 和 Markdown。命令名和别名优先精确匹配，支持唯一分段缩写。应用使用 `console` 翻译分组覆盖界面文本，并复用翻译器回退。语言不改变命令、选项和 JSON 键。结构化输出保留 OpenRust 现有 `implemented`、`aliases` 和单个 `usage` 字符串，短输出通过别名字段保留别名。字段组织与 Symfony 描述器有差异。框架接受全局详细程度参数，命令可从输入读取级别。Clap 仍负责解析及英文错误正文。既有跨命令别名冲突拒绝和显式 `--env` 值规则继续有效。

## 验收

- Rust 1.94.0，`cargo fmt --all --check`、锁定离线严格 Clippy 和并行工作区测试通过。490 项通过，3 个子进程入口由集成测试启动。已通过独立提交 `aab1ff4d` 修复调度夹具目录碰撞，并行全量含 Redis 调度验收。
- 实际 Artisan 进程验证英文、简体及繁体 `list` 和 `help`，中文参数错误退出 1，应用语言文件回退、别名及缩写、安静和静默输出、版本和成功退出 0。
- 从三语文档抽取七条公共命令实际执行，解析 JSON 和 XML 并检查 Markdown。三语站点构建、12 项通用浏览器检查及 6 项 Artisan 到本地化小节导航通过。

下一单元 07-01 Helpers 与 Strings，前置 02-08 已提交。

## Artisan 完整纯文本帮助

- Symfony Console v7.4.0 的 `HelpCommand` 将 `--raw` 作为文本描述器的样式选项。详细帮助、参数及选项仍走原命令描述。复核旧版 `tests/unit/console/command_help.rs`，复用两种入口读取注册定义的回归思路。OpenRust 统一渲染 Clap 长帮助，保留应用注册的长简介和参数详细说明。普通文本帮助尊重 ANSI 选择并翻译标题，`--raw` 强制纯文本。JSON、XML、Markdown 延续既有字段与布局，保持纯文本内容。 [固定 Symfony HelpCommand](https://github.com/symfony/console/blob/v7.4.0/Command/HelpCommand.php#L57) · [固定文本描述器](https://github.com/symfony/console/blob/v7.4.0/Descriptor/TextDescriptor.php#L111)
- 隔离快照 `b6cb04500` 的 17 项 `console`、`feature`、`artisan_entry` 回归通过。实际 `./artisan` 验证中英文三语标题、两种帮助入口、ANSI 与纯文本内容等价、结构化输出和未知命令退出 1。注册命令回归带必填参数和别名，验证帮助退出 0 且保留详细说明。
- Rust 1.94.0 全仓格式及全目标严格 Clippy、三语站点构建与根路径和 `/docs/` 的 18 项浏览器检查通过。已回收本批隔离编译和站点产物。项目卷可用 89.04 GB（82.93 GiB），提交前周额度剩余 27%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-artisan-raw-help.tyEkCgMR`。
- 提交前本地 Git 合并了独立短信文档。核对隔离快照与新基线 `49b40cf74` 的差异仅涉及文档，随后在新基线复跑三语构建和 18 项浏览器检查，全部通过。

## Artisan 文本列表的全局选项

- 固定 Symfony Console v7.4.0 在普通命令列表及指定命令组的列表中，先显示应用全局选项，再显示命令目录。复核旧版 `console/command.rs` 的 `ListCommand`，复用从实际注册命令生成目录的思路。OpenRust 的文本列表从 Clap 根命令渲染全局选项，复用帮助描述与翻译键，避免另外维护一份选项清单。ANSI 输出遵循终端选择，平铺 `--raw` 仍只输出命令。 [Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#introduction) · [固定 Symfony 文本描述器](https://github.com/symfony/console/blob/v7.4.0/Descriptor/TextDescriptor.php#L142)
- 隔离快照 `43530b1ec` 的 18 项 `console`、`feature`、`artisan_entry` 通过。实际 `./artisan` 用英、简中、繁中验证默认入口、普通列表、命令组、全局选项、ANSI 和纯文本等价、`--raw` 及未知命令组退出 1。另解析 JSON、XML 和 Markdown，验证结构化命令目录继续可用。
- Rust 1.94.0 全仓格式及工作区全目标严格 Clippy 通过。英、简中、繁中 Artisan 文档已同步，站点构建与根路径及 `/docs/` 下的 18 项浏览器检查通过。临时编译与站点产物已回收。磁盘可用 89.01 GB（82.90 GiB），提交前周额度剩余 27%，第二窗口无读数。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-artisan-list-options.PkJJh0aT`。

## 三语 Artisan 入口与 Windows 验收对齐

- 对照固定 Laravel 的独立 Artisan 命令入口及 Goravel 的 Runner 篇章，复核 v0.x 由源码脚本转交 CLI 的做法。当前编译入口在无参数时调用 `Application::start` 运行符合配置的 Runner。显式 `artisan` 模式进入独立的控制台内核，启动服务与控制台分派分别由各自入口负责。同步三语说明并链接部署章节。帮助保留 `Planned` 条目的可识别状态，内核引导后加载延迟 Provider、命令清单、内置及应用命令。 [固定 Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#introduction) · [固定 Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- 核对主线 `7d6de4678` 与 Windows ARM64 实测快照 `baac80470` 的 `artisan`、`artisan.bat`、`main.rs` 和框架分流源码，内容相同。沿用 CMD `inspire`、PowerShell JSON 帮助成功退出 0、未知命令和无效格式退出 1，以及真实 `/up` 的 Windows 证据。补充 CMD 和 PowerShell 的退出码读取方式及独立发布入口。 [Windows 源码启动器复验](../00-03/README.md#windows-arm64-源码启动器复验)
- 本批只修改正文及记录。三语站点构建通过，根路径和 `/docs/` 下 12 项通用浏览器检查及 6 项入口、退出码提示和部署跳转检查通过。临时站点产物已回收。提交前项目盘可用 52.34 GB（48.75 GiB），周额度剩余 18%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-artisan-entry-docs.cxPFMLkq`。

## Artisan 门面程序化调用

- 对照固定 Laravel 的程序化调用、参数解析和静默执行。复核 v0.x 的 Artisan 启动脚本及入口测试，复用主线 Console Kernel、Clap 根命令和现有命令分派。当前应用的弱引用绑定让门面在 HTTP 或 Artisan 生命周期内取得正确应用。参数逐项使用 Rust `OsString`，返回命令退出码，输出保存在调用方的 `Output`，因此并发调用各自读取自己的输出。未知命令和非法参数以 `ConsoleError` 返回。 `callSilently` 使用缓冲输出并丢弃两路内容。队列派发入口另批核对。 [固定 Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#programmatically-executing-commands) · [固定 Console Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Application.php)
- 在 `f6b7bb7c4` 隔离归档上叠加候选源码，Ubuntu x86_64 的 Rust 1.94.0 容器验证 26 项 `console`、`deferred_lifecycle`、`artisan_entry` 测试。真实 TCP 并发请求分别执行两个应用的同名命令。嵌套命令验证别名、含空格和中文的参数、非零退出码、静默和帮助，解析三个语言版本共同的 `help inspire --format=json` 示例输出。实际应用二进制的 `artisan inspire` 退出 0，未知命令退出 1。全仓格式与锁定离线的工作区全目标严格 Clippy 通过，源文件 SHA-256 与候选一致。
- 三语正文及示例同步。三语构建、根路径和 `/docs/` 下 12 项通用浏览器检查及 6 项新章节检查通过。编译、容器、镜像和站点产物已回收，Docker 卷清单一致。提交前项目盘可用 52.20 GB（48.62 GiB），周额度剩余 18%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-artisan-call.CW7ghMzf`。

## Artisan 命令队列

- 对照固定 Laravel 的 `Kernel::queue`、`QueuedCommand::handle` 与 `displayName`。核对旧版 Worker 真实队列回归，沿用主线应用级 Bus、编译注册和版本化 JSON。 `Artisan::queue` 返回可选择连接和队列的待派发对象，Rust 使用显式 `dispatch().await` 交付任务。参数以 UTF-8 保存，拒绝非 UTF-8 数据。Worker 解析自身应用内的命令并丢弃输出。正常返回的非零状态照常确认，执行错误遵循 Worker 重试和失败政策。Worker 和失败清单展示命令名，派发仍使用固定处理器类型。 [Laravel Queueing Artisan Commands](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#queueing-artisan-commands) · [固定 QueuedCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/QueuedCommand.php)
- 基线 `b3f1ce016` 的隔离归档叠加候选。Ubuntu x86_64 与 Rust 1.94.0 通过 26 项 `console`、`deferred_lifecycle`、`artisan_entry` 测试，覆盖 sync 完成、执行错误和 Unix 非 UTF-8 参数拒绝。全仓格式、锁定离线工作区全目标严格 Clippy、实际二进制的 `artisan inspire` 通过。候选源文件 SHA-256 与验收一致。
- 实际应用二进制加入临时 HTTP 与命令夹具。通过项目 MySQL 验收入口启动 MySQL 8.4.13，并单独启动 Redis 8.10.3。实际 HTTP 入队到默认 MySQL 连接及明确指定的 Redis 连接和 `commands` 队列，独立 `queue:work` 验证其他队列保持为空、带空格和中文的参数、空字符串、重复选项、正常返回 7 的任务确认。未知命令首次释放，第二次执行写入失败清单，后续 `inspire` 在两种后端均消费成功。应用 SIGTERM 退出 0。宿主运行容器编译出的验收二进制时显式设置 `APP_BASE_PATH`。
- 英、简中、繁中正文和示例同步。三语构建及根路径和 `/docs/` 下 18 项浏览器检查通过。临时镜像、容器、编译及站点产物已回收，Docker 卷清单一致。提交前项目盘可用 52.12 GB（48.54 GiB），周额度剩余 18%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-artisan-queue.jGf6gMPv`。

## Artisan 开发 HTTP 服务

- 对照固定 Laravel `ServeCommand`，复核 v0.x 的 `http/serve.rs`。沿用独立命令只运行 HTTP 的职责，复用现有 HTTP 内核的请求处理和关停。支持 `SERVER_HOST`、`SERVER_PORT`、`--host`、`--port`、`--tries` 和 `--no-reload`。默认监听 `127.0.0.1:8000`，只有隐式端口遇到地址占用时递增尝试。 [固定 Laravel ServeCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ServeCommand.php)
- 默认监控选中的环境文件。采用同一 Rust 二进制运行 HTTP 子进程，等待旧进程退出后在已选端口重新启动。禁用重载或启动时没有环境文件时直接运行 HTTP。普通 Runner 启动与独立 Artisan 分流保持原有职责。Rust 源码改动需重新构建。Windows 新增子进程及 Ctrl+C 路径仍待实机复验。
- 基线 `71db48089` 的隔离源码使用 Rust 1.94.0 构建。Ubuntu 实际二进制验证 HTTP 首页和 `/up`、独立 Artisan 帮助和 `inspire`、显式端口占用与重试耗尽退出 1、隐式端口回退、命令行覆盖环境值、`--no-reload`、`--quiet`、`--silent`、选定环境文件重载后保留端口，以及 SIGTERM 和两种模式下 Ctrl+C 退出 0。27 项定向回归、全仓格式、锁定离线全目标严格 Clippy、三语站点构建及 18 项浏览器检查通过。
- 本批临时构建和站点产物已回收。项目磁盘可用 52.08 GB（48.50 GiB），提交前周额度剩余 18%，第二窗口无读数，按用户授权继续。验收日志保留于 `/private/tmp/openrust-artisan-serve.k3BIeZYm`。

## Artisan 开发进程注册与列表

- 对照固定 Laravel 的注册入口、列表命令及源码回归。复核 `v0.x`（`6d1a81924`）的 Artisan 入口，旧版没有开发进程登记或列表可直接移植。沿用 Goravel 将应用装配和命令执行分开的职责。 [Laravel DevCommands](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/DevCommands.php) · [Laravel DevListCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/DevListCommand.php) · [Laravel 回归测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationDevCommandsTest.php) · [固定 Artisan 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#the-dev-command) · [固定 Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- 注册表存放在应用容器中，支持同名覆盖、默认／包／应用优先级、来源和颜色、顺序和包含／排除规则。Rust 用调用者的源码路径识别包和行号，按应用根目录检测 Node 锁文件。默认列出 `serve` 和 `queue:listen`，应用有 Node 清单文件时加入 Vite。后续接通 `dev` 的并发执行、重启与终端交互。Pail 独立跟踪。
- 基线 `2ab1cb7a4` 的隔离快照使用 Rust 1.94.0。锁定离线 `cargo test --test console --test artisan_entry --test dev_commands` 共 11 项通过，覆盖真实 HTTP 入口回归、两应用并发注册并查询、包优先级、颜色、Vite 包管理器、过滤与退出码。实际二进制在 Ubuntu 通过交互式 ANSI／纯文本及无匹配退出 1，三语共 9 条 `./artisan` 指令通过。格式、全目标严格 Clippy、三语站点和 18 项浏览器检查通过。
- 已回收本批编译和站点产物。项目磁盘可用 52.34 GB（48.75 GiB），提交前周额度剩余 17%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-dev-commands.YyKYNyK0`。

## Artisan dev 并发执行与关停

- 对照固定 Laravel `DevCommand` 和 Artisan 章节，结合 Multiplex 的事件和重启契约。复核旧版 `v0.x` `6d1a81924` 的 Artisan 及启动源码，没有可直接提取的 `dev` 监督器。参考固定 Goravel 的应用启动与命令分工，开发进程继续使用独立 Artisan 入口。 [固定 Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/DevCommand.php) · [固定 Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#the-dev-command) · [Multiplex 事件和重启语义](https://github.com/laravel/multiplex#json-output) · [固定 Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- `dev` 并发运行 `dev:list` 所用注册表，同一二进制运行 `serve`、`queue:listen` 及通过 `DevCommands::artisan` 登记的 Artisan 命令。保留自定义 Shell、Node、选定环境和应用根目录。内联保留 stdout／stderr，机器输出使用版本 1 的 JSON 行。失败重试最多五次，一秒内失败立即告知。Unix 复用子进程组，并为 HTTP 和队列监听器的关停链保留最多六秒，关停时等待读取子进程退出钩子输出，到时强制结束进程组并关闭输出读取。UTC 时间戳及 Rust 原生监督器取代 PHP 外部 Multiplex 的启动方式。交互标签、流式导航、Windows 进程树回收与原生验收及 Pail 仍待推进。
- 基线 `237c8d812` 隔离构建使用 Rust 1.94.0。锁定离线 `cargo test --test dev_commands --test artisan_entry --test console` 共 13 项通过，框架 `processes` 另有 13 项通过，覆盖真实 HTTP、测试 Vite 子进程延迟失败重试、快速失败、SIGTERM、Provider 禁用重试及输出。三语共 9 条真实 `./artisan dev` 示例通过，使用 SQLite 数据库迁移后的 `queue:listen`、真实 `GET /up`、受控资产脚本、Ctrl+C、进程退出及端口释放。独立二进制另验证初次启动加五次重试、永久失败退出码 19，以及忽略 SIGTERM 的进程在宽限结束后被强制回收。格式、锁定离线全工作区全目标严格 Clippy 和 18 项浏览器检查通过。
- 已回收本批编译和站点产物。项目磁盘可用 52.34 GB（48.75 GiB），提交前周额度剩余 17%，第二窗口无读数，按用户授权继续。验收日志保留于 `/private/tmp/openrust-dev-run.NTjtHHe1`。

## Artisan dev 标签与流式终端

- 对照固定 Laravel 的 `--tabs`、`--stream`、`--inline`、缓冲配置和非交互回退，以及 Multiplex 的按键和退出语义。复核 `v0.x` `6d1a81924` 的 Artisan 与测试，没有可直接提取的交互终端。沿用 Goravel 启动与控制台职责划分，复用当前独立 Artisan 监督器。 [固定 Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/DevCommand.php) · [固定 Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#the-dev-command) · [Multiplex 键盘操作](https://github.com/laravel/multiplex#keyboard-shortcuts) · [固定 Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- Rust Unix 设备拥有非阻塞终端描述符，开启备用屏幕并在退出时恢复终端属性。共用进程事件实现标签、合并流、搜索、筛选、滚动、清屏及手动重启，回放保留的日志与原时间戳。默认各标签保留 2,000 行，合并流 10,000 行。交互文本去除控制序列，每行最多保留 4,096 个字符。重定向、尺寸不足的终端和 Windows 走内联输出。窗口标题、桌面通知和 Multiplex 原有视觉细节未纳入本批。Windows 进程树回收与实机验收、Pail 仍待推进。
- 基线 `094df4d7e`，Rust 1.94.0。锁定离线 `cargo test --test dev_commands --test artisan_entry --test console` 共 13 项通过，覆盖强制 JSON 优先、重试、退出钩子和 HTTP。真实伪终端运行 `python3 -B tests/artisan_dev_terminal.py`，验证 SQLite 队列监听装配、`GET /up`、搜索换项、筛选、手动重启、清屏、缩放、小终端回退、`q` 和 Ctrl+C、日志回放、终端属性逐项恢复及端口释放。三语 9 条 `./artisan` 交互样例、格式、全目标严格 Clippy 和 18 项浏览器检查通过。
- 已回收本批编译和站点产物。项目磁盘可用 52.76 GB（49.13 GiB），提交前周额度剩余 17%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-dev-tui.UxrvvmzW`。

## Artisan dev Windows 进程树与 Ctrl+C

- 固定 Laravel Windows 入口由独立控制台进程组监督进程，并在禁用重试时遇失败关停其他进程。复核 `v0.x` `6d1a81924` 的 Windows 命令，沿用现主线已验收的 Worker Job Object，提取共享的作业创建、门禁和回收。Windows Artisan 子命令收到分配确认后执行，外部 Shell 命令通过同一二进制进入作业后启动。Ctrl+C 由父进程接收，给各控制台组转发 Ctrl+Break，剩余后代由内核作业回收。独立 Worker 识别 Windows CRT 的 SIGBREAK 21。 [固定 Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/DevCommand.php) · [固定 Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#the-dev-command) · [Microsoft 控制台进程组](https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent) · [Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) · [固定 Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)
- 基线 `2c4cdec9e`，Parallels Windows 11 Build 26200，Rust 1.94.0 和 ARM64 MSVC，二进制 PE Machine `0xaa64`。真实 `python -B tests/artisan_dev_windows.py` 使用独立 SQLite 迁移和队列监听、真实 `GET /up`、资产脚本及拥有独立控制台的孙进程，验证 Ctrl+C 退出 0、正常 HTTP 停止、后代进程退出和端口释放，以及 `--no-restart` 的失败退出 9 并结束其他命令。另实测无控制台强制回收及独立 `queue:work` Ctrl+Break 产生 `interrupted`、退出 0。
- Windows 锁定离线 `dev_commands`、`console` 9 项通过。Windows 工作区库和二进制及 `dev_commands`、`console`、`queue_hard_timeout` 的严格 Clippy 通过。完整 `queue_hard_timeout` SQLite 进程夹具在当前改动和同工具链的未改基线均触发 `STATUS_STACK_OVERFLOW`，缩小到既有失败回调模式仍复现。Windows 全目标 Clippy 报 `faker_color.rs:13` 的 `parse` 类型推断错误，未改基线的 Faker 夹具也复现。两项完整回归仍待收口，x64 未覆盖。Ubuntu 13 项、真实伪终端及全目标严格 Clippy 通过。格式、三语站点构建及 18 项浏览器检查通过。
- Windows 验收目录和临时产物、Ubuntu 编译缓存、站点产物已回收，Windows 虚拟机恢复关机。项目磁盘可用 56.88 GB（52.98 GiB），提交前周额度剩余 17%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-dev-windows.b2hPXJEI`。

## Windows 回归门禁收口

06-04 的 [Windows ARM64 Debug 作用域与堆上状态](../06-04/worker-hard-timeout.md#windows-arm64-debug-作用域与堆上状态)已通过原生真实 Worker 的定向及完整 SQLite 进程验收。Faker RGB 的显式通道类型先独立提交于 `6796629d9`。全目标门禁随后暴露的 Process `Duration` 和本地文件系统 `WriteOptions` 只被 Unix 测试使用，导入随测试限定到 Unix。Pail 日志跟踪继续独立推进。

- 原生 Windows ARM64 执行 `cargo +1.94.0 clippy --locked --offline --workspace --all-targets -- -D warnings` 退出 0，随后 `cargo +1.94.0 test --locked --offline -p openrust-framework --test processes --test faker_color --test filesystem_local` 的 10 项测试通过。Ubuntu 同范围 24 项、完整 Worker 进程验收及全目标严格 Clippy 通过。
- 本批文件范围如下，Faker 修复已先独立提交。

```text
.agents/project/
├── audits/
│   ├── 06-04/worker-hard-timeout.md
│   ├── 06-08/README.md
│   └── core-closure-ledger.md
├── current.md
└── progress.md
docs/
├── en/digging-deeper/queues.md
├── zh_CN/digging-deeper/queues.md
└── zh_TW/digging-deeper/queues.md
tests/queue_hard_timeout.rs
vendor/openrust/framework/
├── src/queue/worker.rs
└── tests/
    ├── filesystem_local.rs
    └── processes.rs
```

- Windows 与 Ubuntu 当批快照及编译缓存、站点产物已回收，数据卷清单一致，Windows 虚拟机恢复关机。提交前磁盘可用 56.59 GB（52.71 GiB），周额度剩余 16%，第二窗口无读数，沿用用户忽略本轮额度停线的授权。原始验收日志保留于 `/private/tmp/openrust-win-baseline.em1FHFXM`。

## Pail 日志监听前置

已核定 [Pail 专项对照与顺序](pail.md)，并完成 [同步日志监听](../01-04/log-listeners.md)。跨渠道通知、原载荷、回调顺序和错误、重入及双应用隔离通过真实 HTTP 与独立 Artisan 验收。Pail 跨进程捕获、命令和开发进程注册继续依次推进。

## Pail 会话捕获

[专项验收](pail-capture.md)完成独立可选包、现有包发现、活跃会话 JSON 行、跨进程锁、过期与删除、HTTP／命令／队列来源和公开 Context。真实双应用 HTTP、独立 Artisan、并发命令及同步任务、MySQL 8.4.13 Worker、失败回调、后续消费和隔离 deferred 回调通过。日志／认证／根 Console 定向回归、严格 Clippy、三语站点与 12 项浏览器检查通过。Rust 同步日志只读取已解析认证身份，自动解析差异继续跟踪；`pail` 命令和 dev 注册尚未实现。

## Pail 消息对象与筛选

[专项验收](pail-options.md)完成 Options、MessageLogged 和三类来源对象。固定 PHP 8.4.27 执行未改上游源码，210 组筛选、21 组 timeout、3 条 JSON 序列化结果与 Rust 对照通过。6 项定向测试、文件回归、真实双应用 HTTP→独立 Artisan 筛选、全目标严格 Clippy、三语正文与浏览器检查通过。本批未执行 MySQL Worker，不沿用旧结果冒充本批验收。[认证解析边界](pail-auth-boundary.md)、Provider 环境门禁、CliPrinter、实际会话命令和 dev 注册继续跟踪。

## Pail Provider 环境门禁与延迟解析

[专项验收](pail-provider.md)接通 testing／Vapor 跳过、PAIL_TESTS 例外、应用 Environment 快照、Application 及其契约的 runningUnitTests，并按固定上游延迟解析 Files／Handler。22 组真实 HTTP／Artisan 矩阵、并发双应用、进程覆盖、--env、配置缓存、零解析与错误传播、延迟构造失败、MySQL 8.4.13 Worker、根二进制自动发现与启停通过。22 项日志／Context、24 项根回归、Pail 回归、全目标严格 Clippy、三语配置／日志正文与浏览器检查通过。原入口和环境模板未修改。统一存储路径 API、认证自动解析、命令模式边界、打印器和会话命令继续跟踪。


## Pail 统一存储路径

[专项验收](../07-01/storage-path.md)接通 Application／契约的 storagePath、useStoragePath、作用域 storage_path helper 和 Files 所属容器解析。默认、自定义及双应用目录通过真实 HTTP、独立 Artisan、MySQL Worker、隔离回调、严格 Clippy、三语正文与浏览器检查。路径 API 已验收，其他存储消费者迁移继续跟踪，再接打印器和会话命令。


## 存储路径消费者收口

[配置与文件消费者验收](../07-01/storage-consumers.md)接通日志、Session、缓存、视图、local/public 磁盘和维护模式；环境快照以弱引用传递所属应用路径上下文，不读取其他应用的 Facade。55 项定向测试函数、严格 Clippy、实际根二进制 HTTP／Artisan、缓存前后路径、维护模式和三语文档验收通过。Pail 路径依赖收口，下一批接打印器与实时命令；认证自动解析和发布矩阵继续跟踪。


## Pail CLI 打印器

[专项验收](pail-printer.md)接通 Printer／CliPrinter、Output 详细级别与原始流式写入。432 组固定 PHP 对照、真实 HTTP→独立 Artisan、PTY 颜色／详细级别／静默、关闭管道退出 1、23 项根回归、严格 Clippy、三语正文和浏览器检查通过。Rust 保留实际来源和 u64 精度，相关适配已说明。实时命令、会话生命周期、认证自动解析与发布矩阵继续跟踪。
