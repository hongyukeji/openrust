# Scout

## 调研与设计（2026-11-21）

联网读取GitHub官方仓库元数据与固定Laravel文档，采用MIT项目[laravel/scout](https://github.com/laravel/scout/tree/6f6560dbcda3ec99d9ba90bd6eebed5e0f16e363)的Searchable、ModelObserver、EngineManager、Builder、CollectionEngine及队列Job职责。固定文档为[Laravel文档快照](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scout.md)。源代码、元数据与文档存于项目tmp/openrust-ecosystem-20261121。web搜索未返回正文，实际使用GitHub官方API／raw源。v0.x提交6d1a81924451e15f595f68c8dfb520ad6d6c44cf未发现Scout／Searchable独立实现可提取。

首个引擎按固定上游默认选择collection，直接查询原生Eloquent再按toSearchableArray进行内存包含匹配；不引入独立倒排索引或假装实现第三方服务。保留Engine契约与应用级EngineManager扩展入口、null引擎。数据库全文、Algolia／Meilisearch／Typesense／Turbopuffer均不冒充已实现；配置未知驱动明确失败，用户可实现Engine接入。共享SQL仍须三库实际验证。

目录定为vendor/openrust/scout，公开Searchable／ModelObserver／EngineManager／Builder、engines及jobs，配置scout归包，由ScoutServiceProvider发现、发布。根应用只增加可选feature／依赖。静态泛型及显式注册模型代替PHP反射／动态trait启动，应用Provider登记模型；不向默认骨架添加示例模型。Searchable需Clone以复用原生提交后观察器，队列更新重新读取模型，删除携带主键墓碑；保留事务提交后执行与回滚丢弃。复用原生队列、模型事件、分页及流，不创建并行基础设施。

## 验收状态

O06 约定的首个引擎范围已实现并通过单包验收；最终组合结果见下节与统一扩展账本。没有把所有上游引擎或完整 PHP PHPUnit 套件计作交付。


## 实现与单包验收（2026-11-21）

- Searchable、ModelObserver、EngineManager、Builder、SearchableCollection、MakeSearchable／RemoveFromSearch 使用原生 Eloquent、提交后观察器、队列 Worker、Artisan、分页及 SQL 游标。默认 collection，另有 null 和泛型 Engine 扩展入口。模型须在应用 Provider 显式 register；同一类型与别名重复登记均拒绝，任务和开关按应用隔离。
- 查询覆盖大小写、空字符串／空格／0、scalar 与嵌套 JSON、where／in／not-in、排序、limit、raw 回调、模型映射回调、分页、简单分页和游标。映射尊重 getScoutModelsByIds 自定义入口，按引擎顺序返回；整数主键复用现有 PHP cast／loose comparison 帮助函数。SQL 游标复用核心已有 inOrderOf／reorder 实现的 Eloquent 转发，不采用逐 ID 查询或新增 CASE 拼装器；简单分页复用原生 Paginator::hasMorePagesWhen。
- 正常、嵌套提交／回滚、立即观察器、软删除／恢复／强删、shouldBeSearchable、索引更新条件、嵌套与取消后的 withoutSyncingToSearch 恢复均已验证。嵌套事务先提交的内层 callback 顺序 [4,2] 与固定 Laravel DatabaseTransactionsManager::stageTransactions 一致，未为测试修改核心事务顺序。
- 真实 database Worker 更新时重新读当前模型，删除使用 forceFill 主键墓碑，受保护主键与已删业务行不阻断清索引。引擎首次失败后任务保留，按原生 tries=2／backoff=0 重试读取最新字段；异常不吞掉。import 支持固定上游 --fresh／--chunk／-c，实际 2/2/1 分块；flush 只清索引，保留业务行。未知模型、驱动、chunk=0、未支持语义查询明确失败。
- 对照固定 CollectionEngine::searchModels 后修复 null 必须先 json_encode 再匹配；复用核心 php_float_string／php_numeric_json_string，保留 JSON 非 ASCII、斜线和浮点格式。tests/reference.php 在固定 PHP8.5.12 镜像实际生成 12 个值、16 个查询的结果，tests/fixtures/collection-php.json 通过公开引擎对照；这是具体语义参考，不等同完整上游 PHPUnit。

证据统一位于项目 tmp/openrust-scout-20261121，源码快照 tmp/openrust-pennant-20261121/source，独占 target tmp/openrust-core-ecosystem-20261121/target，TMPDIR 为项目内绝对路径。Rust1.94.0、原生并发及增量，未使用额度停线。

| 验证 | 实际结果／日志 |
|---|---|
| 包行为与实际 HTTP | tests-final.log：6项通过；最终六扩展组合再次覆盖同一6项 |
| PostgreSQL18.7／MySQL8.4.13 | postgres-final.log／mysql-final.log：各6项，无提前返回或 ignored；专用包装器创建隔离数据库，单线程避免同名夹具表互相覆盖 |
| 根全部 feature 发现／骨架／发布／缓存 | configuration-final-combined.log：配置1＋骨架2通过；configuration-pgsql.log／configuration-mysql.log 各配置1通过 |
| 关闭 feature | configuration-off.log：Scout命令不可用1＋骨架2通过 |
| 严格检查 | clippy-workspace-final.log：全 workspace／all-features／all-targets -D warnings 通过；fmt-final.log 通过。第三方依赖既有warning不冒充清零 |
| 三语文档 | docs-build-final.log 构建通过；docs-scout-browser-final.log 专项12组、通用12组和导航6组通过；最终文档及 API 组合按统一账本记录 |

## 明确支持边界

静态 trait 与显式泛型注册替代 PHP 动态 trait boot／反射，公开方法沿用上游拼写；异步 I/O 返回 Future／Result。Builder 分页显式传入页大小和页码，缺失／非正尺寸拒绝；不伪造当前 HTTP 页码解析。未提供 PHP 动态宏或任意对象反射。

本阶段为路线中明确的“引擎契约与首个引擎”：不内置 Scout database／Algolia／Meilisearch／Typesense／Turbopuffer、外部索引设置同步、导入队列编排及语义服务。未知驱动及未支持语义／混合搜索明确报错，不能称为全部 Scout 驱动已移植。核心原有 database::eloquent::Searchable 数据库全文能力是独立入口，三语 search 指南已与 Scout 区分。默认应用未添加演示模型／Provider／配置，许可证随包保留。
