# Pail 实时命令

## 固定对照与复用

承接 `48da4f3e6`，对应 06-08 和固定 Laravel 日志文档的 [Pail 日志跟踪](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail)。Pail v1.2.5 固定 `fdb73f5eacf03db576c710d5a00101ba185f2254`，逐项核对 [PailCommand](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Console/Commands/PailCommand.php)、ProcessFactory、File、Options 和 PailServiceProvider。

检查旧版 `openrust-v0.x` `6d1a81924`，无 Pail 或实时日志命令；旧 console 的终端判据作为复核线索。复用当前日志事件捕获、文件与 Options、CliPrinter、Provider 命令回调、ConsoleKernel 和原生关停信号。入口仍遵循固定 Goravel，不改 main、bootstrap、Runner、Worker 或业务路由。

## 实现和适配

- `openrust_pail::console::commands::PailCommand` 保留 pail、说明、filter/message/level/auth/user、timeout=3600；筛选复用已对照固定 PHP 的 Options，auth 优先于 user。零关闭执行超时，负数返回错误。详细级别和颜色沿用 Console 输出作用域。
- Provider 在现有 testing／Vapor／PAIL_TESTS 门禁后登记所属应用命令回调，ConsoleKernel 引导时执行。Rust 统一进程不复制 PHP 全局 SAPI，HTTP-only boot 只保存回调，独立和程序化 ConsoleKernel 均可使用。
- 每次命令以独占创建的随机文件名拥有一个会话，保留目录 .gitignore。正常超时和信号退出成功；解析、读取、写出失败传播错误。作用域清理器覆盖异常返回、Future 取消和展开，只清理本次会话路径，保留其他会话。SIGKILL／崩溃无法运行清理，继续由现有陈旧会话规则回收。
- `ProcessFactory::run` 用原生异步循环和阻塞池读取替代 PHP Process／PCNTL／tail 子进程。每次至多读取 8 KiB，文件共享锁采用非阻塞尝试；未获得锁时让出，超时和取消仍可处理。保留初始末十行、完整行解析、分段 UTF-8、按名称跟随轮换及检测截断。读取分代边界丢弃半行，避免拼接不同文件内容；这是明确的原生适配，不能宣称该损坏输入边界与 GNU tail 的字节流完全相同。
- 独立终端调用接框架 Ctrl+C／SIGTERM，受管调用接 Input 的现有取消令牌，捕获调用由调用方控制 Future／令牌。超时显示 Maximum execution time exceeded.。没有新增外部命令或 shell 参数拼接。
- `Output` 的详细级别和抑制状态用 RAII 恢复，覆盖取消中的程序化命令。现有 Input shutdown 和 Output streams_to_terminal 只开放包接线所需的只读访问。
- 保留上游 TTL：没有心跳，即使 timeout=0，超过一小时未写入的会话也可能在下一次生产日志时删除。无限跟随不会擅自重建被删除的会话。
- Windows 复用既有关停信号路径，本批不宣称 Windows 实机通过。stdout 满管道的同步写阻塞仍沿当前输出契约，不能承诺任意输出背压下的即时关停。

## 验收

Ubuntu 原生 amd64，Rust 1.94.0 Debug，隔离快照从 `48da4f3e6` 导出并只覆盖本批源码。实际验收：

- `cargo test --locked -p openrust-pail` 通过。新增实际命令夹具覆盖真实 HTTP→独立 Artisan、双应用和三会话、筛选、帮助、testing／PAIL_TESTS 门禁、零超时、负超时、正常超时、quiet／silent、SIGINT／SIGTERM、无效 JSON、关闭输出管道及会话清理。原生读取覆盖初始末十行、跨读取 UTF-8、截断、轮换和取消令牌。程序化 ConsoleKernel 通过 Provider 注册运行，并验证 Future 取消后文件及输出状态恢复。
- 既有 4 项文件回归（含子进程入口）、6 项 Options／消息用例、432 组固定 PHP 打印语料、22 组真实 Provider HTTP／Artisan 矩阵和捕获回归通过。未提供 MySQL URL，Worker 用例明确跳过；本批未改 Worker，不将旧验收算作本批数据库验收。
- `cargo test --locked --test console --test feature --test artisan_entry` 的 23 项根回归通过。
- 三语日志及 Artisan 正文、站点构建、根路径和 `/docs/` 部署路径的 12 项浏览器检查通过。

- `cargo clippy --workspace --all-targets --features pail --locked -- -D warnings`、`cargo build --locked --features pail` 和 `cargo +1.94.0 fmt --all --check` 通过。
- 根应用真实二进制自动发现 Pail，list／help／timeout 通过。4 组真实 PTY 验证默认颜色、no-ansi、quiet、silent，按文档的 timeout=0／level=error／message=payment／-vv 组合打印并以键盘 Ctrl+C 退出 0，清理会话；Linux `/proc` 确认没有额外子进程。
- 根实际二进制 HTTP GET /up 返回 200，inspire、项目原始 `./artisan inspire`、SIGINT 退出 0 和端口释放通过。Pail 保持可选编译功能：文档的实时命令从带 `--features pail` 编译的二进制进入 Artisan，未修改默认 Cargo wrapper 或擅自启用该功能。
- 10 份候选源码与隔离验收副本 SHA-256 一致。私有证据位于 `/private/tmp/openrust-pail-command-20261119`；命令夹具、进程验收、Clippy、根二进制及文档记录分别保存。

首次开发编译修正根路径方法接线与测试返回类型，原生读取夹具补齐真实来源字段；最终整包和入口验收均使用修正后的源码通过。

## 最小提交范围

```text
Cargo.lock
vendor/openrust/framework/src/console/{application,mod}.rs
vendor/openrust/pail/
├── Cargo.toml
├── src/lib.rs
├── src/process_factory.rs
├── src/console/mod.rs
├── src/console/commands/{mod,pail_command}.rs
└── tests/command.rs
docs/{en,zh_CN,zh_TW}/
├── the-basics/logging.md
└── digging-deeper/artisan.md
.agents/project/
├── audits/06-08/{README,pail,pail-command}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 保留边界

认证自动解析见 [专项边界](pail-auth-boundary.md)。自动 dev 注册按固定 Framework 的 [DevCommands::registerDefaults](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/DevCommands.php) 单独收尾：Pail Provider 已加载时登记 logs → pail --timeout=0，保持默认优先级、过滤和用户覆盖，不将它误写成 vendor 优先级。Windows 发布验收及 [Blade 标量算术](../07-01/storage-consumers.md)继续跟踪。核心账本的既有 66/66 不等于 Pail 或框架全量闭合。

## 资源与提交

已回收约 6.05 GB 本批远程源码／编译／依赖缓存及站点产物，Docker 数据卷清单一致，保留共享 Rust 镜像和文档依赖。提交前磁盘可用 55.16 GB（51.37 GiB），周额度剩余 14%，第二窗口无读数；沿用用户忽略本轮额度限制的授权，磁盘规则生效。未使用额度重置、启用 credits 或推送。

本批提交号以本验收记录所属 Git 提交为准；未跟踪备份、core 和 translation-database 继续保留，不纳入提交。
