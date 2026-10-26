# ADR-005 许可证、crate 命名、Edition 与 MSRV

- 状态：Partially Superseded（许可证、Edition、MSRV 与源码版本原则有效；包命名和注册表分发由 [ADR-039](adr-039-source-workspace-distribution.md) 替代）
- 日期：2026-10-25

## 背景

Rust 生态惯例为 MIT OR Apache-2.0 双许可；crates.io 名称先到先得。2026-10-25 核实 `openrust` 与 `openrust-core` 均未被占用。

## 决策

- 许可证 MIT OR Apache-2.0；仓库根目录保留 `LICENSE-MIT` 与 `LICENSE-APACHE`。
- 产品名为 OpenRust，根 package 与公共路径保留 `openrust`；整套源码交付、内部中性包名和禁用发布由 [ADR-039](adr-039-source-workspace-distribution.md) 决定。源码目录与宿主职责继续按 ADR-021。
- Edition 2024；MSRV 为当前 stable 减两个小版本，写入 workspace `rust-version` 并由 CI 验证。
- 0.x 期间破坏性变更只在次版本号；源码版本升级记录 API 兼容性，`cargo semver-checks` 使用明确的源码基线，不依赖 crates.io 发布记录。
- 仓库 `https://github.com/hongyukeji/openrust`，默认分支 `main`。

## 影响

- 贡献默认按双许可授权（README 已声明）。
- 依赖主版本升级与 MSRV 提升需新 ADR 或 changelog 条目。
