# 数据库连接

应用的可编辑默认值位于 `config/database.rs`，部署值位于 `.env`。标准 Provider 在同一 Container 中提供 `Db` 和 `Databases`；当前在启动时建立已配置连接，完整 Laravel 懒连接仍待实现。

## SQLite 连接

启用 `db-sqlite` 后可使用标准 Provider，无需自定义连接器。连接器按固定官方最低 SQLite 3.26.0 校验；当前实际验证引擎为 3.51.3，最低版本完整运行矩阵仍未完成。默认 MySQL 与 Redis 不变。

```toml
openrust = { version = "0.0.0", features = ["db-sqlite"] }
```

```dotenv
DB_CONNECTION=sqlite
DB_DATABASE=/absolute/path/database.sqlite
DB_FOREIGN_KEYS=true
```

普通数据库文件须先存在；未指定 `DB_DATABASE` 时使用应用根下的 `database/database.sqlite`。相对文件先按当前工作目录解析，失败后按应用根解析。连接器不会为普通缺失路径创建文件。安装器的 SQLite 选择、默认迁移及完整业务支持仍待后续交付，当前不要把默认应用的认证、队列等切换为 SQLite。

`DB_DATABASE=:memory:` 创建池私有内存库，限制为一条连接，并禁止空闲和寿命回收，直至关闭池。`file:name?mode=memory&cache=shared` 允许同进程、同名 URI 的多个连接共享内存；最后连接关闭后数据消失，不跨进程共享。`file:` URI 由 SQLite 解析，可显式选择只读等模式；仅对可信部署配置开放。

每条物理连接依次执行 `pragmas`、`foreign_key_constraints`、显式 `busy_timeout`、`journal_mode` 和 `synchronous`。外键默认开启；`DB_FOREIGN_KEYS=false` 可关闭。忙等待默认采用 PDO SQLite 的 60000 毫秒；`busy_timeout=None` 保留默认或前行 PRAGMA，显式值单位为毫秒。日志模式与同步级别默认不强制设置 WAL 或 NORMAL。配置名称和值须是框架支持的安全标量，错误不返回路径、URI 或驱动原文。

## 独立具名连接

`DatabaseConfig.instances` 中每项是完整的 `DatabaseInstanceConfig`，不继承默认连接。应用通过 `Databases::get("reporting")` 获取句柄；具名连接不自动启用默认库的认证或队列。唯一综合模块提供 `/api/example/reporting-connection`：仅配置 reporting 时挂载，通过标准 Provider 检查该连接，响应仅含 backend 和 reachable。

## 隔离测试库

`TestDatabase::sqlite_memory::<M>()` 创建唯一命名内存库，guard 持有连接直到关闭；`url()` 可供同进程应用连接。`sqlite_file::<M>()` 创建自有临时目录和文件，`close().await` 关闭连接后清理自有文件。两者均通过标准 Provider 执行迁移 `M`；文件构造器不会接管调用方已有数据库。框架默认迁移的 SQLite 方言与互斥仍待后续验证，当前使用已兼容的应用迁移或空迁移器。

SQLite 文件名须满足当前 SQLx 驱动的 UTF8 要求，非 UTF8 路径明确拒绝。复杂或带 schema 的 PRAGMA 名称、同步级别数值写法和 nullable 外键配置尚未完整对应；当前字段不能代表全部 Laravel 配置表达。测试库应显式等待 `close()`，Drop 仅尽力清理，完整取消和故障回收矩阵仍待补齐。

## 当前边界与来源

当前交付目标仅为连接、配置和自有测试库；迁移互斥、全部框架迁移，以及现有认证、Session、队列和调度 SQL 的 SQLite 适配尚未完成。文件库仅限单机，不承诺多机共享。

固定来源：[Laravel SQLiteConnector](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Database/Connectors/SQLiteConnector.php)、[Laravel 数据库文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md)、[PDO SQLite 默认超时](https://github.com/php/php-src/blob/php-8.4.0/ext/pdo_sqlite/sqlite_driver.c)。完整配置与事务 API 可用 `cargo doc --open` 查看 `openrust::database`。
