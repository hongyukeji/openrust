# Pail 消息对象与筛选

## 对照与范围

承接 `9b7b35779` 的[会话捕获](pail-capture.md)，实现固定 Pail v1.2.5 的 Options、MessageLogged 和 Console／Http／Queue 来源对象。固定提交 `fdb73f5eacf03db576c710d5a00101ba185f2254`；对应 Laravel 日志文档的 Pail 小节。核对同版 Filters/FilterTest、LevelTest、MessageTest 和 Origins 测试。旧版 `openrust-v0.x` `6d1a81924` 无对应 Pail 代码；复用主线包发现、同步日志通知、捕获文件和真实 HTTP／Artisan 夹具。固定 Goravel 的编译型应用装配保持原样，不增加入口或服务。

[Options](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Options.php) · [MessageLogged](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/ValueObjects/MessageLogged.php) · [来源对象](https://github.com/laravel/pail/tree/fdb73f5eacf03db576c710d5a00101ba185f2254/src/ValueObjects/Origin)

- 公开 `openrust_pail::Options`、`value_objects::MessageLogged` 和 `value_objects::origin::{Console, Http, Queue}`；保持 `fromCommand`、`fromJson`、`fromArray`、`classOrType`、`authId` 及对应字段名。
- auth 精确字符串匹配、level 忽略 ASCII 大小写、filter 搜索四字段 JSON、message 只搜索消息；所有条件同时成立。认证 ID 数字转换为字符串，空值与前导零保持区别。auth 优先于 user，显式空值也不回退。
- 四字段 JSON 保留对象键顺序、Unicode UTF-16 转义、未转义斜杠和 PHP 浮点表示；避免通用 filter 因语言 JSON 格式差异改变结果。框架已经启用 serde_json preserve_order，本包显式声明该需求，没有更改工作区既有 Map 顺序机制。
- timeout 保留 PHP 的整数转换：空值、非数字、数字前缀、正负号、小数、指数、溢出与非有限值。只读取其值；实际进程超时校验由后续运行命令负责，不提前改变 Options 契约。
- 日期和时间保留记录中的 UTC 偏移时区；origin 解码、日期转换和 JSON 解析以 Rust `Result` 表达失败。Rust 命令与解析输入分离，因此 `fromCommand` 接收 `&Input`。
- `context()` 保留普通键，排除 `__pail`、`exception` 和 `userId`。捕获时的异常与 Context 策略不变。

## PHP 独立对照

本机 PHP 7.4 缺失既有 tidy 动态库，未修改系统安装。改用本批专属的 `php:8.4-cli-bookworm`，实际 PHP 8.4.27。从固定提交逐文件读取原始 PHP 源码，执行未改的 Options 和 MessageLogged。测试脚本只替代 `Illuminate\Console\Command::option` 的输入读取，不加载 Laravel 应用；不将此结果宣称为 Laravel 整体运行验收。

`tests/fixtures/pail_options_oracle.php` 生成 210 组筛选、21 组 timeout 转换及 3 条完整序列化结果。固定语料为 `tests/fixtures/pail-options.json`。重新执行生成器后用 `cmp` 核对字节一致。Rust 测试逐条比对原 PHP 结果，覆盖数字／字符串／访客身份、大小写、空字符串、中文、代理对、对象键顺序、浮点和别名规则。测试运行本身不依赖 PHP。

生成器用法（`/ref` 挂载固定 Pail 源码目录）：

```sh
docker run --rm \
  -v "$PAIL_REFERENCE:/ref:ro" \
  -v "$PWD/vendor/openrust/pail/tests/fixtures/pail_options_oracle.php:/oracle.php:ro" \
  php:8.4-cli-bookworm php /oracle.php
```

## 实际验收

隔离源码以 `9b7b35779` 为基线，只叠加本批源码，Ubuntu 原生 amd64、Rust 1.94.0、Debug 构建。

- `cargo test --locked -p openrust-pail --test options`：6 项通过，含全部 PHP 对照语料、来源解码、日期、缺失和无效输入、组合筛选、别名与三语示例。
- `cargo test --locked --offline -p openrust-pail`：6 项消息／筛选测试、3 项文件父测试、1 个子进程入口及自定义验收程序通过。四进程 400 条完整 JSON 回归通过。
- 实际 TCP HTTP 同时访问两个应用，分别生成捕获文件。独立实际二进制执行测试命令 `artisan capture:inspect --auth=<当前用户> --level=iNfO --message='after IDENTITY' --filter=get --timeout=1e2`，通过 `Options::fromCommand` 读取真实输入并筛选真实记录；两应用都恰好选中自身的一条认证后日志，timeout 为 100。另复验嵌套命令、并发命令与同步任务的来源隔离。`capture:inspect` 仅在测试应用注册，未污染生产命令目录。
- 本批不修改数据库／Worker，未提供数据库 URL，夹具明确报告 MySQL Worker 跳过；上一批的真实 MySQL 验收保持独立证据，不混算本批通过项。
- `cargo clippy --locked --workspace --all-targets --features pail -- -D warnings` 通过。首轮发现 JSON 辅助格式化函数缺少 Display trait 导入，修正后定向测试、实际入口及完整严格检查通过。
- `cargo +1.94.0 fmt --all --check`、`git diff --check`、三语站点构建及根路径和 `/docs/` 的 12 项浏览器检查通过。

## 文件范围与剩余项

```text
Cargo.lock
vendor/openrust/pail/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── options.rs
│   └── value_objects/{mod,json,origin}.rs
└── tests/
    ├── capture.rs
    ├── options.rs
    └── fixtures/{pail-options.json,pail_options_oracle.php}
docs/{en,zh_CN,zh_TW}/the-basics/logging.md
.agents/project/
├── audits/06-08/{README,pail,pail-options,pail-auth-boundary}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

[认证解析边界](pail-auth-boundary.md)仍未闭合。提前认证、响应后回填均改变调用时序，不采用。额外复核固定 PailServiceProvider，测试环境跳过策略尚未接入，`VAPOR_SSM_PATH` 属 PHP 部署判据，需核定 Rust 适配。下一批先补 Provider 环境门禁，再按依赖顺序实现 CliPrinter、实际 `pail` 会话命令、超时／信号清理与 dev 注册。Pail 整体保持未完成，Windows 捕获路径继续在发布矩阵跟踪。

原始日志、固定源码和命令保留于 `/private/tmp/openrust-pail-options.n8akrhok`。本批约 3.95 GB 远程源码与编译缓存、无人使用且摘要匹配本批拉取记录的 PHP 镜像、57.91 MB 站点产物已回收，数据卷清单一致。11 个候选源码文件在最终验收后核对 SHA-256 一致。提交前磁盘可用 56.36 GB（52.49 GiB），周额度剩余 15%，第二窗口无读数。沿用用户本轮忽略额度停线的授权，磁盘规则生效。
