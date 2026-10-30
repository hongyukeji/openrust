---
name: implement-feature
description: 按已定 contract 实现 OpenRust 功能簇，适用于框架模块、driver、CLI、生成器及配套测试的新增或扩展。公共 API 仍需设计时先使用 codebase-design；不用于纯文档任务或只读审计。
---

# 实现一个 OpenRust 功能簇

## 确认范围

遵循根目录 `AGENTS.md` 的启动读取与 Spec → Implement → Verify → Commit。先核对 Git/WIP、当前执行校准和相关 ADR，区分既有实现与新目标，确认复用、适配或重写范围。一个功能簇写明输入、可观察行为、失败语义、验收证据与排除项。

- 从 `Cargo.toml` 确认模块、workspace 成员和 feature；源码位置与目标布局以当前获准 ADR 为准；全面重构允许重做迁移，避免无必要的逐能力拆包。
- 按能力名称读取 `.agents/project/core/` 对应文件、模块或包的 `README.md` 和源码。目录位置以当前工作区为准。
- 找到现有调用方、测试、模板和集成入口；已定 contract 出现矛盾时记录具体证据，先收敛受影响契约再继续依赖它的实现。

## 实现

- 生命周期按本簇已定 contract 实现；迁移前核对 Plugin/AppBuilder，迁移到 Provider/Container 时收敛到一套内核，不长期维护平行装配。
- 配置、错误和扩展机制沿用当前模块与 `conventions.md`；先检查已有类型和公开导出，不另建同义 API 或平行 Hooks 系统。
- driver 实现现有公开 trait，允许外部包接入；不要把某个内置后端的实现细节写入公共契约。feature 名称从当前 manifest 读取，避免照搬假想名称。
- 修改公共行为时同步模块调用方、当前可运行应用集成、模板、示例、测试、用户文档及 changelog；只更新实际受影响的文件。
- 先完成本簇源码、测试代码、调用方与文档，再在提交前集中执行验证；实现期间不逐小改动反复跑测试/Clippy/整组门禁，不默认测试先行。确有类型/API阻断时只做最小必要编译诊断；用户明确要求TDD时才按 [tdd](../tdd/SKILL.md) 执行。单元 fake 只用于其能覆盖的层次。

## 验证与收口

从 `AGENTS.md`、`.agents/project/conventions.md` 和 `.agents/project/memory/testing.md` 读取当前门禁与可执行命令，不在技能内维护另一份版本矩阵。

- 提交前按`conventions.md`集中验证：分支执行本簇定向与适用真实集成门禁；main功能簇/集成执行workspace fmt、clippy、tests、docs与cargo deny及必要feature/MSRV。默认feature成功不能证明可选模块已测；分支定向通过不冒充main全量验收。
- 数据能力在 PostgreSQL 和 MySQL 各执行适用集成测试。复制、租约恢复、多实例与网络行为必须用对应真实拓扑验收；`oneshot` 只证明进程内 HTTP 行为。
- 模板或生成器变更先生成项目，再实际执行其 `cargo check` 和 `cargo test`；有服务依赖时检查相关测试确实执行，不能将跳过当通过。
- 复用已授权服务并隔离测试数据；缺少凭据或服务时继续独立工作，明确未验收项。只有满足当前功能簇完成条件才标记完成。
- 重读 diff，更新本簇相关的项目事实、待办与验证记录，按项目脚本创建限定路径的本地提交；不夹带其他 WIP。
