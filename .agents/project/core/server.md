# 根应用事实

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：根应用提供通用骨架和空模块清单。综合业务模块在 example 分支。

根应用 package `openrust` 依赖独立框架包，由薄入口 `main.rs` 提供二进制 `app`。`default-members = ["."]`、`default-run = "app"`，根无参数 `cargo run` 启动唯一应用；app/bootstrap/config/database/public/resources/routes/storage/tests 直接位于根，按根工作目录读取配置和资源。

综合示例及其业务 Provider、资源、迁移和测试随 `example` 分支维护。根应用继续使用同一模块清单和 Provider 生命周期。

本簇保持 openrust:: 公共入口及底层已确认行为，变更目录、应用装配与路径。旧 R1-S 布局依据 历史决策 073，当前模块边界依据 [ADR-001](../decisions/adr-001-framework-principles.md)；当前待办见 [todo](../todo.md)。R1-S 与 R2-D 的旧验收仅作历史证据，R2-M 的模块激活、实际迁移与运行根验证单独记录于 [测试基线](../memory/testing.md)。

当前应用职责子簇已限定验收：根应用Controller提供信息API，bootstrap::routes配置统一api前缀/组，routes/mod只导出，框架库移除专属业务端点。原bootstrap测试主体归tests/server/bootstrap.rs，保私有cfg/path与资源根断言；模块关闭/宿主API/stateless及通用HTTP无应用端点有真实测试。生成器、认证/MFA/Job及两consumer同步，完整适用门禁与fresh/MySQL/真实TCP通过；见[本簇证据](../memory/testing.md#应用启动与固定api宿主职责限定验收)。完整模块持久管理与DB门控继续开放。
