# ADR-005 许可证、crate 命名、Edition 与 MSRV

- 状态：Accepted
- 日期：2026-10-25

## 背景

Rust 生态惯例为 MIT OR Apache-2.0 双许可；crates.io 名称先到先得。2026-10-25 核实 `openrust` 与 `openrust-core` 均未被占用。

## 决策

- 许可证 MIT OR Apache-2.0；仓库根目录保留 `LICENSE-MIT` 与 `LICENSE-APACHE`。
- crate 命名：门面 `openrust`，全局工具 `openrust-cli`（二进制名 `openrust`），能力 crate `openrust-<capability>`。开发者在 M0 以 0.0.0 占位版本预留名称。
- Edition 2024；MSRV 为当前 stable 减两个小版本，写入 workspace `rust-version` 并由 CI 验证。
- 0.x 期间破坏性变更只在次版本号；发布前运行 `cargo semver-checks`。
- 仓库 `https://github.com/hongyukeji/openrust`，默认分支 `main`。

## 影响

- 贡献默认按双许可授权（README 已声明）。
- 依赖主版本升级与 MSRV 提升需新 ADR 或 changelog 条目。
