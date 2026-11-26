# Pail CLI 打印器

## 固定对照与复用

承接 `0f7ad08fc`，对应 06-08 与固定 Laravel 日志文档的 [Pail 输出](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail)。Pail 固定 v1.2.5 `fdb73f5eacf03db576c710d5a00101ba185f2254`，按 [CliPrinter](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Printers/CliPrinter.php)、Contracts/Printer、MessageLogged、来源对象及 tests/Unit/CliPrinterTest.php 核对。实际比较前，6 个参考 PHP 文件与固定提交的 GitHub 原始文件 SHA-256 一致。

检查旧版 `openrust-v0.x` `6d1a81924`，未找到 Pail／CliPrinter；旧 console/command.rs 的终端判据只作为复核线索。复用当前已验收的消息对象、来源、Options、HTTP 捕获夹具、Artisan 分发与 Output、console 库及 Str::width。入口装配仍沿用固定 Goravel，本批不改 main、bootstrap、Runner 或应用路由。

## 实现与 Rust 适配

- 正式路径为 openrust_pail::contracts::Printer 和 printers::CliPrinter。保持 new(output, basePath)、print、optionsHtml、traceHtml 与对应私有 helper 名称。print 返回 Result<(), ConsoleError>，借用当前命令的可变 Output；不将该借用输出作为容器全局单例。
- 普通模式显示时间，缩略深层类型与路径，并按上游先比较字节长度再取 Unicode 子串。-v 显示完整日期、类型、路径和正文；-vv／-vvv 显示来源堆栈。边框最多 150 列，消息与长底部信息的溢出规则保留上游行为。
- 底部合并 HTTP 方法／路径、认证身份或命令／队列／任务及公开 Context，保留字符串键覆盖顺序、数字键重排、数字键 null 丢弃、PHP var_export 风格标量／数组、引号和 HTML 转义。零字符串消息显示 No message.；Context 字符串的 NUL 经终端文本渲染转为空格。
- 直接构造终端布局与 ANSI，省去 PHP HTML／DOM 层；公开 HTML helper 的内容与参考字节摘要一致。保留 Termwind 的 ASCII 样式标记／空白规则、多行 pre 字节填充及非断行／全角空白，避免误处理中文标签。不据此声明未经测量的速度提升。
- Rust 的 :: 类型命名空间参与缩略；Windows 原生分隔符适配到相对路径显示。PAIL_TESTS 仍控制捕获测试，打印日期、文件和堆栈保持记录本身，不复制 PHP 单元测试中伪造的 MyClass.php、时间与堆栈。
- 沿用 Rust JSON 契约保留 u64 精度。实际 PHP 8.4.14 对 18446744073709551615 解码为 float，并显示 1.8446744073709552E+19；Rust 打印原整数。该语言适配在三语正文说明并由独立原生用例验证，不将它混入 PHP 等价样本。
- Output 增加 isDecorated、isVerbose、isVeryVerbose 与 writeRaw。分发时读取可用参数，兼容直接解析而未附全局选项的 Input；嵌套输出详细级别／抑制状态按作用域保存、继承和恢复。writeRaw 写已渲染文本，真实命令立即写出／刷新，捕获模式写缓冲区，quiet／silent 抑制它，I/O 错误经 Result 传播。
- 保留上游 MIT 版权通知。console、regex 及仅测试使用的 sha2 均复用现有锁定版本，没有引入 PHP 运行时依赖。

## 实际验收

Ubuntu 原生 amd64、Rust 1.94.0 Debug，隔离快照从 `0f7ad08fc` 导出，仅覆盖核定源码。

- 使用真实 PHP 8.4.14、Illuminate Support 13.40.0、Termwind 2.3.3、Symfony Console 7.3.6 与 phpdotenv 5.6.2 运行固定 PHP 文件，不使用渲染 stub。432 组样本覆盖 20／26／50／80／150／200 列与三档详细级别，包含空／0、多行、长文本、HTML、深层路径／类、来源覆盖、认证零值、标量／数组／浮点／NUL 和中文简繁体、阿拉伯文、泰文、法文及特殊空白。每组比较显示文字与 optionsHtml／traceHtml 的 SHA-256，共 1,296 项；显示文字统一去行尾空白。432 组全通过。
- 正文 fixture 保存去重输入与摘要；tests/reference/printer.php 重新生成后逐字节一致。原始参考输出和 Composer 完整锁定清单保留在本批私有证据目录，未重复收录参考 vendor 或构建产物。
- 新增错误日期／无效来源在写入前返回错误、Rust 命名空间缩略、空对象和 u64 精度验证。
- `cargo test --locked -p openrust-pail` 通过：4 项文件生命周期／四进程追加、6 项 Options、Provider 环境矩阵与应用路径、捕获及打印器自定义验收程序。真实双应用 HTTP 捕获后，由独立 Artisan 的验收命令读取并打印，校验所属方法／路径／账户／邮箱及 normal／-v／-vv／ansi／quiet／silent，防止应用串用。
- 根 console、feature、artisan_entry 共 23 项通过，包含直接 Input 调用、命令嵌套与现有参数行为。工作区格式和差异检查通过。
- 全工作区全目标、启用 pail 的严格 Clippy 通过。最终多语种修正后再次通过包回归、432 组、全目标严格 Clippy 和真实终端验收，退出 0。
- 验收夹具实际二进制使用框架 run_application：7 组真实 Unix PTY 校验默认颜色、no-ansi、quiet、silent、-v、-vv、-vvv，正文与对应 PHP 样本一致；关闭输出管道后写入错误传播并退出 1，无挂起。验收命令是测试夹具，不是声称 realtime pail 已注册。
- 三语日志正文、站点构建和根路径／docs 子路径的 12 项浏览器检查通过。13 个候选文件与最终验收副本 SHA-256 一致。

本批未改 Worker，也未提供 MySQL URL；Worker 可选用例明确跳过，未计作实际数据库验收。首次回归后的容器缺少临时安装的 Clippy 组件已补齐并重跑。开发中发现的布局和夹具数值类型保留问题均在最终样本通过前修正。

## 最小文件范围

```text
Cargo.lock
vendor/openrust-framework/src/console/{application,mod}.rs
vendor/openrust-pail/
├── Cargo.toml
├── LICENSE.md
├── src/lib.rs
├── src/contracts/mod.rs
├── src/printers/{mod,cli_printer}.rs
├── tests/{capture,printer}.rs
├── tests/fixtures/printer.json
└── tests/reference/printer.php
docs/{en,zh_CN,zh_TW}/the-basics/logging.md
.agents/project/
├── audits/06-08/{README,pail,pail-printer}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

## 后续与资源

打印器接口、实现与本批验收闭合。下一批接实时 pail 命令、会话文件生命周期、timeout／信号清理、程序化调用边界及 dev 注册。[认证自动解析](pail-auth-boundary.md)、Windows 发布矩阵和 [Blade 标量算术缺口](../07-01/storage-consumers.md)继续跟踪。核心账本维持 66/66 的既有范围，不等同于 Pail 或整个框架完成。

私有证据位于 `/private/tmp/openrust-pail-printer.ka1uxypj`。已回收约 5.48 GB 当批远程源码／构建／PHP 参考缓存、此前不存在且无人使用的匹配摘要 Composer 镜像及 60.63 MB 站点产物；数据卷前后一致，保留共享 Rust 镜像与文档依赖。提交前项目磁盘可用 55.15 GB（51.36 GiB），周额度剩余 14%，第二窗口无读数。沿用用户忽略本轮额度限制的授权，磁盘停线继续生效；未使用额度重置、启用 credits 或推送。
