# 07-06 Search

依赖 05-03、06-04、07-02。对照固定 [Laravel Search](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/search.md)、[MySQL Grammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/MySqlGrammar.php) 和 [Scout DatabaseEngine](https://github.com/laravel/scout/blob/055441073f6c4791f4d2492f526b215d4a3afb24/src/Engines/DatabaseEngine.php)。数据库由既有编译型 Provider 装配，沿用固定 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go) 参考模式。旧版 `6d1a8192` 未找到全文谓词或 Scout 引擎实现，本批沿用当前 Blueprint 全文索引、语法包装、Eloquent 查询和分页。

本批选定数据库引擎。Query Builder、嵌套条件和 Eloquent 支持自然语言、布尔模式、查询扩展及 OR 分支。标识符经语法包装，搜索文本绑定为参数。原生全文索引随写事务更新和删除。SQLite 上的全文谓词返回明确错误。

Eloquent 使用 `Searchable` 声明列策略，Rust 枚举表达 Scout 的列属性。包含、前缀和联合全文条件分组，保留模型全局作用域、软删除和后续过滤。数字自增主键使用等值，空关键词保留模型的常规过滤。原生模型查询负责模型恢复及分页。无全文列时先用显式排序，再使用主键降序。

本批的 MySQL 全文和 SQLite LIKE 已验收。独立 Scout 包、外部索引引擎及队列同步尚未接入。MySQL 的写失败恢复使用既有数据库事务重试。PostgreSQL 全文、MariaDB 原生向量、pgvector、嵌入生成、AI 重排和混合搜索需要对应数据库与 AI 契约，保留在完整目标中。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区 520 项通过，4 个忽略项由集成测试作为子进程启动。格式和严格 Clippy 通过。
- 四项定向测试覆盖查询语法、绑定、方言拒绝、模型搜索、分页、HTTP 及实际 MySQL 8.4.13。真实行锁竞争触发一次事务重试，提交后全文索引反映更新。另验证全文记录删除、软删除作用域和模型数字主键。
- 三语六份代码片段在真实 MySQL 运行。VitePress 构建、12 项通用浏览器检查及两个搜索小节各 6 项导航通过。
- 临时 SQLite 迁移、Artisan inspire、首页和 /up 通过。

下一单元 07-07 Package Development，依赖 01-02、03-08、06-08。

## MySQL 核心主线复验

Rust 1.94.0 锁定离线 `search` 专项 4 项通过，连接独立 MySQL 8.4.13 验收库。实测索引建立、自然语言与布尔查询、分页、事务重试后的索引更新和删除。SQLite 覆盖已有的 LIKE 查询与全文方言拒绝。
