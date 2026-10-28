# 根应用事实

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：根 Laravel 应用与唯一业务模块已落地；R1-S 结构子步骤已提交，modules/example 经当前 R2-M 模块清单接入；综合 CRUD/能力贯通仍未完成。

根 package openrust 同时提供 src/lib.rs 框架库与默认 server 薄入口 src/main.rs。default-members=["."]、default-run="server"，根无参数 cargo run 启动唯一应用；app/bootstrap/config/database/public/resources/routes/storage/tests 直接位于根，按根工作目录读取配置和资源。

唯一综合示例 modules/example 的薄 provider 使用当前公开 ServiceProvider，由根 bootstrap 调用 modules/mod.rs 编译清单和状态激活器统一接入；模块自有 routes/api.rs、routes/web.rs 贡献业务路由，根 routes 仅框架通用路由。模块内部 providers/http/database/resources/tests 按实际业务规范组织，未有实现不建空models/services。它是贯穿框架能力的目标示例，目前仅已有认证/队列/页面与基本模块路由，完整 CRUD/能力覆盖仍待交付；不保留独立示例 bin/package。示例有效业务、迁移、种子、资源、协议/生成脚本和专属 fixture 全部归 modules/example；根职责目录只保留框架通用结构与公开挂载，不存示例 build.rs/build 依赖或样例特判；tests/server 承担真实进程与服务回归，tests/consumers 承担公开 API/独立拓扑验收。

本簇保持 openrust:: 公共入口及底层已确认行为，变更目录、应用装配与路径。旧 R1-S 布局依据 [ADR-073](../decisions/adr-073-root-application-and-example-module.md)，当前模块边界依据 [ADR-074](../decisions/adr-074-laravel-modules-directory.md)；当前待办见 [todo](../todo.md)。R1-S 与 R2-D 的旧验收仅作历史证据，R2-M 的模块激活、实际迁移与运行根验证单独记录于 [测试基线](../memory/testing.md)。
