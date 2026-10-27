# ADR-007 聚合启动与集成验收宿主

- 状态：Partially Superseded（聚合集成验收职责保留；宿主 package/路径由 [ADR-062](adr-062-root-server-entry.md) 迁入根 package）
- 日期：2026-10-25

## 背景

初始多 package 结构需要一个可运行入口验证 framework facade、配置、Plugin 和外部依赖组合。`apps/server` 因此作为内部集成宿主建立，不是用户必须安装的后台或业务应用。

## 当前有效决定

- 新能力须有经公共 `openrust` API 组装的应用级集成测试；不能只通过模块单测或声明 feature 就宣称开箱即用。
- 根 package 的 `server` bin 是 `publish = false` 的集成宿主。主程序安装 HTTP、I18n、Mail/SMS 和可选 Encryption；数据库/auth/queue/schedule 等由各测试 Application 显式组装，默认 serve 不自动启动 worker 或外部依赖。
- 需要外部服务的能力通过 feature 转发、插件/迁移注册和真实服务验收接入；不为未实现能力创建空 feature。
- 宿主只承载框架验证，不承载后台组织模型或独立产品业务。按 [ADR-039](adr-039-source-workspace-distribution.md)，用户业务默认在整套源码的 `apps/<name>/` 开发，保留应用自己的 `src/` 和根 `config/`；根 `server` bin 只改变框架集成宿主的位置，不改变这些验收职责。
- 项目根 `cargo run`、按目标应用定位配置、HTTP smoke、双库测试、独立子进程信号与崩溃测试均属于要维持的职责。

## 与目录简化的关系

根模块与工具目录统一按 ADR-021；本 ADR 不再规定逐能力 crate 或旧示例路径。`apps/` 是应用目录约定，`apps/server` 是可在职责替代后移除的具体宿主。

当前目录结论由 ADR-062 更新：`server` 二进制和聚合集成职责保留在根 package，测试位于根 `tests/server/`，配置位于根 `config/`。不得恢复单独的 `apps/server` package；验收职责仍须保留。

## 完成条件

1. 公共 API 的实际组装和成功/失败行为有集成测试。
2. 数据能力在 PostgreSQL/MySQL 真实隔离库执行，非数据库依赖按各 driver contract 验证。
3. 命令生命周期按真实进程测试，不以 helper/fake 代替信号、租约和故障恢复。
4. feature、配置、测试入口、用户 README 与 Agent 事实同步；宿主若被替代，同步更新 `AGENTS.md` 完成定义。
