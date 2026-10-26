---
name: implement-feature
description: 按已定 contract 实现 OpenRust 功能簇，适用于根 src 模块、driver、CLI、生成器及配套测试的新增或扩展。公共 API 仍需设计时先使用 codebase-design；不用于纯文档任务或只读审计。
---

# 实现一个 OpenRust 功能簇

## 确认范围

遵循根目录 `AGENTS.md` 的启动读取与 Spec → Implement → Verify → Commit。先核对 Git/WIP、当前执行校准和相关 ADR，确认能力没有已完成的实现。一个功能簇写明输入、可观察行为、失败语义、验收证据与排除项。

- 从 `Cargo.toml` 确认模块、workspace 成员和 feature；框架能力位于根 `src/`，不要为每项能力新建 crate，也不要重做已完成的迁移。
- 按能力名称读取 `.agents/project/core/` 对应文件、模块或包的 `README.md` 和源码。目录位置以当前工作区为准。
- 找到现有调用方、测试、模板和集成入口；已定 contract 出现矛盾时记录具体证据，先收敛受影响契约再继续依赖它的实现。

## 实现

- 插件以 `src/core/plugin.rs` 为准：同步 `Plugin::build` 注册，异步 `Plugin::finish` 初始化；服务注册参考 `AppBuilder::insert`。只在需要接入应用生命周期时实现插件。
- 配置、错误和扩展机制沿用当前模块与 `conventions.md`；先检查已有类型和公开导出，不另建同义 API 或平行 Hooks 系统。
- driver 实现现有公开 trait，允许外部包接入；不要把某个内置后端的实现细节写入公共契约。feature 名称从当前 manifest 读取，避免照搬假想名称。
- 修改公共行为时同步模块调用方、`apps/server` 集成、模板、示例、测试、用户文档及 changelog；只更新实际受影响的文件。
- 按已定行为逐项实现并验证；测试先行的细节见 [tdd](../tdd/SKILL.md)。单元 fake 只用于其能覆盖的层次。

## 验证与收口

从 `AGENTS.md`、`.agents/project/conventions.md` 和 `.agents/project/memory/testing.md` 读取当前门禁与可执行命令，不在技能内维护另一份版本矩阵。

- 源码变更执行 workspace fmt、clippy、tests、docs 与 cargo deny；按当前测试入口补齐必要 feature，默认 feature 的成功不能证明可选模块已测。
- 数据能力在 PostgreSQL 和 MySQL 各执行适用集成测试。复制、租约恢复、多实例与网络行为必须用对应真实拓扑验收；`oneshot` 只证明进程内 HTTP 行为。
- 模板或生成器变更先生成项目，再实际执行其 `cargo check` 和 `cargo test`；有服务依赖时检查相关测试确实执行，不能将跳过当通过。
- 复用已授权服务并隔离测试数据；缺少凭据或服务时继续独立工作，明确未验收项。只有满足当前功能簇完成条件才标记完成。
- 重读 diff，更新本簇相关的项目事实、待办与验证记录，按项目脚本创建限定路径的本地提交；不夹带其他 WIP。
