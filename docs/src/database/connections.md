# 数据库连接

应用的可编辑默认值位于 `config/database.rs`，部署值位于 `.env`。标准 Provider 在同一 Container 中提供 `Db` 和 `Databases`；当前在启动时建立已配置连接，完整 Laravel 懒连接仍待实现。

## 兼容目标与运行验收

固定Laravel最低兼容目标为MariaDB10.3+、MySQL5.7+、PostgreSQL10.0+、SQLite3.26.0+和SQL Server2017+。这些是实现目标，当前尚未全部交付：连接版本门禁仍为PG14/MySQL8，MariaDB差异与SQL Server驱动仍有缺口。日常实际数据库验收只用MySQL8.4；发布前完整验证全部承诺类型与最低/当前版本。日常通过不代表五库完整兼容。

## SQLite 连接

启用 `db-sqlite` 后可使用标准 Provider，无需自定义连接器。连接器按固定官方最低 SQLite 3.26.0 校验；连接、既有迁移清单及所有者边界已在 3.26.0 和当前 3.51.3 引擎验证；这不代表全部业务 SQL 已支持最低版本。默认 MySQL 与 Redis 不变。

```toml
openrust = { package = "openrust-framework", path = "/path/to/openrust/vendor/openrust/framework", version = "0.0.0", features = ["db-sqlite"] }
```

```dotenv
DB_CONNECTION=sqlite
DB_DATABASE=/absolute/path/database.sqlite
DB_FOREIGN_KEYS=true
```

