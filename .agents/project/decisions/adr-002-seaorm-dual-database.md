# ADR-002 SeaORM 2 与 PostgreSQL / MySQL 双库支持

- 状态：Accepted
- 日期：2026-10-25

## 背景

框架需要同时覆盖 PostgreSQL 与 MySQL 以扩大适用环境。SeaORM 是唯一同时成熟支持两库并带迁移与实体生成的 Rust ORM；SQLx 的编译期检查宏绑定单一数据库，不适合作为跨库默认层。

## 决策

- ORM 与迁移使用 `sea-orm` 2.x + `sea-orm-migration`；SQLx 直连只通过 `db.sqlx()` 显式获取。
- 正式支持 PostgreSQL 14+ 与 MySQL 8.0+，PostgreSQL 为推荐默认；SQLite 不在第一代。
- 迁移只使用可移植列类型；时间 PostgreSQL `timestamptz` / MySQL `DATETIME(6)`，JSON `jsonb` / `json`；框架表不用数组、枚举等单库特性。
- 单库 SQL 集中在 `openrust-db::dialect`，必须同时提供两库分支并有双库测试；CI 以 service 容器运行双库矩阵。
- 生成项目在创建时选择一个后端并只启用对应 feature，不要求运行时切换。

## 影响

- 每个数据功能的完成定义包含双库证据。
- 测试隔离采用每用例独立数据库（PostgreSQL 模板库复制、MySQL 迁移结果重放）。

## 验证

M0 Spike A（2026-10-25）：独立项目用 `sea-orm` / `sea-orm-migration` 2.0.4（`sqlx` 0.9.0、`sea-query` 1.0.2，features `sqlx-postgres` + `sqlx-mysql` + `runtime-tokio-rustls`）对本机 PostgreSQL 18 与 MySQL 8.4 容器执行同一组迁移（`users`：bigint 自增主键、`varchar(191)` 唯一索引、JSON、时间；`posts`：JSON、时间、外键 `ON DELETE CASCADE`），用 `Migrator::up` / `down` / `down(Some(1))` / `status` 实际连库验证，结论：**可行**，stack.md 版本无需调整。

| 项 | PostgreSQL 18 | MySQL 8.4 |
| --- | --- | --- |
| 重复 `up` | 第二次无操作，`seaql_migrations` 仅 2 行 | 同左 |
| `down(None)` / `down(Some(1))` / 再 `up` | 逐个回滚并可重放，表与外键随之移除 | 同左 |
| 唯一索引 | 重复插入报 `uq_users_email` 违反 | 报 1062，`for key 'users.uq_users_email'` |
| 外键 | 违反报 `fk_posts_user`；删父行级联删子行 | 报 1452；级联删除同样生效（`COUNT(*)` 为 0） |
| JSON 列 | `jsonb`，规范化为 `{"a": 3, "z": 1}`（重复键后者胜、键重排） | `json`，同样规范化为 `{"a": 3, "z": 1}` |
| 时间列 | `timestamptz`（微秒，带时区） | 见下方差异 1 |
| 迁移失败 | DDL 在事务内，失败后 `half_done` 表与迁移记录一并回滚 | DDL 隐式提交，失败后 `half_done` 表残留，记录未写入 |

两库差异与规避：

1. **时间类型陷阱**：`ColumnDef::timestamp_with_time_zone()` 在 MySQL 生成的是 `timestamp`（无小数秒、范围止于 2038），不是 `DATETIME(6)`。迁移必须在 MySQL 分支用 `.custom("DATETIME(6)")`，默认值用 `Expr::cust("CURRENT_TIMESTAMP(6)")`（PostgreSQL 用 `timestamp_with_time_zone()` + `current_timestamp()`）。
2. **时区**：`DATETIME(6)` 不带时区，`now(6)` 取会话时区（本机 `SYSTEM` 为 `Asia/Shanghai`，实测写入 16:07 而 UTC 为 08:07）；`timestamptz` 内部存 UTC，仅显示随会话时区。因此框架必须由应用写入 UTC 值（`chrono::Utc`），并在 MySQL 连接上固定会话 `time_zone = '+00:00'`；不依赖库端默认时间函数的时区。
3. **JSON 类型映射**：`json_binary()` 在 PostgreSQL 为 `jsonb`、在 MySQL 为 `json`；`json()` 在 PostgreSQL 是文本 `json`。迁移按 `DbBackend` 分支选择，不能共用一个调用。JSON 列均不可建普通索引，查询语法（`->>` 与 `JSON_EXTRACT`）不同，只能放在 `dialect`。
4. **迁移原子性**：PostgreSQL 单个迁移失败可整体回滚，MySQL 不能。迁移保持“一个迁移一个主题、尽量单语句”，MySQL 失败后需人工清理残留对象，迁移写成幂等（`if_not_exists` / `if_exists`）以便重跑。
5. **外键索引**：MySQL 自动为外键列建同名普通索引（`KEY fk_posts_user`），PostgreSQL 不会；需要按外键查询时 PostgreSQL 分支显式 `create_index`。
6. **标识符与长度**：MySQL `utf8mb4` 唯一索引受 3072 字节限制，唯一字符串列用 `varchar(191)` 起步更稳；约束名在 PostgreSQL 为库内唯一、MySQL 为表内唯一，统一显式命名 `uq_<table>_<col>` / `fk_<table>_<ref>` 避免冲突。

对 `openrust-db` 的约束建议：

- 提供迁移辅助（`dialect::timestamp_utc(name)`、`dialect::json(name)`），封装上述时间与 JSON 分支，框架表与生成器模板只经过辅助函数，禁止直接调用 `timestamp_with_time_zone()`。
- 连接层在 MySQL 上设置 `time_zone = '+00:00'`，业务代码统一使用 `chrono::Utc`。
- 迁移测试同时断言 `information_schema` 中的列类型（`timestamp with time zone` / `datetime(6)`、`jsonb` / `json`），防止回退到 `timestamp`。
- 迁移 CLI 只承诺 `up`、`down`、`status` 幂等；失败恢复策略对 MySQL 文档化为“需人工清理”。
