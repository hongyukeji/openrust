# 扩展组合依赖与 SQLite 发行声明

## 五个目标的锁定依赖图

在源码 `f236bb1ef78f5b743eb11d6edba1620aac1a46f6` 上，用 Rust 1.94.0、cargo-deny 0.20.2 和同一份 `Cargo.lock` 核对扩展组合。关闭默认特性，启用 `webhook-client,webhook-server,translate,pay,localization-database,content-moderation,reverb,cashier,new-relic`。存储、短信与 Lang Loader 的基础能力随框架进入依赖图。

对每个目标运行 `cargo tree --locked --target <target> --edges normal,build` 和 `cargo deny --exclude-dev --locked --target <target> list --format json --layout crate`。按包名和版本去重，五组结果逐项一致，许可字段均非空。补齐本地缓存中缺少的两个锁定 Linux 依赖后，对五个目标离线重跑，输出与首次成功结果逐字节一致。

| 目标 | Cargo 包版本数 | cargo-deny 包版本数 |
| --- | ---: | ---: |
| `aarch64-apple-darwin` | 597 | 597 |
| `x86_64-unknown-linux-gnu` | 597 | 597 |
| `aarch64-unknown-linux-gnu` | 596 | 596 |
| `x86_64-pc-windows-msvc` | 608 | 608 |
| `aarch64-pc-windows-msvc` | 607 | 607 |

本表包含构建依赖及根包，记录 Cargo 解析结果和已声明的许可。下文记录 Linux ARM64 组合候选制品的实际检查。其他目标的最终二进制、发布渠道中的许可随附及其他原生依赖仍需逐项核对。Webhook 双包已有的制品证据单独维护。[Webhook 制品与许可](webhooks/artifact-licenses.md)

## 原生 SQLite 制品

另取源码 `577daf16fe76e562bdb09f4a01512bf9668856ef` 的 macOS ARM64 Reverb 验收二进制，SHA-256 为 `f65283f74ce2788fbeae1d56c5a0612cddff1b6b6f355b61c028c3f4874b1eaa`。`nm -gU` 检查到该制品定义了 `sqlite3_libversion` 和 `sqlite3_open_v2`，`otool -L` 的动态库列表无系统 SQLite。制品内的 SQLite source ID 与锁定的 `libsqlite3-sys` 0.39.1 归档中 SQLite 3.54.1 的声明相同。

对归档计算 SHA-256 和 `Cargo.lock` 比对，再将 Rust 封装的 MIT 原文及内嵌 SQLite 的来源补入根发行声明。现有 Docker 构建配方将根发行声明复制到 `/app/`。[根发行声明](../../../THIRD_PARTY_NOTICES.md) · [Docker 构建配方](../../../docker/Dockerfile)

用该制品启动隔离 SQLite 应用，`migrate --force`、`config:cache`、`route:cache` 成功，真实 HTTP `GET /up` 返回 200，独立 `queue:work database --once` 成功退出且数据库队列为空。三语部署文档构建成功，Playwright 验证 `/` 和 `/docs/` 两种部署前缀下的六个页面。

依赖图、原生符号及测试日志保存在本机 `target/tmp/goal-extension-release-licenses/`。

## 静态 ring 原文

从本机既有的 `openrust:localization-bundled-final` 镜像取出 ARM64 ELF。镜像 ID 为 `sha256:f55bca90ba3778fb4b4af162e609fedd70a29643549da1b85f48dc1236479b04`。`readelf --wide --syms` 显示 `ring_core_0_17_14__x25519_sc_reduce` 等符号在 ELF 内定义。该制品用于定位既有声明缺口，组合构建另外核验。

按 `Cargo.lock` 校验 `ring` 0.17.14 的归档 SHA-256 `a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7`，将归档中的六份许可原文补入根发行声明。六段分别与归档逐字节一致，覆盖许可索引、BoringSSL、ring 的 ISC、once_cell 双许可和 Fiat Cryptography。三语部署文档已通过 VitePress 构建，Playwright 复验了 `/` 和 `/docs/` 两种部署前缀下的六个页面。[锁定的 ring 归档](https://static.crates.io/crates/ring/ring-0.17.14.crate) · [根发行声明](../../../THIRD_PARTY_NOTICES.md)

## Linux ARM64 组合候选制品

用 `0d56e59c14345abc9dfa65e03efbd2c6577c8ffd` 的干净归档，叠加上述 ring 发行声明，在本机 ARM64 Docker Desktop 从仓库 Dockerfile 构建 Release 镜像。使用 Rust 1.94.0、上述九项特性、Dockerfile 的 `standalone` 和 `APP_BUILD_JOBS=1`。首次构建触及 32 GB 停线，守卫取消命令并在协调锁下清理项目编译产物，释放 68,368,412,672 字节。重跑成功，未改变编译选项或依赖锁文件。

- 本地 OCI 索引摘要为 `sha256:1142145a50815b88a8ec2cf8d4abc31b07142061e43b49a71a43d3935e09f5dc`，ARM64 镜像清单摘要为 `sha256:ebda1764c5ad563a8f95399759e87ccc37f2a99fcc0fe4b8a568f7ffe5fb828f`。实际 `/app/main` 的 SHA-256 为 `092e37bb978ee9bc619b384588a2e77d728df40c2fbc039621895f8c93ff2474`。
- 从 UID/GID 10001 的只读容器读回 10 个许可文件，与源码逐字节一致。根发行声明包含 SQLite 来源和 ring 的六份原文。动态 PCRE2、glibc、libgcc 的 Debian 版权文件可读，版本与摘要已记录。ARM64 ELF 中定义了 SQLite、AWS-LC 和 ring 的原生符号，SQLite source ID 与前述归档一致。
- 独立 SQLite 挂载内，`package:discover` 的实际清单只含上述九个包。Artisan 执行 58 项迁移，核对 Webhook 收发、Pay、Cashier 和内容审核数据表。生成配置及路由缓存后重跑迁移，无待执行项。缓存前后实际 HTTP `/up`、`/`、`/api` 均为 200，两个容器响应 SIGTERM 并以状态 0 退出。
- 缓存后，Reverb WebSocket 建立连接、订阅频道并收到 HTTP 签名发布的事件。另启独立 Artisan `queue:work database --once` 成功退出，确认队列为空。带业务任务的验收仍引用各模块的独立测试与集群记录。

构建、ELF、许可和运行日志在本机 `target/tmp/goal-extension-combined-image/`。ELF 符号表中的 `blake3_neon.c` 是文件条目，对应的静态可执行代码仍需用链接映射核对。该候选制品验收覆盖本机 ARM64 Docker 环境。组合的 x86_64、Windows、发布渠道、完整静态依赖溯源及外部测试租户仍按各自的交付条件核验。
