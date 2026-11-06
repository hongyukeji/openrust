# OpenRust 框架

OpenRust 是 Laravel 的 Rust 实现；Laravel Modules 是默认集成的优秀第三方扩展。框架库公开路径为 `openrust::`，根应用通过统一 Container、Provider 和配置生命周期接入。

框架源码位于 `src/lib.rs`；宿主应用、bootstrap 和唯一综合示例保留在仓库根目录。默认启动在根执行 `cargo run`。

功能与尚未完成的对应以[用户指南](../../../docs/src/SUMMARY.md)为准；独立框架包拆分已由 `44632383` 完成限定验收；后续命名空间迁移及全量 Laravel 能力仍须分别验收。
