# ADR-007 聚合启动与集成验收宿主

- 状态：Partially Superseded（集成验收职责保留；原多能力 crate 拓扑及永久保留 server 的要求由 [ADR-021](adr-021-loco-style-module-layout.md) 替代）
- 日期：2026-10-25

## 背景

初始多 package 结构需要一个可运行入口验证 framework facade、配置、Plugin 和外部依赖组合。`apps/server` 因此作为内部集成宿主建立，不是用户必须安装的后台或业务应用。

## 当前有效决定

- 新能力须有经公共 `openrust` API 组装的应用级集成测试；不能只通过模块单测或声明 feature 就宣称开箱即用。
- 当前 `apps/server` 是 `publish = false` 的 `openrust-server` package。主程序安装 HTTP、I18n、Mail/SMS 和可选 Encryption；数据库/auth/queue/schedule 等由各测试 Application 显式组装，默认 serve 不自动启动 worker 或外部依赖。
- 需要外部服务的能力通过 feature 转发、插件/迁移注册和真实服务验收接入；不为未实现能力创建空 feature。
- 宿主只承载框架验证，不承载后台组织模型或独立产品业务。用户应用由 CLI 生成独立项目，保留自己的 `src/` 和根 `config/`。
- 项目根 `cargo run -p openrust-server`、配置查找、HTTP smoke、双库测试、独立子进程信号与崩溃测试均属于要维持的职责。

## 与目录简化的关系

根模块与工具目录统一按 ADR-021；本 ADR 不再规定逐能力 crate 或旧示例路径。`apps/` 是应用目录约定，`apps/server` 是可在职责替代后移除的具体宿主。

当前评估仍保留 server：根 HTTP 测试和 hello 示例尚不覆盖真实二进制、配置目录、双库迁移/认证/队列、feature forwarding 及进程故障验收。若后续迁移，必须逐项给出等价入口、测试发现/用例映射与通过结果；不能先删除宿主再补验证，也不能仅改名为 example 宣称完成整合。详见 [目录收尾 Spec](../plans/module-layout-migration-spec.md)。

## 完成条件

1. 公共 API 的实际组装和成功/失败行为有集成测试。
2. 数据能力在 PostgreSQL/MySQL 真实隔离库执行，非数据库依赖按各 driver contract 验证。
3. 命令生命周期按真实进程测试，不以 helper/fake 代替信号、租约和故障恢复。
4. feature、配置、测试入口、用户 README 与 Agent 事实同步；宿主若被替代，同步更新 `AGENTS.md` 完成定义。
