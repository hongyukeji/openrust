# Laravel 13.x 关系与集合验收对账

固定源码和用例基线为 `1449bf241017087fbd38c118ecaabef3f6797fdc`。以下按 05-02 的交付行为归并用例，记录实际对应入口和仍待核对的行为。单个 Rust 集成测试包含多个场景，测试文件之间的覆盖不能按数量一一折算。 [固定数据库用例](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database) · [固定集成用例](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database)

| 固定用例组 | 实际对应证据 | 尚待核对 |
| --- | --- | --- |
| HasOne、HasMany、BelongsTo、HasOneOfMany | `verify`、`verify_singular_identity`、`verify_belongs_to_cache` 验证关联外键、身份、软删除、并列决胜和缓存回填。 | [单条关系多列快捷入口](README.md#单条关系的多列选择)已双库实测。[反向关系零值、枚举外键、非主键所有者键及首行／末行语义](README.md#belongsto-所有者键与单条加载)已双库实测。 |
| HasManyThrough、HasOneThrough、MorphTo、MorphToMany | `verify_through_and_morph`、`verify_one_through`、`verify_morph_soft_deletes` 验证穿透、类型隔离和软删除。 | [反向多态所有者键、别名与穿透四键](README.md#反向多态所有者键和穿透四键)已双库实测。[多态赋值零值键、所有者列投影与错误原子性](README.md#多态关联赋值与所有者列投影)已双库实测。 |
| EagerLoadingLimit、LazyEagerLoading、ModelRelationAutoload、MorphEagerLoading | `verify_eager_columns`、`verify_named_paths`、`verify_morph_paths`、`verify_relation_autoload` 验证带列和路径的预加载、查询计数及自动加载。 | Rust 异步回填及共享关系快照已专项核对，见 [折叠与共享快照](README.md#折叠模型集合与关系快照)。 |
| BelongsToMany、Pivot、PivotEvents、CustomPivotCast | `verify_custom_pivot`、`verify_pivot_constraints`、`verify_pivot_or_fail`、关系 upsert 验证事件、转换、同步及回滚。 | 枚举 Pivot 键、返回类型与仅创建时间已通过 [MySQL 专项](README.md#pivot-枚举键返回类型与仅创建时间)。[Pivot 闭包筛选读取与写入](README.md#pivot-闭包条件与筛选写入)已双库实测。继续核对限定条件下自定义 Pivot 的事件选择。 |
| EloquentCollection、CollectionFresh、CollectionLoadMissing、CollectionQueueable | `verify_collection_keys`、`verify_collection_reload`、`verify_collection_factory` 以及既有真实 Worker 恢复记录。 | 分层 `collapse` 与 [`multiply`](README.md#模型集合重复) 已复验。自定义集合沿用 `Model::newCollection` 和扩展 trait。[集合延后计数与嵌套选列](README.md#嵌套关系选列与集合批量计数)已实测。[嵌套谓词、同名递归和多态混合目标](README.md#缺失关系的嵌套谓词)已实测。[批量缺失路径的独立约束、重复闭包顺序与共享父关系](README.md#批量缺失关系谓词)已实测。 |
| BelongsToManyChunkById、EachById、LazyById 及 Builder 查询转发 | `pagination_on_mysql_and_http` 实测各关系类型的分批、异步流、短路、缺键与 Pivot。 | 关系分页剩余项见 [05-06 测试矩阵](../05-06/test-matrix.md)。 |

当前复验仍按表中边界记录。05-02 的核心账本保持进行中。
