# ADR-016 中性应用配置命名空间与数据库表前缀

## R2-ENV 当前获准命名目标（待R2-R02提交后实施）

用户最新选择覆盖本文较早的端口/缓存环境命名：`SERVER_PORT`→`server.port`，`APP_PORT`及独立`PORT`退出监听配置，不保留通用或命名空间fallback兼容；自定义应用命名空间不改变中性SERVER_PORT。根/生成`.env.example`使用`SERVER_PORT=3000`与`APP_URL="http://localhost:${SERVER_PORT}"`。

缓存按Laravel官方`CACHE_STORE`→`cache.default`，内部字段、序列化、读取与错误路径同步default；删除REDIS_DEFAULT_STORE，不引入已撤回的CACHE_DEFAULT_STORE别名。默认具名redis且driver=redis，业务自定义名default合法；缓存与队列共用REDIS_DB，不增REDIS_CACHE_DB。默认Redis/共享DB是已明确授权的部署选择，与上游原生默认的差异如实记录；不扩改其他DB_*键。

本段是已定contract，不宣称当前代码已切换。当前R2-R02冻结先验收提交，再由[唯一todo](../todo.md)领取R2-ENV，完成来源优先级、dotenv插值、CLI/模板/consumer/唯一模块、当前文档、负例和真实启动/Redis验收后本地提交。旧测试与验收事实保留，不热改在途listener或真实密钥。

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-25
- 替代：ADR-006 的品牌化环境变量契约；框架持久表的品牌化命名按本 ADR 迁移。crate、Rust 模块与 CLI 产品名不因本决定变更。

## 决策

现行端口键已按统一命名校准为 `APP_PORT` / `<命名空间>PORT`，内部仍映射 `server.port`；不保留旧环境别名。

应用部署配置默认使用 `APP_` 命名空间，数据库表默认使用 `tb_` 前缀；两者均可由应用显式定制，不强制开发者在业务部署资产中使用框架品牌。

### 环境变量

- 默认 `APP_ENV`、`APP_CONFIG_DIR`，应用字段使用单下划线扁平名称，例如 `APP_PORT`、`APP_DATABASE_URL`、`APP_LOG_LEVEL`；常用能力仍采用领域变量，如 `DB_CONNECTION`、`REDIS_DB`、`QUEUE_DRIVER`、`MAIL_DRIVER`。字段路径由框架显式映射，不要求开发者编码 TOML 层级。
- 应用可在配置加载前通过显式启动参数/构建 API 指定命名空间，例如 `SHOP_`，对应 `SHOP_ENV`、`SHOP_CONFIG_DIR`、`SHOP_PORT`。不能依赖尚未加载的 TOML 或 `.env` 动态决定读取自己的命名空间。
- 命名空间默认 `APP_`，定制值必须有效且非空，启动时校验。一个应用只读取选定命名空间，不自动混读 APP_/SHOP_/OPENRUST_，也不回退 RAILS_ENV/NODE_ENV 等变量。
- `PORT` 作为通用监听端口简写保留；数据库、Redis、队列、邮件、短信和国际化使用领域前缀环境变量。单下划线应用变量按固定字段表映射，不再解析双下划线层级写法；不提供 `DATABASE_URL` / `REDIS_URL` 通用环境别名。TOML 仍使用自然的区段与字段层级。
- 配置分层为 crate 类型默认值 → 根 `config/*.toml` 按文件名排序的领域配置 → `.env` → 进程环境变量。运行时 TOML 使用 `app.toml`、`database.toml`、`cache.toml` 等领域文件，不按环境命名；环境差异通过 `.env` 或部署环境变量设置。环境仍为 development/test/production，缺省 development。
- 环境选择仍为进程变量优先；未设置时从 `.env` 引导读取所选命名空间的 ENV。所有环境（包括 production）均可加载应用工作目录的 `.env`，进程环境变量不被文件覆盖，并在配置源中保持最高优先级。格式错误或文件不可读时启动安全失败，不回显文件内容。命名空间、配置目录与环境引导必须避免循环依赖。
- 当前 0.0.0 开发期优先统一迁移代码、模板、运行示例与文档，不默认为 OPENRUST_ 增加永久兼容别名。实施时盘点调用方，说明旧变量不再读取及迁移办法；测试专用变量、工具内部变量逐项判断，不机械全仓字符串替换。

### 本地环境文件

- `.env` 是各环境可选的配置覆盖来源，适用于本地开发，也允许部署在生产应用根目录；项目继续保留分层 TOML 作为可审查的非敏感默认配置。部署平台注入的进程环境变量优先于 `.env`。生产使用文件时，运维须限制文件读取权限、避免将应用根目录暴露为静态站点，并保证集群节点配置一致；部署平台 secret store 仍是更适合集中管理和轮换敏感值的方式。
- 生成器提供 `.env.example`，仅包含变量名、说明和无效占位值；开发者复制为 `.env` 后填写本机值。新增配置键时同步更新示例，不在示例或受版本控制的 TOML 中放真实密钥、密码、个人连接串。
- 仓库根目录与生成项目的 `.gitignore` 忽略 `.env`、`.env.*`，明确放行 `.env.example` 与 `.env.*.example`；提交前检查实际 ignore 结果，避免本地密钥被纳入 Git。
- `.env` 是配置来源，不是集中式生产密钥管理或集群分发系统；需要跨节点一致的密钥仍应由部署系统安全分发，并通过现有密钥轮换能力管理应用数据密钥。

### 数据库表

