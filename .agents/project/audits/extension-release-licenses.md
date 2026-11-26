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

本表包含构建依赖及根包，记录 Cargo 解析结果和已声明的许可。下文记录 Linux ARM64、Linux AMD64、Windows ARM64、Windows x64 和 macOS ARM64 组合候选制品的实际检查。原生主机覆盖、发布渠道中的许可随附及其他原生依赖仍需逐项核对。Webhook 双包已有的制品证据单独维护。[Webhook 制品与许可](webhooks/artifact-licenses.md)

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

## Windows ARM64 组合候选制品

用 `7dd141aac49bafa452f35293d68b45375b36d4ff` 的干净归档，在 Windows 11 ARM64 虚机的专用 NTFS 盘，以 Rust 1.94.0、`aarch64-pc-windows-msvc`、`--release --locked --offline --jobs 1 --bin main` 和 `standalone` 加上述九个扩展特性完成构建。宿主触及磁盘停线后，以同一归档和构建选项续跑，复用虚机 NTFS 上已完成的编译产物。

- 从 Windows PowerShell 生成的 ZIP 解包并启动。归档 SHA-256 为 `fd5a4ec6f5a2ae838a211e78f1284f113d6a5a038d648f972cf13e30925914b7`。解包二进制 SHA-256 为 `362dfe5481d9ddac6d932c25f44f8f024d90ca6340cd053059cfd2dfba874e06`，与构建产物一致。10 个许可文件与源码逐字节一致。
- PE32+ 的机器类型为 `0xAA64`，栈保留 16 MiB，内嵌 SQLite source ID 与锁定的 SQLite 3.54.1 一致。MSVC `dumpbin /dependents` 列出 Windows 系统库、`VCRUNTIME140.dll` 和 UCRT API 集，未列出 `sqlite3.dll`。本轮验收使用安装了 MSVC 工具链的虚机，未测无工具链机器上的运行库安装流程。
- Artisan 发现九个 Provider，执行 58 项 SQLite 迁移，生成配置和路由缓存，重跑迁移无待执行项。缓存前后实际 HTTP `/up`、`/` 和 `/api` 均返回 200。两次都在各自的专用 Windows 控制台发送 Ctrl+C，服务进程以状态 0 退出。首次脚本读取控制进程的退出状态时报错，预先持有进程句柄后，用同一二进制在新目录复跑通过。
- 缓存后使用 Windows WebSocket 客户端连接 Reverb、订阅频道并收到 HTTP 签名发布的事件。独立 `artisan queue:work database --once` 成功退出。只读检查 SQLite 确认迁移记录数为 58，相关应用表已持久化，`jobs` 表为空。
- 在同一二进制上启用 New Relic Provider，重建 ZIP 后得到与上次相同的归档摘要，复跑 HTTP、Artisan、Reverb 及退出。回环导出经 Rust TCP 转发到 Parallels 私有网络的隔离收集器。实际 protobuf 解码得到 206 个 Span 和 12 条指标记录，核对服务名、`GET /api` 响应与路由属性、`http.server.request.duration` 及独立 `artisan inspire` Span。收集器拒绝携带 `api-key` 的请求。本批使用隔离收集器和合成业务数据，真实账号按模块门禁验收。

构建、PE 导入及运行日志保存在本机 `target/tmp/goal-extension-combined-windows/`，OTLP 复跑在其 `otlp/` 子目录。验收覆盖 Windows ARM64 虚机里的独立 ZIP。Windows Docker Desktop、无 MSVC 工具链的 Windows 安装、原生 Windows x64 和实际发布渠道仍需各自的环境。

根据实际 PE 导入和 Microsoft 的兼容规则，三语部署指南补充 `VCRUNTIME140.dll` 的运行库架构及版本要求。VitePress 构建通过，Playwright 在 `/` 和 `/docs/` 下核对六个页面的已渲染说明及官方链接。该检查覆盖发布文档。 [Microsoft Visual C++ v14 运行库](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist)

## Windows x64 组合预检

在同一 Windows 11 ARM64 虚机核对 `7dd141aac` 归档。Rust 1.94.0 已安装 `x86_64-pc-windows-msvc` 标准库，MSVC HostARM64 的 x64 链接器及 Windows SDK 的 x64 库均可用。针对 x64 执行 `cargo tree --locked --offline`，九个扩展依赖均出现。以 `rustc --target x86_64-pc-windows-msvc` 编译最小探针，核对 PE 机器类型为 `0x8664`，在虚机的 x64 兼容层执行成功。此项验证工具链及兼容层加载，完整应用、发布归档、实际运行行为和原生 x64 主机仍需独立验收。

## Windows x64 组合候选制品

沿用 `7dd141aac49bafa452f35293d68b45375b36d4ff` 的干净归档，用 Rust 1.94.0、`x86_64-pc-windows-msvc`、MSVC HostARM64、`--release --locked --offline --jobs 1 --bin main` 和相同的九项扩展及 `standalone` 构建。两次宿主磁盘停线都只停止本批构建树。宿主持锁清理后，复用专用 NTFS 盘已完成的编译产物，等待其他虚机构建退出后用相同选项续跑。

