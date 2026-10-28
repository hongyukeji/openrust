# ADR-071：应用职责目录与独立框架库

- 状态：Accepted；实现与实际验收以当前代码和验证记录为准
- 日期：2026-10-28
- 前置：[ADR-070](adr-070-laravel-first-rebuild.md)

## 决策

Laravel 13 应用职责目录为默认布局：app、bootstrap、config、database、routes、resources、public、storage、tests。根 `src/main.rs` 是唯一启动入口，通过编译期 `#[path]` 引用职责模块。框架库 `openrust` 移到 `framework/`；根 package 为 `application`，只保留一个 `server` bin。根 `cargo run` 无需选 package/bin。CLI、macros、testing、performance 保留独立编译边界。

独立生成应用使用相同职责目录、自有 workspace 和本地 framework/tool 依赖，不再强制 apps/<name> 或修改源 workspace。综合示例移到 examples/example；公开 consumer 保留 tests/consumers，所有 path dependency 同步。可发布 registry 依赖不是本次源码交付的前提。

## Rust 与工具取舍

顶层 `#[path]` 与 src 内模块均由 Cargo 静态编译，前者更贴近 Laravel 职责目录；不使用运行时源码扫描，也不建立转发兼容目录。薄入口保留断点位置，cargo metadata、fmt、check/test 和实际 HTTP 用来验证 Cargo 成本；RustRover 2026.2.4 同一 Command=run 的运行/断点已实际 UI 验收；暂停于 src/main.rs:12，恢复后真实 HTTP 成功。

本阶段迁移位置与装配职责，不重写 Application/AppBuilder/Plugin、HTTP、数据库和认证语义。框架既有单元测试留在 framework/src，现有集成/真实服务测试与 consumer 保留；后继 R2 起再按领域 contract 收敛公开 API。

## 替代关系

替代 ADR-007/021/039/062–065 中与本布局、package 与生成位置冲突的约束。历史行为/真实服务证据继续保留；不兼容旧路径，不保留双框架/双默认入口。源码分发依然保留 Cargo、Rust 类型/async/IDE，不引入 PHP 运行环境。

## 验证

workspace fmt/clippy/test/doc、cargo deny、适用 feature/MSRV；独立生成应用 check/test/HTTP，数据路径双库回归；元数据确认一个根 bin、框架 library、独立 tools。IDE run/debug 另记实际证据，不以命令行推断。