- 建议使用 `[database] table_prefix = "tb_"`；缺省即 `tb_`，显式空字符串表示不加前缀，允许 `shop_` 等自定义值。最终字段与连接实例布局在实现前核对已有配置结构。
- 默认示例：`tb_users`、`tb_sessions`、`tb_personal_access_tokens`；后续框架持久表采用中性逻辑名并统一解析。此处为目标命名，不代表当前表已重命名。
- 表前缀与显式表名映射结合使用：先查应用显式映射的完整物理表名，未映射时才按 prefix + 逻辑表名生成，避免重复加前缀。已有业务用户表可明确映射，不为接入框架额外复制用户数据。
- 前缀不是 schema、数据库名或租户隔离机制。只接受受限标识符字符，不允许点、引号、SQL/路径片段；前缀、映射和最终标识符长度按目标数据库校验，并通过 ORM/查询构建器正确引用，不拼接未经校验的 SQL。
- 建表、查询、关联、索引/唯一约束、外键、seed、清理命令、生成器与测试隔离共用同一命名解析。多连接配置隔离；索引/约束名称遵守双库长度/作用域规则，可采用稳定短名，不能靠截断制造碰撞。
- SeaORM 静态实体、运行时映射与迁移如何共用命名，必须先验证可行性。框架自有表、生成项目与受管理查询明确覆盖；手写实体/原生 SQL 需显式使用相同映射，不承诺自动重写所有 SQL，也不另造 ORM。
- 迁移历史表、已运行迁移 ID 与应用表是不同契约，实施时单独核查，不将旧 migration ID 全局替换。不得把改前缀视为启动时自动改名/复制/删表；运行过旧 schema 的环境需要显式、可验收的数据迁移方案。
- 迁移后前缀/映射变更不应被当作无害热配置；缺失目标表或命名冲突明确报错，不偷偷建立第二套空表。不能用配置切换绕过唯一性或权限验证。

## 实施与验收

- 当前认证 WIP 保留，当前簇 schema/生成物最终验收前必须核对本 ADR；配置/命名按独立跨 crate 功能簇实施，不把文档决定冒充代码已改。
- 配置验证默认 APP_、自定义命名空间、通用变量覆盖优先级、错误命名空间、production `.env` 加载与进程环境覆盖、无效文件安全失败以及旧前缀不误读；同步 CLI 子进程、根目录集成宿主与生成模板。生成项目验证 `.gitignore` 忽略真实环境文件并保留示例文件。
- 在 PostgreSQL/MySQL 验证默认 tb_、空前缀、自定义前缀、完整表映射、索引/外键/关联、迁移 up/down、认证/Session/PAT、冲突/非法名称和既有数据迁移。验证命名不能导致越权、平行用户表或错误数据库访问。
- 代码实现后执行全量 Rust 门禁、双库与生成项目检查，更新 core/README/测试基线与迁移说明。改变表名前缀或映射不自动重命名现有数据库对象。

## 当前实现状态

- 默认 `APP_` 配置读取、`Application::config_namespace()` 自定义前缀、环境与 `.env` 引导、配置目录选择及前缀变量覆盖已在 `openrust-core` 实现；CLI、仓库启动配置、生成模板和用户文档已迁移到该 contract。未保留 `OPENRUST_` 运行时兼容别名。测试工具专用 `OPENRUST_TEST_*` 变量属于独立命名空间。
- 表前缀/映射 contract 已在 `openrust-db` 实现：`DatabaseConfig` 及 `Db` 持有 resolver；`MigrationPlugin` 与 `TestDatabase` 使用显式 task-local scope；AuthMigration 和认证 Session/PAT/refresh 查询已接入 resolver。历史 PostgreSQL 14 与 MySQL 8.0 真实隔离库均验证默认、空前缀、自定义前缀、完整 users 映射和认证流程；独立空库验证迁移 runner up/status/down。无 URL 时按设计跳过的测试入口不作为数据库验证证据。

### 迁移期表名解析器 contract

- `DatabaseConfig` 提供 `table_prefix`（缺省 `tb_`，空值表示无前缀）与 `table_names`（逻辑名到完整物理名映射）。显式映射优先于前缀；映射值不再加前缀。
- `Db` 持有启动时验证后的不可变 resolver；业务受管查询和 seed 用 `Db::table_name` / `Db::quoted_table_name`，不散布 backend 类型判断。
- SeaORM 的 `MigrationTrait::up/down` 只有 `SchemaManager` 参数，不能直接收到 `Application` 或 `DatabaseConfig`。`MigrationPlugin` 在一次迁移调用内用 Tokio task-local scope 提供当前 `DatabaseTableNames`；OpenRust 内置迁移在没有 scope 时明确失败。第三方 migration 需要自定义表名时调用 `DatabaseTableNames::current()`。
- `testing::TestDatabase` 默认使用默认 resolver，并提供传入 resolver 的隔离数据库入口；它与生产 migration runner 采用相同 scope，避免测试 schema 与应用查询不一致。
- 标识符限定为 ASCII 字母/下划线开头，后续只含字母/数字/下划线，不允许 schema 分隔符或 SQL 片段；连接初始化按 PostgreSQL 63-byte 与 MySQL 64-byte 表名限制校验。生成器与框架查询按 backend 正确引用标识符。
- 前缀和映射只影响之后执行的迁移与访问，不自动改名现有表、索引或数据。手写实体和原生 SQL 不自动重写，使用方必须显式采用同一 resolver；无配置时保留当前 `tb_*` 物理名。