普通数据库文件须先存在；未指定 `DB_DATABASE` 时使用应用根下的 `database/database.sqlite`。相对文件先按当前工作目录解析，失败后按应用根解析。连接器不会为普通缺失路径创建文件。`openrust new blog --db sqlite` 的安装路径会创建自有文件及 `.env`，然后运行默认迁移；迁移失败返回非零并保留生成文件。安装迁移、生成 check/test 与无状态 API 已验证；原生/Guarded数据库Session已完成限定验收；本地认证/MFA的机制与未支持范围见[认证边界](../../../vendor/openrust/framework/src/auth/README.md#sqlite-本地认证边界)。完整OIDC、队列等业务SQL仍须后续适配，不能仅凭安装成功切换这些生产能力。

`DB_DATABASE=:memory:` 创建池私有内存库，限制为一条连接，并禁止空闲和寿命回收，直至关闭池。`file:name?mode=memory&cache=shared` 允许同进程、同名 URI 的多个连接共享内存；最后连接关闭后数据消失，不跨进程共享。`file:` URI 由 SQLite 解析，可显式选择只读等模式；仅对可信部署配置开放。

每条物理连接依次执行 `pragmas`、`foreign_key_constraints`、显式 `busy_timeout`、`journal_mode` 和 `synchronous`。外键默认开启；`DB_FOREIGN_KEYS=false` 可关闭，`null`/`(null)` 跳过专用设置。该变量按 Laravel env 保留字及 PHP 布尔转换处理：`off`、`no` 等普通非空字符串仍为 true，请用 `false`/`(false)` 或 `0` 关闭。忙等待默认采用 PDO SQLite 的 60000 毫秒；`busy_timeout=None` 保留默认或前行 PRAGMA，显式值单位为毫秒。日志模式与同步级别默认不强制设置 WAL 或 NORMAL。PRAGMA 是可信配置中的原生 SQLite SQL token：名称可含 schema 和引号，字符串值须按所需 SQLite 表达式自行引用；不是用户输入绑定接口。与 PDO 一样只执行声明的首条语句，尾部 SQL 不会执行；配置错误不返回路径、URI 或驱动原文。

## 独立具名连接

数据库 Session 通过 `session.connection` 选择这里的同一具名连接，`session.table` 仍使用该连接的表名解析规则。默认 `database` 使用原始 ID、Base64 属性载荷和活动时间；要求绝对截止或持续撤销时显式选择 `database-guarded`，并登记同表增强迁移。具体配置、载荷切换与后端边界见 [Session 指南](../../../vendor/openrust/framework/src/session/README.md#原生-database-与显式增强)。Session 的后端支持不等同整个 Auth、队列和 Schema 已适配。

`DatabaseConfig.instances` 中每项是完整的 `DatabaseInstanceConfig`，不继承默认连接。应用通过 `Databases::get("reporting")` 获取句柄；具名连接不自动启用默认库的认证或队列。唯一综合模块提供 `/api/example/reporting-connection`：仅配置 reporting 时挂载，通过标准 Provider 检查该连接，响应仅含 backend 和 reachable。

## 隔离测试库

`TestDatabase::sqlite_memory::<M>()` 创建唯一命名内存库，guard 持有连接直到关闭；`url()` 可供同进程应用连接。`sqlite_file::<M>()` 创建自有临时目录和文件，`close().await` 关闭连接后清理自有文件。两者均通过标准 Provider 执行迁移 `M`；文件构造器不会接管调用方已有数据库。标准 FrameworkMigrator 与唯一示例已登记清单的 up/down/status 已验证；认证等其他迁移及完整 Laravel Migrator 命令族不在该结论内。

SQLite 文件名须满足当前 SQLx 驱动的 UTF8 要求，非 UTF8 路径明确拒绝。`foreign_key_constraints` 使用 `Option<bool>`：默认 `Some(true)`；`None` 或来源中的 `null` 跳过该专用设置，保留引擎默认或前序 PRAGMA。`busy_timeout`、`journal_mode`、`synchronous` 使用 `Option<serde_json::Value>`，可声明原生字符串或数值；`None`/`Some(Value::Null)` 均跳过。杂项 `pragmas` 中的 false/null 仍按上游插值进入原生错误边界，不等于跳过。PHP 浮点字符串精度等边界尚未完整对应。文件测试库须先关闭所有借用该 URL 的应用和 worker，再等待 `close()`，关闭期间停止新借用。显式关闭失败、取消或返回的 future 未被轮询时会保留目录，`Drop` 不会另外重试删除；普通 Drop 仅尽力清理，完整构造取消和平台故障矩阵仍待补齐。

## 当前边界与来源

nullable/原生 PRAGMA、安装器、文件跨进程互斥及既有迁移清单已完成限定验证。迁移取消会等待当前原生操作结束；SQLite 默认迁移不隐式包事务。旧版删列经 `dialect::drop_column` 在迁移所有者内重建 main 普通表，保留列、约束、索引/触发器与行标识；不支持 attached schema、虚表或完整 Schema API。完整 Laravel Migrator、其余框架迁移及认证、Session、队列和调度业务 SQL 仍未完成。文件库仅限单机，不承诺多机共享。迁移互斥不支持硬链接别名；同一文件的不同名字会让 SQLite 使用不同 journal/WAL，[上游明确不保证安全](https://www.sqlite.org/howtocorrupt.html#multiple_links_to_the_same_file)。不同 mount 别名、运行中替换文件及不确定 VFS 的迁移行为尚未闭合。

归还确认使用 SQLx 安全 API 注册每物理连接一个内部随机 collation 见证，不按空闲连接数判断，也不覆盖已有同名业务扩展。见证失效只表示该 handle 无法确认存活；故障与在途 acquire 之间的原子禁重连尚有缺口，内存连接关闭可能丢失数据。

最低版本路径不依赖 SQLite RETURNING；固定迁移依赖的局部 manifest 补丁和许可记录见仓库 `vendor/seaql/sea-orm-migration/SOURCES.md`。生成应用通过 OpenRust 的迁移 re-export 使用同一来源，不另加官方迁移包以免重新启用该 feature。

固定来源：[Laravel SQLiteConnector](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Database/Connectors/SQLiteConnector.php)、[Laravel 数据库文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md)、[PDO SQLite 默认超时](https://github.com/php/php-src/blob/php-8.4.0/ext/pdo_sqlite/sqlite_driver.c)。完整配置与事务 API 可用 `cargo doc --open` 查看 `openrust::database`。
