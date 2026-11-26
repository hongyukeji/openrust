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

本表包含构建依赖及根包，记录 Cargo 解析结果和已声明的许可。组合构建的最终二进制、发布渠道中的许可随附及其他原生依赖仍需用实际候选制品逐项核对。Webhook 双包已有的制品证据单独维护。[Webhook 制品与许可](webhooks/artifact-licenses.md)

## 原生 SQLite 制品

另取源码 `577daf16fe76e562bdb09f4a01512bf9668856ef` 的 macOS ARM64 Reverb 验收二进制，SHA-256 为 `f65283f74ce2788fbeae1d56c5a0612cddff1b6b6f355b61c028c3f4874b1eaa`。`nm -gU` 检查到该制品定义了 `sqlite3_libversion` 和 `sqlite3_open_v2`，`otool -L` 的动态库列表无系统 SQLite。制品内的 SQLite source ID 与锁定的 `libsqlite3-sys` 0.39.1 归档中 SQLite 3.54.1 的声明相同。

对归档计算 SHA-256 和 `Cargo.lock` 比对，再将 Rust 封装的 MIT 原文及内嵌 SQLite 的来源补入根发行声明。现有 Docker 构建配方将根发行声明复制到 `/app/`。该轮运行验收使用上述 macOS 制品，组合镜像仍待构建核验。[根发行声明](../../../THIRD_PARTY_NOTICES.md) · [Docker 构建配方](../../../docker/Dockerfile)

用该制品启动隔离 SQLite 应用，`migrate --force`、`config:cache`、`route:cache` 成功，真实 HTTP `GET /up` 返回 200，独立 `queue:work database --once` 成功退出且数据库队列为空。三语部署文档构建成功，Playwright 验证 `/` 和 `/docs/` 两种部署前缀下的六个页面。

依赖图、原生符号及测试日志保存在本机 `target/tmp/goal-extension-release-licenses/`。