- Windows 生成的 ZIP 的 SHA-256 为 `376ed9f455e2bd2fb9f9ced95d14160663c686276661ff711c1351eadfb094fa`，解包二进制为 `7f13525d4c4a557ef5e26a291ea50082bddd34165f9f777e79a91eb3e0f1426e`。10 个许可文件与源码逐字节一致。
- PE32+ 的机器类型为 `0x8664`，栈保留 16 MiB，内嵌 SQLite source ID 与锁定的 SQLite 3.54.1 一致。MSVC `dumpbin /dependents` 显示 Windows 系统库、`VCRUNTIME140.dll` 和 UCRT API 集，未列出 `sqlite3.dll`。虚机已安装 x64 Visual C++ 运行库。
- 从解包目录运行，Artisan 发现九个 Provider、执行 58 项 SQLite 迁移并生成配置与路由缓存，重复迁移无待执行项。缓存前后的实际 HTTP `/up`、`/` 和 `/api` 均返回 200。缓存后连接、订阅及接收 Reverb 签名投递通过，独立 `artisan queue:work database --once` 成功退出且队列表为空。两个 HTTP 进程响应各自私有控制台的 Ctrl+C 并以状态 0 退出。
- 同一制品启用 New Relic Provider 后再跑整套，归档与二进制摘要一致。隔离私有网络收集器解码 206 个 Span 和 12 条指标记录，核对服务身份、`GET /api` 状态与路由、请求耗时指标及 `artisan inspire` Span。

实际运行环境为 Windows ARM64 虚机的 x64 兼容层，证据位于本机 `target/tmp/goal-extension-combined-windows-x64/`。原生 Windows x64、Windows Docker Desktop、无 MSVC 工具链的安装、真实 New Relic 租户及目标发布渠道按各自环境复验。

## macOS ARM64 组合候选制品

沿用 `7dd141aac49bafa452f35293d68b45375b36d4ff` 的干净归档。使用本机 ARM64、macOS 27.2.1、Rust 1.94.0、`aarch64-apple-darwin`、`--release --locked --offline --jobs 4 --bin main` 及同一 `standalone` 和九项扩展特性。两次磁盘停线经过项目持锁清理。并发从 1 调至 4 后完成首个构建。

- 默认构建在本机链接 Homebrew 的 `libpcre2-8.0.dylib`，其二进制 SHA-256 为 `fa1e64b6ec2d7b15e5ec48b92a3b17cff6df6282cdbfdebcccc4793240784d21`。用锁定 `pcre2-sys` 的 `PCRE2_SYS_STATIC=1` 重建。解包后 `otool -L` 只列出 macOS 系统库和 Framework，`nm -U` 核对静态 PCRE2 编译及 JIT、SQLite、AWS-LC 和 ring 已定义的符号。SQLite source ID 与锁定的 3.54.1 一致。
- 归档 SHA-256 为 `20178e3d64854360ab36a2f5aeee230369217146a4f55e6e42839bab77a118a6`，解包二进制为 `87e28497af025b6e2d6edf876f34e99191d3fdae33b205337f29d5963551a17d`。10 个许可文件与源码逐字节一致。Mach-O 为 ARM64 可执行文件，本机链接器临时签名通过 `codesign --verify --strict`。Mach-O 最低版本字段为 11.0，实际运行验证覆盖本机 27.2.1。
- 从解包目录启动，Artisan 发现九个 Provider 并执行 58 项 SQLite 迁移。配置及路由缓存前后，实际 HTTP `/up`、`/`、`/api` 均返回 200。两个进程分别响应 SIGINT、SIGTERM 并以状态 0 退出。缓存后签名 Reverb 投递通过，独立 Artisan Worker 成功退出且数据库队列为空。
- 本机隔离 OTLP 收集器解码 206 个 Span 和 12 条指标记录，核对服务名、HTTP 路由及状态、请求耗时指标和独立 `artisan inspire` Span。三语部署指南补齐 Mac 构建选项和动态依赖检查，VitePress 构建以及两种部署前缀下的六个浏览器页面通过。

构建、Mach-O、许可、运行和文档日志保存在本机 `target/tmp/goal-extension-combined-macos/`。Apple Developer ID 签名、公证、其他系统版本和目标分发渠道仍须对应授权和环境。

### macOS ARM64 原生归档链接溯源

沿用同一归档、锁文件、Rust 1.94.0 和静态 PCRE2 设置。用 `cargo rustc` 只为最终二进制加入 Apple 链接器的 `-map` 参数，并以实际输出的原生归档成员逐项核对映射。Cargo 构建脚本列出五个相关 crate 的六个静态归档。每个匹配到的对象仅归属一个归档，符号统计采用链接器的保留区及裁剪区。单位为映射条目。

| 锁定来源 | 对象数 | 保留符号 | 裁剪符号 |
| --- | ---: | ---: | ---: |
| `aws-lc-sys` 0.47.0 | 185 | 4428 | 2865 |
| `libsqlite3-sys` 0.39.1 | 1 | 4268 | 235 |
| `pcre2-sys` 0.2.10 | 23 | 1303 | 55 |
| `ring` 0.17.14 生产归档 | 21 | 151 | 13 |
| `ring` 0.17.14 测试归档 | 0 | 0 | 0 |
| `blake3` 1.8.7 NEON 归档 | 1 | 0 | 2 |

四项保留原生代码的归档来源均可对应根发行声明的许可条目。本次在映射的裁剪区定位到 `blake3_hash_many_neon`，因此该原生对象虽被构建和枚举，映射内没有它的保留符号。范围为该 Mach-O 的六个 Cargo 构建脚本静态归档。Rust 工具链原生代码、其他平台链接映射及最终渠道审核单独核对。

映射对应的 Mach-O SHA-256 为 `473b2b43d91f268773e28dd3f768797ee3a12e1872b49844ca7f683899b41f5f`，重新打包的归档 SHA-256 为 `7c84abe9743af785147aed6c1bb460a832763aa7646e8e68e7003749179c7307`。解包后复验十个许可文件、签名、系统库依赖、58 项迁移、缓存前后 HTTP、Reverb、独立 Worker 和 OTLP。压缩映射及验证日志保存在本机 `target/tmp/goal-extension-combined-macos/link-provenance/`。
