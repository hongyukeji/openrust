# 05-02 Eloquent 关系、加载与集合（进行中）

## 已实现和验证

- 对照固定 Laravel 的 `HasOneOrMany::matchOneOrMany`、`BelongsTo::addEagerConstraints`、`Builder::eagerLoadRelations` 及 `InteractsWithPivotTable`。参考 Goravel 的应用级 ORM Provider。核对旧版 `6d1a8192` 的 `eloquent/{has_many,belongs_to,belongs_to_many,relations}.rs`，提取显式键、分批读取和按外键匹配的职责。 [Laravel 关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md) · [Laravel Eloquent 集合](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 接通 `HasOne`、`HasMany`、`BelongsTo`、`BelongsToMany`、`with`、`load`、`loadMissing`、`modelKeys`、`find`、`contains`、`only` 和 `except`。多对多通过 JOIN 一批加载。直接关系和中间表均按最多 500 个键分批。父模型顺序及空结果保留，相关模型继续应用全局作用域与软删除。Rust 使用类型化描述符、`Arc` 及显式 `getRelation`，用新的父模型关联类型及批量契约替换先前的 `Relation` 骨架签名。
- 接通关系创建、外键更新、`associate` 与 `dissociate`，以及中间表的 `attach`、`detach`、`sync`、`syncWithoutDetaching`。同步在事务内增删链接，复用连接层并发重试。中间表字段属性更新尚未接入。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 436 项测试。`./artisan inspire` 正常退出。
- SQLite 与真实 MySQL 8.4.13 验证外键、软删除过滤、空关系、集合主键、关联创建和修改、失败回滚及重试后的同步。MySQL 专项使用 `OPENRUST_REQUIRE_RELATION_MYSQL=1`。实测三个父模型逐条读取为 4 次 SELECT，直接和中间表预加载均为 2 次。对已加载关系调用 `loadMissing` 不增加查询。
- 英文、简体中文、繁体中文的关系和集合示例共六份，编译并分别在 SQLite 和真实 MySQL 运行。VitePress 构建、12 项通用浏览器组合及 12 项关系和集合正文、导航与交叉链接检查通过。

下一步继续 05-02 的嵌套及约束预加载、穿透和多态关系、关系存在查询、中间表附加字段和其他集合方法。三语页面已闭合上述通过验收的小节，仍随同一单元续补。

## 嵌套预加载、关系存在查询与中间表字段

- 对照固定 Laravel 的 `Builder::getRelation`、`eagerLoadRelation`、`QueriesRelationships` 以及 `InteractsWithPivotTable`。提取旧版 `has_many.rs` 的约束和分批加载顺序。Rust 的 `constrain` 闭包显式配置关联查询，闭包内部的 `with` 接入嵌套加载。外键条件独立于应用查询条件，应用的 OR 无法改变父键约束。单父模型支持行数限制，批量逐父限额仍待实现。
- 接通 `has`、`whereHas`、`whereDoesntHave` 和 `withWhereHas`。用同一连接上的相关子查询执行过滤，包含直接关系、自关联和多对多。自关联使用内层表别名。跨连接存在查询返回查询错误。
- `withPivot` 选择中间表附加字段，`r#as` 配置访问名称。中间表字段以独立的 `Pivot` 值加载，保留模型同名属性。接通 `attachWith` 和 `updateExistingPivot`，拒绝写入覆盖关联键。同步时逐键写入附加字段仍待接通。
- 隔离副本重跑 `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings`，436 项通过。真实 MySQL 8.4.13 验证筛选并嵌套加载三层关系恰好执行 3 次 SELECT。SQLite 与 MySQL 均验证自关联别名、关系不存在、带 OR 的关联约束、中间表附加字段的读取和更新以及关联键保护。Artisan 正常运行。
- 三语新增示例各在 SQLite 和 MySQL 执行一次。VitePress 构建、12 项通用浏览器组合与 6 项新章节、锚点和侧栏检查通过。穿透、多态关系、关系聚合及其余集合能力待续。

## 穿透与多态关系

- 对照固定 Laravel 的 `HasOneOrManyThrough::performJoin`、`MorphTo::buildDictionary` 和 `MorphToMany`。参考固定 Goravel 的连接级 ORM Query 解析，沿用旧版 `eloquent/relations.rs` 的关系加载职责。 [Laravel 穿透源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php) · [Laravel 多态源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphToMany.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `HasOneThrough` 和 `HasManyThrough` 通过中间表 JOIN 查询，按父键分组。默认过滤已软删除的中间行，`withTrashedParents` 可放开中间行过滤。相关模型的软删除列使用限定名，与中间模型的同名列独立。支持嵌套加载和关系存在子查询。
- `MorphOne` 和 `MorphMany` 强制关联 ID 与类型别名同时匹配。反向 `MorphTo` 通过描述符显式登记 Rust 类型和别名，按别名批量读取，返回可按类型读取的 `MorphTarget`。未知非空别名返回查询错误。`MorphToMany` 在中间表的读取、存在查询、附加、解绑及同步中限制类型列。反向定义显式交换中间表键，`MorphPivot` 复用中间表属性类型。
- Rust 1.94.0 隔离副本的 `cargo fmt --all --check`、`cargo test --workspace --locked --offline`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，436 项测试。`./artisan inspire` 正常退出。SQLite 与真实 MySQL 8.4.13 验证中间与最终模型同时有软删除列、类型与 ID 重叠时的读取及更新隔离、反向关联、未知类型失败、关联与解除关联。
- MySQL 实测穿透预加载为 2 次 SELECT，反向多态加载两个目标类型为 3 次 SELECT。测试将建表和删表移出计数期间，避免 MySQL 的自动重新预备语句影响会话计数。
- 三语新增的穿透和多态示例共六份，在 SQLite 和 MySQL 分别执行。VitePress 构建、12 项通用浏览器组合和 6 项新章节、侧栏及锚点检查通过。

下一步继续 05-02 的关系聚合、逐父行数限制、中间表逐键属性同步及其余集合能力。多态存在过滤的类型选择接口和跨连接加载的边界仍待核对。

## 关系聚合、逐父限额与中间表属性同步

- 对照固定 Laravel 的 `QueriesRelationships::withAggregate`、`Grammar::compileGroupLimit`、`InteractsWithPivotTable::sync` 和 `syncWithPivotValues`，复核旧版的关系聚合声明与加载方式。继续沿用固定 Goravel 的连接级查询入口。 [Laravel 关系聚合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 窗口查询源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Grammars/Grammar.php) · [Laravel 中间表同步源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/InteractsWithPivotTable.php)
- 接通 `withCount`、`withExists`、`withSum`、`withAvg`、`withMin`、`withMax` 以及类型化的 `withAggregate`。通过同连接相关子查询生成属性，保留现有投影，支持默认名和显式别名。存在属性转为布尔值，其余聚合保留数据库值表示。
- 批量加载对 `limit` 和 `offset` 使用按关联键分区的 `row_number()`，保留关联查询排序并移除辅助列。单父模型沿用普通查询分页。中间表及穿透辅助投影采用独立别名。
- 接通 `syncWithPivotValues` 统一属性和 `syncWithPivotAttributes` 逐键属性。返回新增、解绑和实际更新的键。通过数据库的空值安全比较跳过相同值，插入、更新与删除在同一事务执行，关联键与多态类型字段受保护。
- 隔离 Rust 1.94.0 执行格式检查、锁定离线工作区测试及严格 Clippy，436 项测试通过。`./artisan inspire` 正常退出。SQLite 与真实 MySQL 8.4.13 验证聚合别名、空值、自关联、软删除过滤、逐父偏移、多对多逐父限额及其关联属性保留。MySQL 实测父查询与六类聚合共 1 次 SELECT，逐父限额预加载为 2 次。两种数据库均验证逐键同步的相同值跳过、空值更新及失败回滚。
- 三语新增聚合示例均在 SQLite 与 MySQL 执行。VitePress 构建、12 项通用浏览器组合和 6 项章节导航及锚点检查通过。

05-02 仍需继续一对多择一、反向多态过滤和聚合、关系计数比较及剩余集合方法。

## 一对多择一、反向多态过滤与集合键

- 接通 `HasMany::one`、`MorphMany::one`、`latestOfMany`、`oldestOfMany`、`ofMany` 和候选条件。复用旧版 `one_of_many.rs` 的多列择一规则及测试中的主键决胜用例。Rust 用逐级关联的分组聚合子查询选择同一条记录，固定 Laravel 用分组子查询 JOIN。两端均按关联键分组，再按顺序应用聚合列和主键决胜规则。 [Laravel 择一关系源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/CanBeOneOfMany.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 接通 `hasCount`、`whereHasMorph`、`whereDoesntHaveMorph`、`hasMorphCount` 和 `morphWithCount`。反向多态以显式注册的类型和别名构造同连接查询，计数比较按条件采用 EXISTS 或标量计数子查询。 [Laravel 关系查询源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php)
- 接通集合 `merge`、`unique`、`diff`、`intersect` 和 `toQuery`，统一 `only` 和 `except` 的主键查找行为。`merge` 与 `unique` 以最后出现的模型替换相同主键，保留首次出现的位置。`toQuery` 按同一来源的模型主键重查，空集合、缺失主键和混合来源返回错误。 [Laravel Eloquent Collection 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php)
- SQLite 与真实 MySQL 8.4.13 验证择一并列、全空候选、候选 OR 条件、软删除、类型隔离、反向多态计数和集合重复键行为。MySQL 批量择一为 2 次 SELECT。Rust 1.94.0 格式、436 项锁定离线工作区测试、严格 Clippy 和 `./artisan inspire` 通过。工作区的统一 JSON 响应回归也通过，使用既有的旧版 `ApiResponse` 和 `PageMeta` 迁移实现。
- 三语关系及集合示例各在 SQLite 和 MySQL 执行。VitePress 构建、12 项通用浏览器组合及 12 项关系和集合页面示例、标题和目录锚点检查通过。

05-02 仍需核对穿透关系择一、反向多态通配类型及默认预加载、集合 `fresh` 和加载后聚合。

## 集合刷新与加载后聚合

- 对照固定 Laravel `Collection::fresh`、`loadAggregate`、六个聚合便捷方法及空集合刷新测试。参考固定 Goravel 的连接级 ORM Query 入口。复查 `openrust-v0.x` 的 `6d1a8192:vendor/openrust/framework/src/database/eloquent/refresh.rs` 及 `tools/cli/templates/tests/query_insert_ids.rs`，沿用从数据库水合独立实例后再加载关系的顺序。旧版模型级刷新显式走写连接，集合按固定 Laravel 的普通读取路由执行。 [Laravel 集合文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md) · [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `fresh` 跳过父模型作用域，保留原顺序和重复键，略过未保存模型及已移除的行。重复键水合为独立 Rust 模型。可用 `freshWith` 指定一个类型化关系，关系描述符支持嵌套预加载。
- `loadAggregate` 复用构造查询时的聚合投影和存在值转换，每批最多 500 个不同主键。更新目标属性及其原始快照，保留其余待保存属性和已加载关系。查询结果缺失时先返回错误，避免部分修改。当前调用一次传入一个关系和聚合类型。
- SQLite 与真实 MySQL 8.4.13 验证空集合、未保存模型、软删除作用域、重复键、已删除行导致的原子失败和刷新后重试、聚合空值和待保存属性。MySQL 测得集合刷新加预加载为 2 次 SELECT，随后加载计数为 1 次 SELECT。
- Rust 1.94.0 隔离副本的格式检查、锁定离线工作区测试（436 项）、全目标严格 Clippy 和 `./artisan inspire` 通过。三语刷新和聚合示例在两种数据库执行。VitePress 构建、12 项通用浏览器组合及 6 项集合页面标题、示例与目录锚点通过。

下一批仍属 05-02，继续穿透关系择一、反向多态通配和默认预加载，再核对集合批量聚合接口。

## 穿透关系择一

- 对照固定 Laravel `HasManyThrough::one`、`HasOneThrough` 和 `DatabaseEloquentHasOneThroughOfManyTest` 的单条读取、预加载、存在查询、计数和多列择一。参考固定 Goravel 的连接级 ORM Query。旧版 `6d1a8192:vendor/openrust/framework/src/database/eloquent/one_of_many.rs` 提供直接关系的相关聚合机制，未找到穿透择一实现。复用新版直接关系的聚合列链和候选过滤，将分组相关键接到穿透父键。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneThrough.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentHasOneThroughOfManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `HasManyThrough::one` 接通 `latestOfMany`、`oldestOfMany`、`ofMany`、`ofManyBy` 和 `whereCandidates`。Laravel 以分组子查询 JOIN 匹配聚合，Rust 以逐列相关聚合子查询匹配，并在候选查询内以 EXISTS 检查中间记录是否属于当前父模型。候选条件只作用于最终模型，中间和最终模型的软删除过滤分别处理。快捷方法继续使用可选单列，多个聚合列显式交给 `ofMany`。
- SQLite 与真实 MySQL 8.4.13 验证跨中间行并列和主键决胜、不同父键与空关系、筛选候选、软删除与分别放开两层软删除、存在和计数、多列聚合以及中间表与父表同名时的别名。MySQL 批量预加载为 2 次 SELECT。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建、12 项通用浏览器组合及 6 项新增章节锚点检查通过。

05-02 下一批继续反向多态通配、默认预加载和集合批量聚合接口。

## 反向多态通配查询

- 对照固定 Laravel `QueriesRelationships::hasMorph` 的通配分支及 `EloquentWhereHasMorphTest` 的类型映射、仅空类型、缺失关联和外层条件分组用例。参考固定 Goravel 的连接级查询入口。检查 `openrust-v0.x` 的 `6d1a8192` Eloquent 源码和模板数据库用例，未发现反向多态通配实现。继续复用新版已验收的类型注册、存在查询及计数构造。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#querying-all-morph-to-related-models) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentWhereHasMorphTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `whereHasMorphAll`、`whereDoesntHaveMorphAll` 和 `hasMorphCountAll` 显式等待发现查询，保留既有显式别名方法的同步构建。发现查询在当前连接上读取模型表的不同类型，跳过模型全局作用域、软删除及外层过滤。使用登记的 Rust 类型构建 SQL，数据库含未登记的有效别名时返回查询错误。
- 缺失目标使用分组否定存在谓词。按固定 Laravel 的比较条件将空类型加入分组，只有空类型时按零个关联处理。实际 SQLite 与 MySQL 8.4.13 验证可用和缺失目标、仅空类型、额外外层筛选、软删除行中的未知类型和未使用的已注册别名。MySQL 实测 2 次 SELECT。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建、12 项通用浏览器组合和 6 项新章节锚点检查通过。

05-02 下一批继续模型默认预加载和集合批量聚合接口。

## 模型默认预加载

- 对照固定 Laravel 模型 `$with`、`newModelQuery`、`newQueryWithoutScopes` 及 Builder 的 `without`、`withOnly`，参考空结果跳过预加载的上游测试和 Goravel 连接级查询入口。旧版 `6d1a8192` 的 Eloquent 关系加载以显式 `RelationLoad` 描述符批处理嵌套关系，未找到模型级默认清单。新版沿用已接通的批量加载和类型化描述符。 [Laravel 默认预加载文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#eager-loading-by-default) · [Laravel 模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel Builder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `Model::withDefaultRelations` 接收类型化 `EagerLoads`。按查询用 `without` 移除命名关系，用 `withOnly` 替换整组关系，用 `with` 覆盖同名描述符。集合 `fresh` 在恢复顺序和重复项后统一加载默认关系。`freshWith` 覆盖指定关系并保留其他默认项。集合 `toQuery` 和加载聚合从基础模型查询构建。
- SQLite 与真实 MySQL 8.4.13 验证查询级隔离、同名约束覆盖、默认关系空结果、重复主键刷新及内部聚合隔离。MySQL 实测两个默认关系共 3 次 SELECT，移除其中一个后为 2 次。刷新并覆盖一个默认关系共 3 次 SELECT。集合按键重查和随后加载计数各 1 次 SELECT。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建、12 项通用浏览器组合及 6 项新章节锚点检查通过。

05-02 下一批继续集合批量聚合和模型默认 `$withCount` 的对应入口。自动关系加载属于单独行为，当前尚未接通。

## 集合批量聚合与默认计数

- 对照固定 Laravel `Collection::loadAggregate`、模型 `$withCount` 及 Builder 的多关系计数、别名和投影测试。参考固定 Goravel 的连接级 ORM Query。检查旧版 `6d1a8192` Eloquent 和模板数据库用例，未找到相应批量集合聚合或模型默认关系计数，复用新版关系存在子查询、聚合投影和主键批处理。 [Laravel 聚合文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#deferred-count-loading) · [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel Builder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `loadAggregates` 接收可混用关系类型和聚合方式的 `RelationAggregateQuery` 列表。同一主键批次只执行一条 SELECT。拒绝重复别名及主键别名，全部批次成功后才同步目标属性和原始快照。
- `Model::withDefaultCounts` 在执行模型读取前构造默认关系计数投影。显式 `select` 取代默认投影，`without` 和 `withOnly` 只控制关系预加载。集合刷新保留默认计数，集合按键查询和延后加载聚合用基础查询。穿透及多对多读取保留相关模型的默认计数。
- SQLite 与真实 MySQL 8.4.13 验证同一批次同时加载存在、求和及另一关系计数，包含空关系、重复主键、别名冲突、原始属性同步及保留无关修改。MySQL 实测三项聚合共 1 次 SELECT，默认计数不增加模型读取的查询次数。默认计数专项覆盖集合刷新、显式投影及穿透读取。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建、12 项通用浏览器组合及 12 项新增章节锚点检查通过。

05-02 继续核对自动关系加载、默认关联模型及关系读取边界。

## 默认关联模型

- 对照固定 Laravel `SupportsDefaultModels`、四类单值关系的实例初始化、`DatabaseEloquentHasOneTest` 和 `DatabaseEloquentBelongsToTest` 的静态及动态默认值。参考固定 Goravel 的连接级 ORM Query。复用 `openrust-v0.x`（`6d1a8192`）的 `has_many.rs`、`belongs_to.rs` 与 `query_insert_ids.rs` 中“新实例继承关联连接、HasOne 预填外键、真实匹配优先”的规则。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#default-models) · [默认模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/SupportsDefaultModels.php) · [HasOne 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentHasOneTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `BelongsTo`、`HasOne`、`HasOneThrough`、`MorphOne` 共用 `withDefault` 配置。Rust 用类型化闭包统一表达空模型、属性和按父模型计算属性。闭包仅在缺失时执行，避免为批量查询中已匹配的行创建弃用实例。默认模型保持未持久化，存在和聚合查询继续读取数据库记录。`HasOne` 预填外键，`MorphOne` 同时预填多态别名。`BelongsTo` 和 `HasOneThrough` 保留关系键的空状态。
- SQLite 与真实 MySQL 8.4.13 验证空外键、悬空外键、父连接继承、穿透及多态默认键、实际匹配优先、择一关系预加载和存在查询。MySQL 批量读取含默认模型共 2 次 SELECT。三语示例均在空外键和真实数据库查无匹配两种情况下执行。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合及 6 项章节锚点检查通过。

05-02 下一批继续访问时关系自动加载及读取边界。

## 访问时批量加载

- 对照固定 Laravel 的 `Collection::withRelationshipAutoloading`、访问时触发和 `EloquentModelRelationAutoloadTest`，参考 Goravel 的连接级 ORM Query。旧版 `6d1a8192` 的 `database/eloquent/relations.rs` 支持显式分组加载，没有访问时回调。新版沿用 `loadMissing` 的批次匹配和缓存安装。 [Laravel 自动预加载](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#automatic-eager-loading) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 集合使用 `withRelationshipAutoloading` 启用，或由模型覆写 `automaticallyEagerLoadRelationships` 为该类型默认启用。Rust 在显式异步 `getRelationOrLoad` 触发加载，集合的可变借用保证缓存更新独占。已有值和空结果都会复用。未启用时只加载指定模型。筛选、合并及刷新保留集合开关。
- SQLite 与真实 MySQL 8.4.13 验证首次加载、部分已缓存、空结果复用、非法索引和输出类型、集合间隔离、SQL 失败不安装结果及后续重试。MySQL 对未加载部分只执行 1 次 SELECT，随后读取空结果不增加查询。三语示例各在双库运行，MySQL 包含父查询共 2 次 SELECT。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合及 6 项章节锚点检查通过。

当前访问时批处理以持有的集合为边界。跨已加载模型的关系链自动传播和禁止隐式加载策略仍待核对，嵌套关系可通过描述符显式预加载。

## 禁止延迟加载

- 对照固定 Laravel `HasAttributes::getRelationValue`、`handleLazyLoadingViolation` 及自动加载的上游测试。参考固定 Goravel 的连接级查询入口。检查 `openrust-v0.x`（`6d1a8192`）的 Eloquent 源码和模板测试，未找到延迟加载保护，继续沿用新版访问时加载及缓存。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#preventing-lazy-loading) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [自动加载测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `Model::preventsLazyLoading` 是按模型求值的 Rust 入口，保护隐式单模型读取。已加载关系及集合自动批量加载优先，显式 `load` 照常执行。默认违规处理允许未保存和刚创建的模型，其他模型返回包含类型及关系名的 `DatabaseError::LazyLoadingViolation`。可用 `handleLazyLoadingViolationUsing` 自定义处理或继续查询。
- SQLite 与真实 MySQL 8.4.13 验证单模型与集合拒绝、缓存优先、自动批量加载优先、显式加载、未保存与刚创建状态及自定义处理。MySQL 拒绝路径 0 次 SELECT，自动批量路径 1 次 SELECT。三语示例在两库实际执行，MySQL 显式读取示例共 2 次 SELECT。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合及 6 项新增章节锚点通过。

05-02 仍需核对已加载关系链的自动传播。

## 关系链批量加载

- 对照固定 Laravel `loadMissingRelationshipChain` 和 `EloquentModelRelationAutoloadTest` 的链式、异构和非模型值用例，参考固定 Goravel 的连接级查询。旧版 `6d1a8192:vendor/openrust/framework/src/database/eloquent/relations.rs` 的 `load_requested` 按路径分组，复用其逐层批处理职责。 [Laravel 自动预加载文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#automatic-eager-loading) · [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `relatedModels` 以类型化单值和多值关系展开当前批次。对每层的 `getRelationOrLoad` 合并读取缺失项，沿用已有模型缓存。共享对象去重，空关系跳过。Rust 用借用的 `RelatedBatch` 持有本次新增缓存，避免修改共享 `Arc` 模型。离开批次后，新增缓存不回写到原来的共享模型。
- SQLite 与真实 MySQL 8.4.13 验证已有父关系、两层链、共享对象去重、空值、SQL 失败和重试。MySQL 已加载第一层时，整批读取下一层为 1 次 SELECT。三语示例分别在两种数据库运行并验证结果。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合及 6 项章节锚点通过。

下一批核对混合 `MorphTo` 目标分组及其关系链和延后聚合。固定 Laravel 会向下传播模型访问回调，当前 Rust 链式读取由调用者持有批次。

## 多态目标分组与延后计数

- 对照固定 Laravel 集合 `loadMorph`、`loadMorphCount` 和两组多态延后加载测试，参考固定 Goravel 的连接级 ORM Query。检查 `openrust-v0.x`（`6d1a8192`）的 Eloquent 和模板测试，未找到对应多态延后入口。复用新版 `MorphTo` 的别名类型登记、关系批次及集合聚合查询。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#morph-to-deferred-count-loading) · [关系测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentMorphToLazyEagerLoadingTest.php) · [计数测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentMorphCountLazyEagerLoadingTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `loadMorph` 和 `loadMorphCount` 在父关系加载后按别名提取一个类型化目标组。每次调用一个组，结果通过返回批次读取。嵌套的 `morphModels` 可继续遍历多态链。空目标不进入批次，类型不符在加载前报错。延后计数复用集合的主键批量查询，批次的属性读取优先采用新计数。
- SQLite 与真实 MySQL 8.4.13 验证同主键跨类型隔离、错误类型、已加载目标复用及嵌套链。MySQL 实测目标已加载时，一个分组关系读取和一个分组计数各 1 次 SELECT。三语示例双库执行。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合及 6 项新增章节锚点通过。

05-02 下一步按交付清单收尾核对，重点检查按关系身份过滤及尚存 Laravel 行为差异。05-03 的前置 05-01 已验收。

## 关系身份筛选与交付核对

- 对照固定 Laravel `QueriesRelationships` 和 Builder 的自定义所属键、多模型及多对多身份测试。参考固定 Goravel 连接级查询。检查旧版 `6d1a8192` 的 Eloquent 和模板测试，未找到这些身份筛选入口，沿用现有关系键元数据、参数绑定及存在查询。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `whereBelongsTo` 使用描述符的外键与所属键，`whereAttachedTo` 通过中间表判断实际关联。多态身份由 `MorphTo::identity` 根据已注册类型和所属键提取，并由 `whereMorphedTo` 和 `whereNotMorphedTo` 按别名分组，`whereMorphType` 支持按别名或空类型筛选。SQLite 与真实 MySQL 8.4.13 实测自定义键、混合类型同键、分组、空类型、空外键和身份类型不符。
- 05-02 交付表列出的五项验收均有双库证据。外键及关联更新覆盖所属键写入、多态判别列和中间表事务失败回滚。预加载及 N+1 覆盖普通、穿透、约束和多态批处理，MySQL 三个父模型逐条读取 4 次 SELECT，直接预加载为 2 次。集合键覆盖过滤、比较、去重和重查。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语身份筛选示例双库执行、VitePress 构建、12 项通用浏览器组合及 6 项新增锚点检查通过。

05-02 交付表验收通过。仍记录 Laravel 扩展面差异，包含关系推断与字符串路径、部分 OR 变体、`whereAttachedTo` 的多态中间表入口、自动加载回调传播和混合目标在单次调用中配置多个类型。当前 Rust 使用类型化描述符和批次缓存。下一交付单元 05-03 为属性转换与模型序列化，前置 05-01 已验收。

## 独立补交复验

以 05-01 本地提交 `e898b923` 为基线。关系、加载和集合独立通过 Rust 1.94.0 的格式、435 项锁定离线工作区测试、全目标严格 Clippy 和 Artisan inspire。实际 SQLite 与 MySQL 8.4 关系测试通过。转换及序列化断言归入 05-03。
