# Laravel 13.x 关系与集合验收对账

固定源码和用例基线为 `1449bf241017087fbd38c118ecaabef3f6797fdc`。以下按 05-02 的交付行为归并用例，记录实际对应入口和仍待核对的行为。单个 Rust 集成测试包含多个场景，测试文件之间的覆盖不能按数量一一折算。 [固定数据库用例](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database) · [固定集成用例](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database)

| 固定用例组 | 实际对应证据 | 尚待核对 |
| --- | --- | --- |
| HasOne、HasMany、BelongsTo、HasOneOfMany | `verify`、`verify_singular_identity`、`verify_belongs_to_cache` 验证关联外键、身份、软删除、并列决胜和缓存回填。 | 对照直接关系与反向关系的变体用例逐项核对。 |
| HasManyThrough、HasOneThrough、MorphTo、MorphToMany | `verify_through_and_morph`、`verify_one_through`、`verify_morph_soft_deletes` 验证穿透、类型隔离和软删除。 | PHP 的隐式类及别名发现需按 Rust 显式注册的职责核对。 |
| EagerLoadingLimit、LazyEagerLoading、ModelRelationAutoload、MorphEagerLoading | `verify_eager_columns`、`verify_named_paths`、`verify_morph_paths`、`verify_relation_autoload` 验证带列和路径的预加载、查询计数及自动加载。 | Rust 异步回填及共享关系快照已专项核对，见 [折叠与共享快照](README.md#折叠模型集合与关系快照)。 |
| BelongsToMany、Pivot、PivotEvents、CustomPivotCast | `verify_custom_pivot`、`verify_pivot_constraints`、`verify_pivot_or_fail`、关系 upsert 验证事件、转换、同步及回滚。 | 枚举 Pivot 键、返回类型与仅创建时间已通过 [MySQL 专项](README.md#pivot-枚举键返回类型与仅创建时间)。剩余的动态对象共享及关联用例继续对账。 |
| EloquentCollection、CollectionFresh、CollectionLoadMissing、CollectionQueueable | `verify_collection_keys`、`verify_collection_reload`、`verify_collection_factory` 以及既有真实 Worker 恢复记录。 | 分层 `collapse` 与 [`multiply`](README.md#模型集合重复) 已复验。自定义集合沿用 `Model::newCollection` 和扩展 trait。[集合延后计数与嵌套选列](README.md#嵌套关系选列与集合批量计数)已实测。[嵌套谓词、同名递归和多态混合目标](README.md#缺失关系的嵌套谓词)已实测。Laravel 在单次调用中组合多条带约束路径的契约继续核对。 |
| BelongsToManyChunkById、EachById、LazyById 及 Builder 查询转发 | `pagination_on_mysql_and_http` 实测各关系类型的分批、异步流、短路、缺键与 Pivot。 | 关系分页剩余项见 [05-06 测试矩阵](../05-06/test-matrix.md)。 |

当前复验仍按表中边界记录。05-02 的核心账本保持进行中。
