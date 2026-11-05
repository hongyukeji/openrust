# testing

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：测试工具 crate 与根公开测试宿主当前实现边界。

## 当前实现

- `TestDatabase::create::<M>(admin_url)` 为一次测试创建唯一数据库并运行应用迁移。
- PostgreSQL 用临时迁移模板执行数据库克隆；MySQL 每次新建数据库并运行迁移。MySQL 连接强制 TLS，session 时区设为 `+00:00`。
- 隔离 URL 保留原 URL 凭据与查询参数，只替换 database path。管理用户必须有 CREATE/DROP DATABASE 及远端归属核验权限，具体要求见 tools/testing/README.md；仅支持直连唯一可写主库。
- 网络生命周期/持久恢复的Unix范围已收齐限定验收，本地提交以Git为准：独立I/O runtime和短寿命执行者负责close/Drop/取消；正常退出最多30秒票据屏障。私有永久文件锁与原子journal、每资源随机marker、实际服务身份和原生物理连接advisory lock共同核验；目标/模板分别持责。业务池每条物理连接及重连验证服务/库名/marker，PG含OID。强杀后的下一次显式create或recover仅凭同一稳定私有目录的回执补偿，绝不按前缀/时间/PID认领。
- 池关闭未确认、归属错配、权限/活动可见性不明或外部池仍活动均保留。MySQL完整threads/lost计数与最终空闲复核不排斥不合作的新借用；调用方必须先关闭独立应用池并停止新借用。不合作管理员DDL、完整复制marker的同名备份、物理克隆同服务ID和非Unix ACL实现仍是公开边界。
- 当前双库各10场永久公开API进程测试实际通过，含SIGKILL/recover及保护负例；库清单完全恢复。MySQL例程权限拒绝回收及受限账号不误记Done亦有实际正反证。默认/全特性组合、单feature/SQLite、MSRV和依赖门禁已过；唯一示例真实双库就绪流程各1通过，旧退出22场仅按未变执行路径限定复用，输入与边界见[验收](../memory/testing.md#网络测试数据库生命周期与持久恢复限定验收)。
- `Debug` 遮蔽 database URL。
- `openrust::testing::{TestApp, TestAppBuilder, TestResponse}` 由根框架公开；TestApp 使用生产 Application/Provider/Router 构建路径，支持 Provider 移除、全 register 后 typed 覆盖、重建、有界 oneshot 请求和 HTTP 状态/header/JSON 断言。它不替代真实 TCP、浏览器或多进程验收。
- 独立 `testing` package 位于 `tools/testing/`，保持 `testing::TestDatabase` 应用入口；实现由根 `openrust::testing` 提供。
- SQLite通过共享内存或自有临时目录构造隔离，拒绝接管用户文件；显式关闭失败、取消或未poll时保留目录。网络清理改动不覆盖该保护。其他SeaORM backend仍需独立隔离策略，不因`Db::initialize()`可接入推断兼容。

## 后续边界

Factory/model factory states 仍是 [R6](../plans/laravel-rebuild.md#5-按依赖实施) 待实现能力；不把 SeaORM Entity 或 TestDatabase 冒充 Laravel Factory。Fake driver assertions 随具体能力的公开 contract 实现，当前应使用模块 TestApp 显式安装捕获 driver 并拒绝 production。

## 验证

`tests/database.rs` 历史验证曾在 PostgreSQL 14 和 MySQL 8.0 上分别创建两个实例，验证迁移表存在、数据彼此隔离并显式删除数据库。CI 的 MySQL 隔离步骤使用 root admin URL；其他普通集成测试使用最低权限 `openrust` 用户。
