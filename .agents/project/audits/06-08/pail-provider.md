# Pail Provider 环境门禁与延迟解析

## 固定对照与复用

承接 `597cca445`，对应固定 Laravel 日志文档的 Pail 小节及应用环境检测。核对 Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 Application、Contracts/Foundation/Application 和 Support/Env，以及 Pail v1.2.5 `fdb73f5eacf03db576c710d5a00101ba185f2254` 的 PailServiceProvider。旧版 `openrust-v0.x` `6d1a81924` 的 `foundation/application.rs::running_unit_tests`、`config/app.rs` 和消费端配置测试，提供精确 testing 判定与默认非测试环境回归思路；按当前容器绑定和 Result 契约重写。应用装配与 Runner 延续固定 Goravel 的编译型实现，本批不改入口。

[固定 Pail Provider](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/PailServiceProvider.php) · [固定 Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php#L792) · [固定 Env](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Env.php#L234)

## 实现与 Rust 适配

- 在 Application 及其契约提供 `runningUnitTests() -> Result<bool, BindingResolutionError>`。环境未绑定时直接返回 Ok(false)，不触发解析。绑定后只比较容器 env 是否精确为 testing，区分大小写。与测试进程类型、config("app.env") 后续变更无关。解析失败通过 Result 传播，对应 PHP 的异常。
- 完成配置加载后，将既有 Environment 移入容器的 Arc 绑定，不复制整个环境表；保留至应用容器释放。Provider 可读取本应用不可变快照，遵守进程变量优先、--env 文件选择和配置缓存跳过文件的规则。避免用进程全局修改破坏多应用隔离。
- `runningPailTests` 保留固定上游名称，按 Env 规则处理 true／false、empty、null、括号、引号和大小写，再做 PHP 字符串真假转换。VAPOR_SSM_PATH 直接按原始字符串处理，仅空串和 0 为 false。PAIL_TESTS 为 true 时覆盖 testing 与 Vapor 两个跳过条件。
- 跳过时仍注册 Files／Handler 单例绑定，不安装日志和队列捕获监听，也不构造服务。正常模式在首次被接受日志或队列生命周期事件时才解析 Handler，Files 同样延迟解析。构造错误在该事件发生时传播；日志观察在原渠道写入后执行。
- 监听器持有容器 Weak 引用，避免形成应用／日志注册表的强引用环；失效引用映射为既有 ScopeClosed 错误。环境旗标使用已有 get_utf8 契约，非法 UTF-8 沿配置错误传播。
- VAPOR_SSM_PATH 仅保留上游跳过判据，本批不引入 PHP Vapor 部署适配。统一服务进程不以 PHP SAPI 判断命令入口；后续 Pail 命令将通过现有控制台内核注册和执行。

## 实际验收

隔离基线 `597cca445`，仅覆盖本批 7 个源码／测试文件。Ubuntu 原生 amd64、Rust 1.94.0、Debug 构建。框架原有 Env 检测、配置缓存和公开 Environment 原始读取方法保持原契约。

- `cargo test --locked -p openrust-pail`：文件父测试与四进程追加、6 项消息／筛选测试、自定义捕获和 Provider 验收程序通过。Provider 包含 22 组实际 HTTP＋独立 Artisan 环境矩阵，另验证并发双应用、--env、进程变量覆盖、配置缓存、未绑定零解析、动态 env 重绑定和解析错误、延迟构造失败及源日志保留。禁用捕获不影响原日志驱动写出。
- 日志／Context 的 22 项回归通过。根 `feature` 14 项、`console` 7 项及 `environment_lifecycle` 3 项通过，包含原始根二进制的 Artisan、环境选择和双应用 HTTP。
- 临时数据库统一由 `tests/mysql_acceptance.py` 以 1 GiB tmpfs 启动，实际版本 MySQL 8.4.13。真实 `queue:work` 验证非隔离失败与失败回调、失败表记录、后续消费，以及硬超时隔离子进程与 deferred 回调来源。
- 全工作区、全目标、启用 pail 的严格 Clippy 通过。私有辅助方法名对齐为 runningPailTests 后，再次通过包测试、真实 MySQL Worker 与完整严格检查。两轮退出码记录均为 0。
- `cargo +1.94.0 fmt --all --check`、`git diff --check`、三语配置与日志正文、站点构建及根路径和 `/docs/` 的 12 项浏览器检查通过。
- 隔离根应用启用 pail feature 后，真实二进制 package:discover 自动发现 Provider。APP_ENV=testing 且 PAIL_TESTS=false 时独立命令正常、捕获为空；改为 true 后 CLI 与 HTTP 各捕获一条记录。`/up`、日志探针均返回 200，inspire 退出 0，SIGINT 退出 0 且端口释放。临时路由恢复并逐字节核对。

## 最小文件范围

```text
vendor/openrust-framework/src/
├── contracts/foundation.rs
└── foundation/application.rs
vendor/openrust-pail/
├── Cargo.toml
├── src/{lib,handler}.rs
└── tests/{capture,provider}.rs
docs/{en,zh_CN,zh_TW}/
├── getting-started/configuration.md
└── the-basics/logging.md
.agents/project/
├── audits/06-08/{README,pail,pail-provider}.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

`main.rs`、`bootstrap/` 和用户环境模板未修改。下一批先补 Application 统一存储路径契约，再接 CliPrinter、实时会话命令、超时与信号清理及 dev 注册。[认证自动解析](pail-auth-boundary.md)仍未闭合；Windows 捕获与文件锁继续在发布矩阵跟踪。Pail 当前目录映射为 base_path()/storage/pail，应用可替换 Files 绑定；框架尚无 Laravel 的 storagePath／useStoragePath 入口，统一存储路径契约需要另行补齐，不能宣称自定义 storagePath 已对齐。

原始日志与夹具保存于 `/private/tmp/openrust-pail-provider.yd1hxja1`。7 个源码候选在最终验收后核对 SHA-256 一致。约 6.11 GB 远程源码与编译缓存、当批拉取且无人使用的匹配摘要 MySQL 镜像、57.93 MB 站点产物已回收，数据卷清单一致。提交前磁盘可用 55.47 GB（51.66 GiB），周额度剩余 15%，第二窗口无读数；沿用用户忽略本轮额度停线的授权，磁盘规则继续生效。
