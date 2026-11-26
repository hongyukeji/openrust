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

本表包含构建依赖及根包，记录 Cargo 解析结果和已声明的许可。下文记录 Linux ARM64 和 AMD64 组合候选制品的实际检查。其他目标的最终二进制、发布渠道中的许可随附及其他原生依赖仍需逐项核对。Webhook 双包已有的制品证据单独维护。[Webhook 制品与许可](webhooks/artifact-licenses.md)

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

构建、ELF、许可和运行日志在本机 `target/tmp/goal-extension-combined-image/`。ELF 符号表中的 `blake3_neon.c` 是文件条目，对应的静态可执行代码仍需用链接映射核对。该候选制品验收覆盖本机 ARM64 Docker 环境。

## Linux AMD64 组合候选制品

用 `ab1d2b4c8553b45768d3cbc36efca0c9958c58c3` 的干净归档、仓库 Dockerfile、Rust 1.94.0 和相同的九项特性在 ARM64 Docker Desktop 交叉构建 `linux/amd64` Release 镜像。构建使用 `APP_BUILD_JOBS=1`。候选制品对应的 `x86_64-unknown-linux-gnu` 锁定生产与构建依赖图包含 597 个包版本，Cargo 与 cargo-deny 的清单逐项一致。

- OCI 索引摘要为 `sha256:37ffb47c67e82522266d0b044f2a4c0b291f3e44cb9f091c2118f0caae254e60`，AMD64 镜像清单摘要为 `sha256:aa68f8954715057e7c98a8bf90feba8a65367bccbeeb97bdf3ce5913966ab77c`。实际 `/app/main` 的 SHA-256 为 `806d1810c89ba0c63f95b386d53a96fb1f10a53886f2812a0462e3e6c53dd074`。
- `readelf` 确认 x86-64 ELF 定义 PCRE2 编译与 JIT、SQLite、AWS-LC 和 ring 原生符号。SQLite source ID 与根发行声明一致。运行镜像中 `ldd` 显示 glibc、libm、libgcc 和动态加载器。以 UID/GID 10001 从只读容器读取的 10 个许可文件与源码逐字节一致，Debian glibc 和 libgcc 版权文件可读。
- 在 x86_64 仿真运行环境，九个 Provider 发现、58 项 SQLite 迁移及幂等重跑通过。配置和路由缓存前后，HTTP `/up`、`/`、`/api` 返回 200。缓存后 Reverb 建立连接、订阅并接收签名发布的事件。独立 SQLite Worker 空队列检查成功，HTTP 容器响应 SIGTERM 并以状态 0 退出。
- 相同镜像启用 New Relic Provider 后重跑上述流程。使用同一容器网络命名空间的回环 Collector，实际 protobuf 解码得到 206 个 Span 和 12 条指标记录。核对服务名、`GET /api` 的响应状态及路由标签、`http.server.request.duration` 和独立的 `artisan inspire` Span。真实租户的摄取与查询沿用模块发布检查。
- 三语部署说明补充可选功能列表和预发布包发现。Compose 解析确认逗号分隔的值原样传入构建参数，VitePress 构建成功，Playwright 核对两种部署前缀下的六个页面。

本机日志保存在 `target/tmp/goal-extension-combined-amd64/`。历史 Ubuntu 实机证明已有部署链路，当前组合仍需可确认的隔离主机 SSH 入口、登录用户和非交互 Docker 权限复验。完整静态依赖溯源、目标发布渠道和外部测试租户仍按各自条件核验。[历史原生 Ubuntu 验收](08-04/docker.md#原生-ubuntu-x86_64-发布复验)

## Windows ARM64 组合预检

在现有 Windows 11 ARM64 虚机上核对 `ab1d2b4c8` 归档和 Rust 1.94.0。首次离线解析缺少 `mago-syntax`，按锁文件补齐公开依赖后，用 `cargo tree --locked --offline --target aarch64-pc-windows-msvc --edges normal --depth 1` 复验，九个扩展依赖均出现，联网与离线树输出逐字节一致。专用验收盘有 34,179,629,056 字节可用，虚机未检测到 Docker CLI。组合 PE、运行与许可随附仍需实际构建验收。

用 `7dd141aac` 的干净归档执行独立发布许可预检。Windows PowerShell 原生创建 ZIP 并解包，核对根许可证、根发行声明以及两个 Webhook 包的声明和许可证，共 10 个文件与源码逐字节一致。三语部署说明补齐复制清单，VitePress 构建成功，Playwright 核对两种部署前缀下的六个页面。完整 ZIP 与组合 PE 的运行验收继续单列。

同一归档首次把 Cargo 输出写入虚机共享盘，Rust 构建探针清理目录时收到 Windows 错误 87。切换到专用 NTFS 构建盘后，续跑触及 32 GB 停线。停止本次构建树后持锁清理八个项目编译目录，释放 10,256,171,008 字节，可用空间恢复到 42,202,472,448 字节。源码与锁文件沿用同一归档，构建进程和磁盘停线同时在虚机内受监控。预检和构建日志保存在 `target/tmp/goal-extension-combined-windows/`。
