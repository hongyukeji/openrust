# 数据库注释翻译与 Laravel 迁移组织

2026-11-21，按用户授权完成本批实现、三库定向验收与开发库同步。本批不实现动态设置后台；其方案状态见[数据库动态设置](../plans/database-settings.md)。

## 来源与复用决定

- 固定 Laravel framework [`1449bf241017087fbd38c118ecaabef3f6797fdc`](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) 的 Translator、Schema Blueprint 与 PostgreSQL grammar；固定应用 [`06d016a364a37430eec9cbc52209adce3d7667ce`](https://github.com/laravel/laravel/tree/06d016a364a37430eec9cbc52209adce3d7667ce/database/migrations) 的 users、cache、jobs 三条初始迁移。以固定版本字段为准，包括 jobs.attempts 的 unsignedSmallInteger、队列时间的 unsignedBigInteger、cache.value 的 mediumText 与 expiration 的 bigInteger。
- 调研 Laravel 本身及 Spatie translation-loader／translatable、sql-commenter：分别属于翻译来源扩展、业务数据翻译和查询追踪，无须为数据库元数据增加这些依赖。复用现有 Translator／FileLoader／`__()`；未另设数据库注释翻译器、语言枚举、三语构造参数或连接级 comment_locale。
- 核对本地 openrust-v0.x 的 SeaORM schema 及注释用法，没有同契约实现可直接移植。Rust 语言资源沿用项目分组 JSON，等价承接 Laravel 分组语言资源职责，PHP 数组文件不直接作为 Rust 资源。

## 实现与边界

- 三个 `lang/<locale>/schema.json` 目录文件提供82个实际使用的表／字段翻译键；当前覆盖 en、zh_CN、zh_TW，新增语言仅增加资源文件。执行迁移时按 app.locale 和 fallback_locale 选择，缺词回退沿用现有翻译器，两者均缺失返回键。英文原文 flat JSON 方案已撤换，避免与分组回退规则不一致。
- 表与字段沿用 `.comment(string)`。注释使用“用户表”“用户名”“邮箱”等短文本；状态按真实存储值说明，时间说明 Unix 秒，JSON／哈希／加密语义按实际字段标注。SMS 状态保存字符串，未虚构成0／1。
- Schema `create`／`table` 接受 unit 或 DatabaseResult 回调，Rust 的 `?` 传播翻译错误，配置成功后才执行该 blueprint 的 DDL；whenTable* 保持原 unit 回调以兼容 panic-only 闭包。`getColumnsMut` 是对 Laravel 可变列对象的 Rust 借用适配，用于 morph 辅助方法生成列的注释。普通字符串注释仍可使用。
- PostgreSQL 新建表、新增列和表变更生成 COMMENT；已有 change 列注释逻辑保留。MySQL 使用既有 comment 语法；SQLite 不保存元数据注释，与 Laravel 的忽略行为一致。注释变更不自动随应用语言切换；已有安装通过新迁移更新。仅改 PostgreSQL 字段元数据可复用 MigrationContext::statement，不另造列注释 API。
- 根应用迁移六条合为四条：users、cache、jobs 三条初始迁移；jobs 同时创建／回滚 jobs、job_batches、failed_jobs；SMS 使用 `2026_11_21_000000_create_sms_deliveries_table`。Fortify、Passport、Passkeys 和 Sanctum 模板同样提供注释。迁移生成器保留 id／timestamps 骨架，未增加专用生成 DSL。
- 迁移 ledger 建表复用应用 translator；独立 DatabaseManager 使用同一 FileLoader 和配置。没有注释翻译资源的独立应用仍按标准缺词规则返回键，不能把非空字符串误认为翻译已齐全；本批测试额外拒绝内置 schema.* 裸键。

## 验收

隔离源码与日志保留在 `/private/tmp/openrust-schema-comments-20261121/`，专属 target；CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=3、dev/test debug=0，显式 Rust1.94.0。未在活跃工作树运行 Cargo 构建。所有31个任务文件最终与受验快照逐字节一致；三语82键静态检查全部存在且非空。

- PostgreSQL18.6：12迁移／17表的表和全部字段注释、en／zh-CN／zh-Hant／新增部分法语／缺目录日语及配置回退、字段新增／change、引号和反斜杠、注释更新保留数据／类型／默认／索引、注释删除、完整回滚、损坏德语资源在 DDL 前失败。
- MySQL8.4.13：真实连接执行同12迁移／17表，五种语言、表和全部字段注释、读写、完整回滚、损坏资源不创建表；另验 MySQL pretend 的表前缀和普通字符串注释。SQLite3.54.1：同12迁移／17表与五种语言、读写／回滚／翻译失败，验证目录不提供表和列注释。见 mysql-sqlite-final.log。
- PostgreSQL／MySQL／SQLite 的 Artisan ledger 三个函数实际执行通过，含迁移预演、状态、刷新、回滚与重跑；PG队列批次与独立迁移生成回归通过。见 regression-final3.log、mysql-ledger-final.log、sqlite-ledger-final.log。
- 多语言三个顶层函数在隔离PG下连续五轮全部通过，日志 schema-stability-final.log。此前出现临时资源读取不稳定，夹具已加入进程内原子计数、排他目录创建；另将 Artisan 夹具语言固定 en，避免中文环境与英文取消提示断言冲突。早期失败日志保留，不计作最终通过证据。
- workspace／all-targets／locked 严格 Clippy（`-D warnings`）及 workspace fmt 通过，另独立格式化迁移和 stub；见 clippy-final3.log、fmt-final.log。未声称全feature或完整发布矩阵通过。
- 三语内容检查与VitePress构建、根及 `/docs/` 两挂载三语桌面／移动12组合、迁移注释专项6页通过，见 docs-content-grouped.log、docs-build-grouped.log、docs-browser-grouped.log、docs-comments-browser.log。
- 本批未改变缓存驱动行为；Redis8.10.3独占实例用于已有队列回归。database／file缓存全行为、SQL Server、MariaDB及其他平台未计为本批验收。

## 开发库维护

当前本地 `.env` 指定的 PostgreSQL18开发库 locale 为 zh_CN。完整备份在 `/Users/Home/.local/state/openrust/backups/schema-comments-20261121-002542/development-postgres.dump`，目录0700、文件0600，不入Git；已在隔离 PostgreSQL18实际全量恢复，逐表行数／全部行哈希／ledger一致后才维护。

单事务写入10张表、55个字段的中文注释；删除两条独立 failed_jobs／job_batches 迁移记录，将SMS记录改为新文件名，保留批号，ledger由六条变四条。维护前后业务行哈希相同，排除注释的 schema-only 转储逐字节一致，字段类型、默认、索引及约束均保留。现有开发库保留历史字段定义，新安装按本批固定Laravel迁移定义创建。

最终只读 migrate:status 显示四条均Ran，`migrate:status --pending=17` 返回0并报告无待执行项；没有在开发库运行 fresh／rollback或再次执行维护脚本。维护日志及脱敏结果见 development/result.json、restore-verified.txt、status-list-final.log、status-final.log。其他已有安装须分别备份并校准自身ledger，不能仅替换文件名后直接迁移。

## 清理与交付

本批核范围本地提交，不推送；保留协作者的认证／缓存测试等改动。独占产物清理实测在下方补记，源码、原始日志、私密备份、开发数据库、全部数据卷及共享缓存保留。

已删除独占 target 3,645,356 KiB 和快照站点 dist 62,280 KiB；df 实测可用由45,938,956增至49,403,076 KiB，净增加3,464,120 KiB（共享机器并发写入会影响净值）。本批PG／MySQL／Redis临时容器确认全部回收，没有删除数据卷。见 cleanup-final.json。
