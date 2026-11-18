# Pail 会话捕获

## 固定对照与范围

对应固定 Laravel 日志文档的 [Tailing Log Messages Using Pail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#tailing-log-messages-using-pail)，按 [Pail v1.2.5 Spec](pail.md) 的第二批交付。核对固定 [Handler](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Handler.php)、同版 File、Files、LoggerFactory 和 PailServiceProvider。旧版 `openrust-v0.x` `6d1a81924` 无对应 Pail 实现，复用当前日志监听、包发现、HTTP、Artisan、认证、Context 与队列运行路径。应用装配继续沿用固定 Goravel 的编译型 Provider 注册，不修改应用入口。

独立 `openrust-pail` 包提供 `PailServiceProvider`、`File`、`Files` 和 `Handler`；根应用通过可选 `pail` feature 接入现有包发现。没有会话文件时不创建捕获文件。任意渠道接受的记录均可捕获，保留原消息、级别和结构化异常，过滤固定上游的 Warning 弃用模式。公开 Context 后合并并覆盖消息 context，隐藏 Context 不进入捕获。

`storage/pail/*.pail` 使用 JSON 行、跨进程排他文件锁、UTC 微秒时间和 Monolog 级别编号。创建不截断，删除幂等，日志不重建被删除的文件，超过一小时未写入的会话被清除。原渠道成功写入后，捕获文件错误仍沿日志调用传播。

## Rust 适配与未闭合项

- PHP 每个进程的最后生命周期事件转换为应用绑定的 Tokio 执行作用域。HTTP 保留 `Request::path()` 的原始编码与 Laravel 去首尾斜杠行为；同进程嵌套 Artisan 保留 HTTP 来源。独立命令、并发命令、同步任务和独立 Worker 分别隔离，任务完成恢复父作用域。硬超时子进程通过现有内部协议传递原命令名。不新增进程或修改 Worker 超时机制。
- 通用来源接口为 `openrust::log::context::{Origin, Http}`。没有日志监听器时跳过来源收集与认证快照。异步命令和任务的内部 future 先置于堆上再包作用域，避免再次搬运大型 Debug 栈状态。本批未作吞吐或零开销声明。
- 同步日志读取当前 guard **已经解析**的认证身份，支持后续切换 guard；异步认证尚未解析时为 null。PHP Pail 同步调用 Auth 可触发认证解析。这项差异仍待异步认证入口补齐，不宣称完全等价。用户邮箱通过现有 `Authenticatable::getAuthAttribute("email")` 契约读取，Rust 没有 PHP 基类运行时判定。
- 结构化异常沿用现有规范化快照；存在 trace 数组时写入来源，否则为 null。Rust 标准错误没有 PHP 原始调用栈。
- `pail` 命令、筛选、显示、超时、Ctrl+C 和 dev 注册尚未交付。Windows 新文件锁及捕获作用域留待平台矩阵实机验收，不能以 Linux 结果代替。

## 实际验收

以 `4d3ef6db9` 为隔离基线，仅叠加本批核定源码；Ubuntu 原生 amd64、Rust 1.94.0、Debug 构建。临时 MySQL 统一由 `tests/mysql_acceptance.py` 创建 1 GiB tmpfs 实例，查询版本为 8.4.13。

- `cargo test --locked --offline -p openrust-pail`：3 项父测试、1 个子进程入口和 1 个实际入口验收程序通过（标准测试报告共 4 项）。四个独立子进程各写 100 条大消息，验证 400 条完整 JSON、无重复与交错破损。覆盖创建保留内容、文件过滤、过期、删除不复活、错误传播及原渠道已写入。
- 实际 TCP HTTP 同时访问两个应用，验证方法、路径、认证前后、guard 切换、公开与隐藏 Context、双会话写入、嵌套 Artisan 和应用隔离。重新执行当前二进制验证独立 Artisan、并发命令和同一命令内并发同步任务来源。真实 `queue:work` 消费 MySQL 队列，验证非隔离失败与回调、失败表记录、后续消费和硬超时隔离子进程及其 deferred 回调的来源。
- `cargo test --locked --offline -p openrust-framework --test session_authentication --test log_listeners --test logging --test log_context -- --test-threads=1`：29 项通过。其中认证既有夹具同时执行了 MySQL 与 SQLite 用例，SQLite 属额外回归，不替代发布矩阵；后续日常回归继续按规范只筛选 MySQL。
- `cargo test --locked --test console`：7 项通过。首次离线运行缺少 `prost` 缓存，随后仅按锁文件下载依赖；锁文件没有升级注册表版本。
- 最终源码另跑 22 项日志／Context、1 项精确 MySQL 认证及 7 项根 Console 回归，全部通过。`cargo clippy --locked --workspace --all-targets --features pail -- -D warnings` 通过。
- 隔离根应用临时增加 HTTP／命令日志探针，`cargo build --locked --offline --features pail --bin main` 成功。真实根二进制 `artisan package:discover` 自动发现 `openrust-pail`，独立探针和 `inspire` 退出 0；HTTP `/up` 与 `/capture/probe` 返回 200，分别验证命令和 HTTP 来源。SIGINT 退出 0，端口可重新绑定。夹具结束时恢复两个路由文件，并逐字节核对。
- 默认未启用 Pail feature 的原始根应用执行 `./artisan inspire` 退出 0，验证可选包不影响既有启动器。
- `cargo +1.94.0 fmt --all --check`、`git diff --check`、三语站点构建及 `/`、`/docs/` 的 12 项浏览器检查通过。三语日志正文同步已验收边界，没有提前宣称 Pail 命令可用。

## 最小文件范围

```text
Cargo.toml
Cargo.lock
vendor/openrust/
├── pail/
│   ├── Cargo.toml
│   ├── src/{lib,file,files,handler}.rs
│   └── tests/{capture,files}.rs
└── framework/src/
    ├── auth/{auth_manager,mod,session_guard}.rs
    ├── console/application.rs
    ├── foundation/http.rs
    ├── log/
    │   ├── context/{mod,origin}.rs
    │   ├── events/mod.rs
    │   ├── log_manager.rs
    │   └── log_service_provider.rs
    └── queue/{isolated_job,queue_base}.rs
docs/{en,zh_CN,zh_TW}/the-basics/logging.md
.agents/project/
├── audits/06-08/{README,pail,pail-capture}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

原始验收日志和根应用临时路由夹具保存于 `/private/tmp/openrust-pail-capture.iXYywfoW`。本批约 6.12 GB 远程源码与编译缓存、当批新拉取且无人使用的 MySQL 镜像和 57.87 MB 站点产物已回收，数据卷清单一致。21 个候选源码文件在最终验收后核对 SHA-256 一致，根路由夹具已恢复。磁盘可用 56.43 GB（52.55 GiB），提交前周额度剩余 15%，第二窗口无读数，按用户本轮忽略额度停线的明确授权继续，磁盘规则生效。
