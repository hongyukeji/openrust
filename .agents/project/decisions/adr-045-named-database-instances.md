# ADR-045 命名数据库连接实例

- 状态：Accepted
- 日期：2026-10-26
- 依据：ADR-002、ADR-009、ADR-016、ADR-039

## 背景

`Db` 和 `DatabasePlugin` 当前管理一个主连接及其只读副本。副本承担同一数据库的读取扩展，不是彼此独立的数据库；应用需要把独立业务数据放入另一数据库时，不能重复注册 `Db`，也不能共享表名配置或连接池。

## 决策

- 保留既有 `[database]`、`DatabasePlugin`、`Db` 和所有单库配置。`AppState::try_get::<Db>()` 仍返回默认连接，现有 `DatabasePlugin` 继续只注册一次。
- 新增 `[database.instances.<name>]` 配置。每个命名实例完整声明 `url`、池限制、连接超时、表前缀/映射与可选 `read_replicas`；不从默认项或其他实例继承配置。秘密字段和 `Debug` 输出必须脱敏。
- 实例名匹配 `[a-z][a-z0-9_-]{0,62}`，`default` 为保留名称；非法名称和缺失/无效 URL 在启动时失败，错误保留配置键路径但不包含 URL、用户名或密码。
- 新增应用作用域的 `Databases`，仅用于选择 `Db` 句柄：`default()` 返回与现有 `Db` 相同的 handle，`get(name)` 取得具名 handle，`names()` 列出已初始化名称。未知名称返回配置错误。注册后集合只读；不提供全局静态查找或任意类型服务定位，也不承诺自定义具名 driver 注册。
- 默认与具名 handle 各自持有独立主连接池、副本池、表名 resolver 和事务 scope。副本仅属于其声明的实例；不在不同实例之间路由或回退。事务始终固定到所选实例的主库。
- `MigrationPlugin<M>` 保持现有默认行为，并提供显式实例选择。命令 `migrate --database <name> [up [steps]|down [steps]|status]` 只对所选实例执行 `M`；省略选项继续选择 default。所有 migration 都由应用显式注册，不自动遍历全部实例。
- `RateLimiter`、认证、Session、审计与数据库队列当前仍固定使用默认 `Db`；不因请求选中某实例而切换这些框架服务。跨数据库事务、自动查询路由及不同实例间数据复制不在本决策范围。
- 默认 database 功能和表解析继续遵守 ADR-002；具名连接仍只支持 PostgreSQL 与 MySQL，其他后端不能由 handle 可创建推断获得框架支持。

## 验收

- 默认单库配置与现有 API 不变；`Databases::default()` 与 `Db` 指向相同连接。
- PostgreSQL 18 与 MySQL 8.4 各自使用真实独立隔离库，启动一个 default 和至少两个 named 实例；断言连接身份、表名前缀、只读副本、事务写入与 migration 目标不串用。
- 配置项包含用户名/密码时，Config/Debug/启动错误不泄漏 URL；非法、重复/保留名称及未知名称失败关闭。
- 生成项目、server 聚合入口、默认/all-features workspace 门禁通过；没有声明全拓扑或跨数据库原子性验收。
