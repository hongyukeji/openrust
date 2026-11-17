# 数据库方言对账

按 [Laravel v13.40.0](../plans/laravel-rebuild-baseline.md) 核对 02-04 查询方言、02-05 Schema 与 02-06 迁移。表内实测范围以各单元验收记录和本地提交为准，完整数据库版本与跨平台矩阵在发布验收时复跑。

| 驱动 | 已有真实验收 | 下一项差异或验收条件 |
| --- | --- | --- |
| MySQL | 8.4.13 已验证 JSON 重叠、跨库子查询、分组限额、Schema 目录与外键、迁移建库、快照导出恢复和限定清库。 [02-04](02-04/README.md) · [02-05](02-05/README.md) · [02-06](02-06/README.md) | 日常继续用 8.4。其余支持版本复验 JSON 函数、窗口函数和 DDL 修饰符可用性。 |
| MariaDB | 11.8.9 已验证向量列、索引、自定义距离选项、查询与 Eloquent 作用域，以及默认 TLS 和配置 CA 的原生快照。 [向量查询](02-04/README.md#向量相似度查询) · [向量索引](02-05/README.md#向量扩展列与索引) · [原生快照](02-06/README.md#mariadb-原生-schema-快照) | 在兼容矩阵复验较早版本的原生改列、目录和常规查询。向量能力按支持该类型的服务端版本验收，快照客户端依现有 TLS 要求验证。 |
| PostgreSQL | 18.4／18.6 已验证向量查询与列、在线索引、类型目录、外键、迁移事务、缺库创建和快照。事务池的模拟预备及直连分离已实测。 [向量查询](02-04/README.md#向量相似度查询) · [外键](02-05/README.md#postgresql-外键变更与验证时机) · [唯一约束](02-05/README.md#postgresql-唯一约束与在线附加) · [迁移事务](02-06/README.md#整条迁移的事务与显式退出) · [事务池](02-02/README.md#pgbouncer-关闭预备语句跟踪) · [PostgreSQL 普通索引方法与重命名](02-05/README.md#postgresql-普通索引方法与重命名) · [PostgreSQL 列类型转换与修饰符](02-05/README.md#postgresql-列类型转换与修饰符) · [PostgreSQL 生成列与显式 Identity](02-05/README.md#postgresql-生成列与显式-identity) | JSON 选择器形式的生成列待对齐。发布矩阵复验生成列、访问方法与在线 DDL。 |
| SQLite | 已验证原生目录、内联约束和多类表重建，以及快照、内存库恢复和缺库创建确认。 [内联约束](02-05/README.md#sqlite-内联约束) · [SQLite 快照](02-06/README.md#sqlite-schema-快照) · [缺库创建](02-06/README.md#sqlite-缺库创建确认) | 发布前复跑附加库、外键检查会话、重建后的索引和数据保留，以及最低支持版本。 |
| SQL Server | 2022 Developer 16.0.4325.4 已在 Apple Silicon 的 Linux amd64 兼容容器中通过在线普通和唯一索引、按名称删除外键、索引和外键目录、真实 Artisan 迁移失败回滚及账本验收。 [SQL Server 在线索引与具名外键](02-05/README.md#sql-server-在线索引与具名外键) · [失败回滚](02-06/README.md#真实-sql-server-迁移失败回滚) | SQL Server 的原生改列及默认约束处理待对齐。在原生 Linux x86_64 和发布版本矩阵复验。在线索引按产品版本和版本级别的支持范围验收。固定 Laravel 快照范围包含 MySQL、MariaDB、PostgreSQL、SQLite。 |

[固定 Laravel PostgreSQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/PostgresGrammar.php) · [固定 Laravel SQL Server Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/SqlServerGrammar.php) · [固定 Laravel 快照范围](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#squashing-migrations)
