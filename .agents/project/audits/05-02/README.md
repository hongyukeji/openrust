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

## 关系身份筛选的 OR 变体

- 按固定 Laravel `QueriesRelationships` 补齐 `orWhereBelongsTo`、`orWhereAttachedTo`、`orWhereMorphedTo` 和 `orWhereNotMorphedTo`。Rust 继续显式传入类型化描述符。多态 `MorphToMany` 复用中间表存在查询并保留类型判别条件。存在谓词与调用方条件进入同一组，外层仍应用全局作用域和软删除筛选。参考固定 Goravel 的连接级查询。旧版 `v0.x` 有 Pivot 关联实现，未发现对应身份 OR 入口。 [Laravel 查询源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#querying-relations)
- SQLite 与独立 MySQL 8.4.13 通过真实 SQL 验证自定义所属键、普通条件与关系 OR 混用、正反多态身份、空外键、相同键的多态 Pivot 区分以及软删除过滤。05-02 仍需核对其余 Pivot 契约及关系加载差异。
- Rust 1.94.0 格式、618 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。真实 `artisan inspire`、隔离三语构建与 18 项浏览器检查通过。

## 中间表过滤与插入默认值

对照固定 Laravel 的 `BelongsToMany`、`InteractsWithPivotTable` 及集成测试。参考固定 Goravel ORM 的连接级查询入口。复用 `v0.x` 的等值过滤和插入默认值规则，SQL 谓词由现有 Query Builder 生成。Rust 的 `wherePivot` 显式传比较运算符，区间用二元素数组，`withPivotValue` 用 `Result` 报告非法默认值。OR 条件按声明顺序编译为括号组，并与父模型及多态类型条件相交，避免写操作影响其他关联。插入默认值在显式属性之前应用，关系键始终受保护。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#filtering-queries-via-intermediate-table-columns) · [Laravel 关系测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 使用实际模型和数据库验收惰性及预加载、存在性查询、`attach` 与 `sync` 的插入默认值、显式覆盖、作用域内更新和解除关联，以及 OR 与多态归属隔离。中间表排序、时间戳和自定义 Pivot 模型仍分别跟踪。

Rust 1.94 格式、启用 Sanctum 的全工作区回归（621 次通过、0 失败，含进程夹具）、全目标严格 Clippy、实际 Artisan、三语示例编译、隔离文档构建及 18 项浏览器检查通过。提交前额度 93%，磁盘 60 GiB。05-02 仍有未闭合项目，核心账本维持 25/66。

## 中间表排序与时间戳

按固定 Laravel 的 `orderByPivot`、`orderByPivotDesc`、`withTimestamps` 和中间表写入测试实现。参考固定 Goravel ORM 的连接级查询，复用 `v0.x` 通过父模型格式生成时间戳、插入时显式值优先、更新时刷新更新时间的行为。Rust 以无参 `withTimestamps()` 使用父模型列名和标准回退，以 `withTimestampColumns` 的两个 `Option` 显式指定或省略列。排序在相关模型加载查询中生效，存在查询和写操作无需排序。 [Laravel 排序文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#ordering-queries-via-intermediate-table-columns) · [固定 Laravel 关系测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测惰性加载倒序、预加载正序、多态加载、显式插入时间覆盖、直接更新和同步刷新更新时间、创建时间保留及自定义时间戳列。自定义 Pivot 模型、其专用转换和生命周期仍是后续范围。

Rust 1.94 格式、启用 Sanctum 的工作区回归（621 次通过、0 失败，含进程夹具）、全目标严格 Clippy、实际 Artisan、三语示例编译及隔离构建、18 项浏览器检查通过。提交前额度 93%，磁盘可用 60 GiB。核心账本维持 25/66。

## 自定义中间表模型

按固定 Laravel 的 `using`、`AsPivot` 和 `InteractsWithPivotTable` 接入类型化中间模型。参考固定 Goravel 的连接级 ORM，复用 `v0.x` 将水合、插入、更新和删除分派到自定义类型的设计。Rust 使用 `using::<M>()` 静态声明，关系注入表、连接和原始复合身份。可选自增主键和多态类型也参与定位。读取复用 Eloquent 转换与序列化，写入复用模型生命周期和事务。自定义 Pivot 的软删除需通过普通 Eloquent 模型实现。Laravel 的 `as` 使用 Rust 原始标识符 `r#as`。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#defining-custom-intermediate-table-models) · [Laravel AsPivot](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/AsPivot.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和 MySQL 8.4.13 实测自定义访问器、属性转换、脏属性更新、attach/sync/detach 生命周期、监听器取消、事件与写操作同事务回滚，以及复合身份和多态类型隔离。自定义 Pivot 的反向关系自动水合按 Laravel 后续小节单独跟踪。

Rust 1.94 格式、启用 Sanctum 的工作区回归（621 次通过、0 失败，含进程夹具）、全目标严格 Clippy、实际 Artisan、三语示例编译、隔离构建和 18 项浏览器检查通过。提交前额度 93%，磁盘可用 60 GiB。05-02 仍待整体闭合，核心账本维持 25/66。

## 自动水合中间表反向关系

按固定 Laravel `SupportsPivotInverseRelations` 和 `EloquentBelongsToManyTest` 接入 `chaperone`、显式名称和 `withoutChaperone`。固定 Goravel 的连接级 ORM 为查询装配参考。复核 `v0.x`，已有自定义 Pivot 的关系容器与序列化入口。Rust 复用现行类型化 `LoadedRelations`，以 `Model::pivotInverseRelations` 声明可用名称，以 `chaperoneWith` 接收显式名称，避免运行时方法反射。反向引用保存已加载模型的快照，相关端快照在挂载当前 Pivot 前构造，以避免强引用和序列化循环。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#automatically-hydrating-pivot-relationships) · [固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/SupportsPivotInverseRelations.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和 MySQL 8.4.13 专项覆盖推断、显式名称、无效名称拒绝、停用、默认 Pivot 保持原行为、改名访问器、多态 Pivot、多父模型身份和 JSON 序列化。MySQL 通过同一连接的 `Com_select` 证明惰性加载 1 次、预加载 2 次，访问和序列化反向关系无额外 SELECT。05-02 余下关系契约继续跟踪，包括切换关联和触碰父模型时间戳。

Rust 1.94 格式、启用 Sanctum 的工作区回归（621 次通过、0 失败，含进程夹具）、全目标严格 Clippy、实际 Artisan、三语示例编译、隔离站点构建及 18 项浏览器检查通过。提交前额度 92%，磁盘可用 60 GiB。05-02 尚未整体闭合，核心账本 25/66。

## 切换关联与父模型时间戳

对照固定 Laravel 的 `toggle`、`toggleOrFail` 与 `touchIfTouching`，参考固定 Goravel ORM 的连接级装配。复用 `v0.x` 的按已有关系划分新增和删除、按类型派发自定义 Pivot 写入及通过父模型保存时间戳的做法。Rust 的 `toggle` 和 `toggleOrFail` 均使用现有事务路径，`toggleWithPivotAttributes` 接收按键分配的属性。通过 `withTouchingParent` 显式启用多对多声明端触碰，附加、更新、解除、同步及切换产生变更时，父模型和 Pivot 共用事务。父模型由已持久化快照构造，调用方重读后可取得最新值。 [Laravel 切换关联](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#toggling-associations) · [Laravel 触碰时间戳](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#touching-parent-timestamps) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

本批验收涵盖多对多关系的声明端触碰。相关端触碰继续跟踪。

SQLite 和独立 MySQL 8.4.13 验证同一调用的新增和解除、重复键去重、非法键拒绝、自定义 Pivot 插入失败后恢复已解除的关系、多态类型隔离、五种 Pivot 写入触碰、空操作和其他父模型隔离、调用方未保存属性保护。Rust 1.94 格式、621 次工作区测试、严格 Clippy、实际 Artisan、三语示例编译、隔离构建及 18 项浏览器检查通过。提交前额度 92%，磁盘可用 60 GiB。核心账本维持 25/66。

## 保存模型时触碰所属模型

对照固定 Laravel 的 `finishSave`、`touchOwners` 和传播回归用例。沿用 `v0.x` 在 `Saved` 后使用当前事务触碰所属模型的时机，参考固定 Goravel ORM 的连接级装配。Laravel 通过 `$touches` 保存关系名称，Rust 通过 `Model::touches()` 返回带类型的 `BelongsTo` 描述符。使用已持久化的外键及关系作用域，按模型类型、连接及主键去重。Laravel 的时间戳更新移除全局作用域，Rust 在查询和更新时均保留作用域，以维持租户隔离。显式事务中的所属查询、更新和事件共用一个连接。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#touching-parent-timestamps) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L2968) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测无变更保存、递归和循环关系、所属模型的 `Saved` 事件顺序、事务内可见性和回滚、缺失和空外键。多态所属关系、多对多相关端触碰及其他关系加载差异仍在 05-02 跟踪，核心账本 25/66。

Rust 1.94 格式、全目标严格 Clippy、启用 Sanctum 的工作区回归（621 次通过、0 失败，含进程夹具）、实际二进制 `artisan inspire`、三语示例编译、隔离站点构建和 18 项浏览器检查通过。Rust 1.99 的严格 Clippy 在既有分页和 Redis 限流代码上报告两项 lint。提交前周额度剩余 92%，项目卷可用 59 GiB。

## 保存模型时触碰多态所属模型

对照固定 Laravel `MorphTo::touch` 的空外键规则和 `HasRelationships::touchOwners` 的传播用例，参考固定 Goravel ORM 的连接级装配。复核 `v0.x` 的保存后触碰与事务回归，本批沿用已有时机。Rust 用 `Touches::morphTo` 接入既有别名表、所属键和类型约束，与 `BelongsTo` 共用触碰、事件与循环防护。类型和 ID 从已持久化属性解析，未注册别名按框架的多态查询契约返回错误。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php#L282) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L2968) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#touching-parent-timestamps) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测自定义所属键、别名切换、关系约束、空类型、未知别名、同事务可见性和回滚。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建和 18 项浏览器检查通过。提交前周额度剩余 92%，项目卷可用 60 GiB。核心账本仍为 25/66，多对多相关端触碰继续跟踪。

## 多对多变更时触碰相关模型

对照固定 Laravel 的 `BelongsToMany::touchIfTouching`、`touch` 和集成测试，复用 `v0.x` 批量更新当前关联模型及不派发逐模型保存事件的做法。参考固定 Goravel 的连接级事务。OpenRust 用 `withTouchingRelated` 在类型化描述符上声明 Pivot 写入时的触碰，并提供 `touch(&parent)` 主动触碰。更新以当前中间表键的 SQL 子查询限定范围，保留模型作用域、Pivot 过滤和多态类型。Pivot 写入与自动触碰共用事务。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L1416) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php#L1030) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#touching-parent-timestamps) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测默认关闭、主动触碰、五种 Pivot 写入、空操作、解除后范围、中间表过滤、软删除和多态类型隔离，以及相关端更新失败时回滚已插入的 Pivot。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建与 18 项浏览器检查通过。提交前周额度剩余 92%，项目卷可用 60 GiB。常规模型保存时声明多对多触碰仍待接入，核心账本维持 25/66。

## 保存模型时触碰多对多关系

对照固定 Laravel 的 `HasRelationships::touchOwners`、`BelongsToMany::touch` 和多对多保存测试。参考固定 Goravel 的连接级事务，复用 `v0.x` 的保存后触碰钩子及相关模型批量更新时间戳契约。Rust 以 `Touches::belongsToMany` 和 `Touches::morphToMany` 声明，使用已持久化的父键。批量更新沿用 Pivot 过滤、类型和模型作用域。相关模型自身还有触碰声明时继续传播，沿用已访问模型记录防止循环。对应 Laravel 集合分支，批量更新本身不派发相关模型的 `Saved` 事件。 [Laravel 传播源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L934) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php#L1030) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证无变更保存、普通和多态多对多触碰、Pivot 过滤、软删除隔离、循环传播、递归触碰其他父模型，以及显式事务内可见性和回滚。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离站点构建和 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB。Pivot 写入时从模型声明自动推断触碰仍待接入。核心账本维持 25/66。

## 从模型声明推断 Pivot 写入触碰

固定 Laravel 的 `touchIfTouching` 分别检查父模型的当前关系和相关模型的反向关系。OpenRust 使用已注册的类型化描述符识别正反向关系，匹配模型类型、中间表、键映射、多态判别条件和 Pivot 过滤。Pivot 过滤按编译后的 SQL 和绑定值匹配，因此同一声明和写入宜复用描述符。复用 `v0.x` 的双端触碰与事务路径，保留显式开关。参考固定 Goravel 的连接级事务。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php#L1049) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证正向附加、反向解除、过滤值和多态别名隔离、触碰失败时回滚 Pivot。首次反向测试在重置时间戳前加载父模型，旧快照影响断言。调整夹具顺序后两库复验通过。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建及 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB。动态关系和其他加载差异继续跟踪，核心账本 25/66。

## 动态关系解析

对照固定 Laravel 的 `resolveRelationUsing`、`relationResolver` 与动态关系测试，复核 `v0.x` 的 Eloquent 源码，未发现对应的运行时解析器。参考固定 Goravel 的应用连接装配。OpenRust 把解析器按模型类型与名称存入应用容器，已水合模型以连接所属应用为准。Rust 用显式描述符类型接入现有关系加载和序列化，重复登记替换后续解析，已加载值遵循既有缓存语义。PHP 的继承回退、魔术属性、按字符串的关系存在查询仍有差异。动态预加载按实例调用解析器，保留实例级条件，可能逐父模型查询。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L90) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentDynamicRelationsTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#dynamic-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证同模型同名解析器在两个应用间隔离、缺失和类型错误、重复登记、缓存与显式失效、序列化、实例级条件及动态预加载。首轮夹具把 `HasMany` 的本地键与外键顺序写反，纠正后双库复验通过。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建和 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB。核心账本维持 25/66。

## 动态关系存在查询与聚合

对照固定 Laravel 的 `getRelationWithoutConstraints`。查询在与 Builder 相同的连接上创建未保存模型，通过应用注册表解析具名关系，再复用现有相关子查询构建器。相同的 `DynamicRelation<R>` 可用于存在、缺失、计数及关系聚合。查询解析在 SQL 执行前完成，实例级预加载仍接收真实父模型。固定 Goravel 的连接级查询装配为参考，`v0.x` 未找到动态关系解析对应实现。Rust 使用显式描述符类型，模型结构体之间的 PHP 类继承回退仍有差异。 [Laravel 查询源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 动态关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#dynamic-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证同名跨应用隔离、缺失和描述符类型错误、正反存在、关系计数投影，以及使用未保存模型解析查询、使用真实父模型预加载的差异。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建及 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB，核心账本维持 25/66。

## 组合模型的动态关系回退

对照固定 Laravel `relationResolver` 和继承覆盖测试。Rust 的模型组合通过 `inheritRelationUsing` 显式指定基础模型适配和关系适配，`HasMany::forParent` 复用键与查询约束。同名本地登记优先，回退随基础模型解析器的登记生效，应用容器隔离贯穿解析链，循环返回错误。参考固定 Goravel 的连接级 ORM 装配，`v0.x` 未找到对应的动态解析器。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L90) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentDynamicRelationsTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证基础模型后注册、约束保留、关系存在查询及预加载、本地覆盖、应用隔离和循环拒绝。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建和 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB。关系字符串路径及剩余加载差异继续跟踪，核心账本 25/66。

## 点分关系路径的批量预加载

对照固定 Laravel `relationsNestedUnder`、`getRelation` 和嵌套预加载集成用例。复核 `v0.x` 的类型化关系及批量加载，没有发现对应的字符串路径入口。OpenRust 以 `Model::namedRelations()` 显式登记描述符，`withPaths` 和 `loadPaths` 合并同一次调用中的公共前缀，再复用各级已有的批量加载、查询约束及连接。参考固定 Goravel ORM 的连接级查询装配。跨多个目标类型的 `MorphTo` 继续通过类型化目标配置加载子关系。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#nested-eager-loading) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentMultiDimensionalArrayEagerLoadingTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测两级和三级路径、根路径与子路径混用、重复路径、多个兄弟节点、模型与集合延后加载、错误路径。MySQL 的共享前缀和重复路径用例共 4 次 SELECT，包括根模型和 3 个关系查询。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离构建和 18 项浏览器检查通过。提交前周额度剩余 91%，项目卷可用 60 GiB。路径存在查询和多态路径仍待跟踪，核心账本 25/66。

## 点分关系路径的存在与缺失查询

对照固定 Laravel `QueriesRelationships::hasNested` 和 Builder 的嵌套关系测试。OpenRust 复用 `namedRelations()` 中登记的类型化描述符和现有相关子查询。完整链的缺失使用最外层 `NOT EXISTS`，计数只比较最后一级，前面的层级使用存在判断。参考固定 Goravel 的连接级查询装配。复核 `v0.x`，未发现对应点分存在查询入口。多目标 `MorphTo` 仍使用显式目标查询接口。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php#L79) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php#L1667) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#querying-relationship-existence)

SQLite 和独立 MySQL 8.4.13 验证三级链、完整链缺失、末级计数和非法路径。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、文档调用编译、三语构建及 18 项浏览器检查通过。提交前周额度剩余 90%，项目卷可用 60 GiB。多目标多态路径仍待跟踪，核心账本 25/66。

## 多目标 MorphTo 的点分路径

对照固定 Laravel 的多目标嵌套存在查询测试和 `morphWith` 章节。OpenRust 的 `NamedRelations::morphTo` 使用已登记的目标别名，各目标分别解析同名子路径并批量预加载。存在与计数按别名组合相关子查询，缺失判断取完整分支的补集，包括空值和查无目标。参考固定 Goravel 连接级装配。复核 `v0.x` 未发现对应路径入口。各类型可为同一名称登记不同关系描述符，名称不同可继续用既有类型化目标配置。 [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#nested-eager-loading-morphto-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证两个别名、重复路径、同名异型子关系、空值和缺失目标、末级计数及错误路径。MySQL 批量预加载共 5 次 SELECT。首轮夹具缺少有效作者目标和达到计数门槛的记录，补齐后双库复验通过。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、文档调用编译、三语构建与 18 项浏览器检查通过。提交前周额度剩余 90%，项目卷可用 60 GiB，核心账本 25/66。

## 从数据库发现多态路径目标

对照固定 Laravel 的 `hasMorph('*')` 和多目标嵌套查询测试。异步路径入口先通过已有 `MorphTo::discover_types` 从各多态段的模型表读取实际别名，再复用现有相关子查询。发现按模型类型和关系名称在单次构建中复用，独立于外层筛选与作用域。Rust 通过 `whereHasPathAll`、`whereDoesntHavePathAll` 和 `hasPathCountAll` 显式区分需要数据库查询的类型发现。已存储的未知别名按现有多态契约返回错误。参考固定 Goravel 的连接级查询装配，复核 `v0.x` 未找到对应通配路径实现。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php#L232) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证两个已存储别名、忽略未出现别名的子路径、缺失和空值、末级计数，以及软删除记录中未知别名仍触发错误。MySQL 通配路径共 2 次 SELECT，一次发现和一次结果查询。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、文档调用编译、三语构建和 18 项浏览器检查通过。提交前周额度剩余 90%，项目卷可用 60 GiB，核心账本 25/66。

## 沿路径补齐缺失关系

对照固定 Laravel 的 `Collection::loadMissingRelation`、`loadMissingRelationshipChain` 及自动加载集成测试。模型和集合通过 `loadMissingPaths` 复用已登记路径，按层批量补齐缺失结果，多态段按别名和目标类型处理。Rust 的共享模型使用 `Arc`，已加载关系上的新后代安装到当前关系图的快照。之前取出的引用仍读取原快照。参考固定 Goravel 的连接级查询装配，复核 `v0.x` 未找到同类路径补齐实现。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L263) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证普通三级路径、重用已加载的父级、模型和集合入口、多态目标分组、序列化和重复调用。MySQL 普通链只发出 1 次缺失后代查询，多态链为两个目标类型各发出 1 次。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、文档调用编译、三语构建及 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB，核心账本 25/66。

## 从集合借用单模型触发自动预加载

对照固定 Laravel 的 `withRelationshipAutoloading`、`attemptToAutoloadRelation` 和自动预加载集成测试。OpenRust 的 `Collection::modelAt` 在一次异步访问中可变借用所属集合，并通过 `getRelationOrLoad` 批量补齐当前集合的缺失关系。借用结束后，同集合中的其他模型可读取已加载的关系。移出集合的模型遵循单模型加载策略。后代链使用 `relatedModels` 或 `loadMissingPaths`。PHP 的对象回调可沿后代传播并在模型脱离集合后继续引用根集合，这两个行为仍单独跟踪。参考固定 Goravel 的连接级 ORM 装配，复核 `v0.x` 未发现关系自动加载回调。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#automatic-eager-loading) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测集合借用、索引越界、SQL 失败后重试、空值复用、默认关闭及集合隔离。MySQL 首次访问缺失关系只发出 1 次 SELECT。Rust 1.94 格式、全目标严格 Clippy、621 次工作区测试、实际 `artisan inspire`、三语示例编译、隔离站点构建和 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的主键查找

对照固定 Laravel 集合的 `find`、`findOrFail`、`contains` 和 `doesntContain` 及其回归用例。Rust 以 `findMany` 和 `findManyOrFail` 表达 PHP 同名方法的数组输入分支，使用现有主键规范化、模型引用及 `DatabaseError::ModelNotFound`。多键结果保留集合顺序及同键模型，缺失检测按键做集合差，重复的模型行不会掩盖缺失的请求键。Laravel 使用 PHP 的宽松比较及结果计数，本批采用框架已有的主键字典和表名错误。模型对象、任意字段和谓词重载仍待对账。参考固定 Goravel 的连接级 ORM 装配，复核 `v0.x` 的查询 Builder 和 Eloquent 集合文件，旧版的集合 Cast 与本批模型查找职责不同。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md#available-methods) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 对查询水合结果实测单键与多键命中、缺失错误、空输入、重复键、结果顺序和重复模型不会掩盖缺失键。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的序列化设置

对照固定 Laravel 的集合 `mergeHidden`、`mergeVisible`、`setAppends`、`withoutAppends` 和对应的整集合测试。Rust 集合保留 Laravel 方法名，接收字段名切片，返回可链式调用的集合可变引用。实现复用现有模型逻辑，隐藏规则继续作用于访问器结果，清除追加项同时覆盖模型默认设置。复核 `v0.x` 模型的 `set_appends`、`without_appends` 及属性存储回归用例，当前模型已具备替换和清除语义。参考固定 Goravel ORM 的连接装配。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L548) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L554) · [Laravel 序列化文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-serialization.md#appending-values-to-json) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 使用两条真实查询记录验证逐模型合并、隐藏优先、替换及清除默认追加项、JSON 输出与独立查询隔离。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的模型身份与谓词

对照固定 Laravel 的 `Model::is`、`isNot` 及集合 `find`、`findOrFail`、`contains` 和 `doesntContain` 的对象与闭包分支。Rust 保留模型比较方法名，并用 `findModel`、`findModelOrFail`、`containsModel`、`doesntContainModel`、`containsWhere` 和 `doesntContainWhere` 表达 PHP 的参数重载。查找使用传入模型的键，身份比较使用严格类型的键、表名及连接名。集合以向量存放模型，谓词接收当前的零基索引。字段和操作符重载可通过类型化谓词表达。复核 `v0.x` 的 Eloquent 模型与 Support 集合，未找到按 Eloquent 身份比较的集合入口。Goravel 固定版用于核对 ORM 连接装配。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测相同身份、同键不同表、连接名不同、同表不同 Rust 模型类型、主键类型、按模型键查找、缺键及谓词短路。Rust 1.94 格式、全目标严格 Clippy、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。首轮全量在既有 SQLite Session 测试的密文子串断言失败，未改动 Session，原样复跑 619 次工作区测试全部通过。该断言的随机碰撞已记录为 05-08 测试遗留。提交前周额度剩余 89%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合映射与类型转换

对照固定 Laravel 的 `map`、`mapWithKeys`、`toBase`、`pluck` 和 `keys` 及集合回归测试。Laravel 依据映射结果在运行时选定集合类型。Rust 的 `map` 和 `mapWithKeys` 返回 Support 集合，`mapModels` 返回 Eloquent 集合并沿用已启用的自动预加载。Eloquent 集合使用向量索引，自定义键的结果保存在 Support 集合。带键投影复用现有 Support 集合的键归一化和覆盖规则。复核 `v0.x` 的 Support 集合映射与 Eloquent 集合 Cast，当前 Support 集合已提供类型化条目构造。参考固定 Goravel 的连接级 ORM 装配。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L339) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md#eloquent-collection-conversion) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证实际查询顺序、键投影、重复键覆盖、空集合和模型结果保留 Eloquent 主键接口。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB。后代访问触发根集合自动加载、保留自定义键的 Eloquent 集合以及其他转换方法继续跟踪。核心账本 25/66。

## 带键的 Eloquent 集合

对照固定 Laravel 的 `mapWithKeys`、`fresh` 和集合转换测试。Rust 通过 `mapModelsWithKeys` 和 `fromEntries` 构造带键模型集合，复用 Support 集合的键规范化和重复键覆盖规则。显式键与连续模型存储并行，默认查询结果仍隐式使用位置键。显式键随 `map`、`mapModels`、`fresh`、`toBase` 和 JSON 保留。Rust 原有位置访问和返回向量的 `all`、`toArray` 契约继续使用，`getByKey` 提供集合键访问。主键集合运算按 Laravel 重新编号。复核 `v0.x` 的 Support 键值集合和 Eloquent 集合 Cast，当前 Support 集合已有可复用的条目接口。参考固定 Goravel 的连接级 ORM 装配。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L414) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L366) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md#eloquent-collection-conversion) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 使用真实查询模型验证带键映射、重复键覆盖、数字字符串规范化、位置访问、JSON 对象、刷新及转换后键保留。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 89%，项目卷可用 60 GiB。核心账本 25/66，后代访问触发根集合自动加载继续跟踪。

## 从关联模型触发根集合自动预加载

对照固定 Laravel 的关联回调传播、`loadMissingRelationshipChain` 和自动预加载集成测试。Rust 用 `CollectionModel::relatedModelAt` 将关联模型访问绑定到根集合的可变借用。首次缺失访问合并同一父关系的缺失目标，复用类型化批量加载，并通过现有 `RelatedModels` 接口安装新快照。共享模型按实例去重。已持有的 `Arc` 仍可读取原快照。关闭自动预加载时沿用所选模型的延迟加载规则。该入口覆盖普通单值和多值父关系下的一层关联访问，任意深度借用、多态目标入口和脱离集合后的对象回调仍跟踪。复核 `v0.x` 未找到自动预加载回调，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 加载链](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L242) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证部分已加载父关系、SQL 失败后重试、不同根模型上的后代复用、空关系、关闭批量时的单模型读取以及根集合序列化。MySQL 在父关系已补齐后只发出 1 次后代 SELECT。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 88%，项目卷可用 60 GiB。核心账本 25/66。

## 多态关联目标触发根集合自动预加载

对照固定 Laravel 的关联回调传播和 `loadMissingRelationshipChain`。Rust 用 `CollectionModel::morphModel` 选择登记的别名和目标类型，持有根集合的可变借用。首次访问缺失关系时，仅批量查询该别名下的缺失目标。普通和多态父关系共用按实例去重、批量加载和单模型延迟加载逻辑。加载后替换当前关系图中的共享快照。Laravel 回调按模型类筛选，Rust 此入口显式按别名分组，同一 Rust 类型登记多个别名时各组独立。复核 `v0.x` 未找到自动预加载回调，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 回调传播](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L182) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证混合别名、别名不匹配返回空、跨多个根模型写回，以及其他别名保持原状态。MySQL 对目标后代只发出 1 次 SELECT。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 88%，项目卷可用 60 GiB。核心账本 25/66。

## 多层关联借用传播根集合预加载

对照固定 Laravel 的关联回调传播、缺失关系链加载和多层多态集成测试。OpenRust 的普通与多态借用共用关系链，每一层可继续调用 `relatedModelAt` 或 `morphModel`。缺失访问从根集合开始按层收集模型，按实例去重，最后向根集合逐层写回快照。Rust 用可变借用表达集合所有权，已有 `Arc` 保留原快照，多态分组沿用登记别名。脱离集合后继续使用 PHP 式对象回调仍待跟踪。复核 `v0.x` 未找到关联自动加载回调，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 加载链](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L242) · [Laravel 多层测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证 `subject.author.posts` 混合多态、单值和多值关系，跨根模型复用并写回序列化结果。MySQL 对尚未加载的作者和文章分别发出 1 次 SELECT，后续访问复用结果。原有普通、多态、禁用及失败重试用例通过。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 88%，项目卷可用 60 GiB。核心账本 25/66。

## 移出集合的模型继续批量加载

对照固定 Laravel 集合自动预加载及其测试。Rust 在消费集合时，为移出的根模型保留共享的同批快照。关系首次访问由同批模型共同查询，各模型在自己的访问点安装结果，保留现有借用接口。异步互斥保护并发首次访问，SQL 失败可重试。显式卸载关系或更改属性和连接后，改用当前模型查询。快照保留模型状态，PHP 对象的即时共享修改和脱离集合的后代自动传播仍需跟踪。复核 `v0.x` 未找到集合自动加载回调，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 自动加载测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证移出集合后仍可加载、SQL 失败可重试、并发访问和空结果复用、显式卸载及主键变化。MySQL 两个并发访问与第三个空结果共用 1 次 SELECT。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 88%，项目卷可用 60 GiB。核心账本 25/66。

## 移出集合的后代继续批量加载

对照固定 Laravel 的关联回调传播和多层多态集成测试。Rust 的 `getRelationOrLoad` 将普通关系输出按实例去重并分配后代批次，多态关系按登记别名分组。已预加载的关系同样在首次访问时建立后代批次。独立模型通过状态快照共享查询，每个模型在访问时安装自己的结果，已持有的 `Arc` 保留原快照。自定义关系可实现输出传播钩子，默认保留原返回行为。PHP 对象的即时共享修改仍单独跟踪。复核 `v0.x` 未找到关联自动加载回调，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 回调传播](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L182) · [Laravel 多层测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证普通后代的失败重试及并发复用，以及预先加载的多态目标在移出后的批量加载。MySQL 两组用例分别只执行 1 次后代 SELECT。Rust 1.94 格式、全目标严格 Clippy、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。首轮全量遇到 SQLite 队列延迟用例 `LostReservation`，原样复跑 619 次工作区测试全部通过。队列用例的时间窗口仍需单独复核。提交前周额度剩余 88%，项目卷可用 60 GiB。核心账本 25/66。

## 批量关系结果的同步可见性

对照固定 Laravel 集合自动加载测试中其他模型立即报告关系已加载的断言。OpenRust 的移出模型共享只追加的关系结果。首次查询完成后，同批模型的 `relationLoaded`、`getRelation` 和序列化可直接读取结果，嵌套序列化也可读取已发布的后代关系。已借用的返回值由不可变条目持有，写入新关系无需更换已有条目。显式卸载和本地覆盖优先于共享结果，模型属性或连接改变时使用当前模型状态。普通属性写入仍属于各模型的 Rust 值。复核 `v0.x` 未找到对应共享关系缓存，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelRelationAutoloadTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测首次加载后同批模型同步可见、后代序列化、空结果和本地卸载隔离，现有并发及失败重试回归通过。MySQL 仍为每层 1 次 SELECT。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 87%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合分区、计数和重复项

对照固定 Laravel 的 `partition`、`countBy`、`duplicates`、`duplicatesStrict` 和 `unique`。分区保留两个子集合的键和自动加载设置，分组计数复用 Support 集合。无投影的两种重复项查询使用既有 `Model::is` 判断身份。Rust 使用 `duplicatesBy` 和 `uniqueBy` 表达投影重载，投影类型定义相等规则。分区用元组表达固定的两个结果，两个成员均保留 Eloquent 接口。复核 `v0.x` 的 Support 集合，未找到可提取的对应分区和重复项方法。参考固定 Goravel 的连接级 ORM 装配。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L711) · [Laravel 集合测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L409) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测分区保留键、分组计数、重复模型保留原位置、投影重复值和按投影去重。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 87%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的位置转换

对照固定 Laravel Eloquent Collection 的 `zip`、`pad`、`flatten` 及返回 Support 集合的回归用例。Rust 的两列 `zip` 用元组和 `Option` 表达异构值及缺项。填充复用 Support 集合，按 PHP 数组填充规则保留字符串键并重排整数键，未达到填充条件时保留原键。Eloquent 的填充值来自独立模型快照。类型化模型在扁平转换中是叶节点，嵌套 Support 集合可复用 `collapse`。多输入配对和类型化键值交换继续跟踪。复核 `v0.x` 的 Support 集合未找到可直接提取的对应方法，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 集合测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L618) · [PHP 填充实现](https://github.com/php/php-src/blob/php-8.5.0/ext/standard/array.c) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测实际查询模型的位置配对、两侧缺项、左右填充、未填充时原键保留、混合键重排及扁平后的顺序。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 87%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的多输入配对与键值交换

对照固定 Laravel 的可变参数 `zip`、`flip`、Support 回归和 Eloquent 返回类型测试。Rust 的 `zipMany` 接收同一元素类型的任意数量输入列，模型列保持独立类型，各列用 `Option` 表达缺项。Support 的 `flip` 使用 `ArrayKey`，Eloquent 的 `flipBy` 以闭包表达从模型取得标量键，`None` 跳过，重复键采用最后写入值。Laravel 的异构附加列可通过 Rust 枚举统一表示。复核 `v0.x` 的 Support 集合未发现可直接复用的对应方法，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Collection.php#L1775) · [Laravel Support 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php) · [Laravel Eloquent 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L618) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 用真实查询模型验证异长多列、空列、无附加列、数字字符串键、跳过项及重复键覆盖。Rust 1.94 格式、全目标严格 Clippy、619 次默认特性工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。提交前周额度剩余 87%，项目卷可用 60 GiB。核心账本 25/66。

## Eloquent 集合的队列元数据

对照固定 Laravel 的 `QueueableCollection`、`QueueableEntity` 和集合回归用例。集合以 `Collection<M>` 在编译时约束模型类型，运行时类型名只用于诊断。主键保持集合顺序、重复项及空值。关系快照保留原始注册名，按模型、嵌套集合和多态目标递归取得已加载路径，并对集合取交集。批量自动加载发布的快照也参与计算。读取不触发查询、不经过 JSON 可见性，也不收录内部 Pivot 投影。复核 `v0.x` 未发现可直接复用的队列模型标识实现，参考固定 Goravel 的连接级 ORM 装配。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L876) · [Laravel 集合测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php#L655) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

队列任务仍通过编译注册和 JSON 载荷传递参数。需要继续按固定 Laravel 核对模型标识的捕获和恢复，包括写连接读取、缺失模型、集合顺序和关系重载。 [Laravel 队列恢复源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SerializesAndRestoresModelIdentifiers.php)

SQLite 和独立 MySQL 8.4.13 验证实际模型的键顺序与重复项、普通和多态嵌套路径、已加载空关系、交集、隐藏设置独立、自动加载后的元数据、空集合和混合连接错误。MySQL 会话计数确认读取元数据新增 0 次 SELECT。Rust 1.94 格式、全目标严格 Clippy、619 次工作区测试、实际 `artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。首轮全量测试的 MySQL SELECT 断言误写为 1 次，修正后完整复验通过。提交前周额度剩余 86%，项目卷可用 60 GiB。核心账本 25/66。

## 队列任务恢复 Eloquent 模型

对照固定 Laravel 的 `SerializesAndRestoresModelIdentifiers`、`newQueryForRestoration` 和模型序列化集成测试。Rust 以 `ModelIdentifier<M>` 和 `ModelCollectionIdentifier<M>` 表达显式 JSON 字段，编译注册的处理器选定模型类型。捕获时保存带类型的主键、数据库连接名和已加载关系路径。恢复时从当前应用解析连接，主模型查询使用写连接并跳过全局作用域，保留模型默认预加载，再补载登记的关系。集合跳过缺失行，保留原顺序及重复键。单模型缺失返回 `ModelNotFound`，由处理器处理。已检查旧分支 `v0.x`，无对应模型标识代码可提取。Goravel 的任务签名与参数契约用于对照编译注册边界。 [Laravel 恢复源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SerializesAndRestoresModelIdentifiers.php) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Queue/ModelSerializationTest.php) · [Goravel 任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)

Laravel 的任务级缺失模型自动删除及自定义 Pivot 标识恢复仍待单独核对。

在 SQLite 和独立 MySQL 8.4.13 上，用实际 `queue:work --queue=model-restoration --once` 运行编译注册的任务。派发后更新并软删除单个模型、硬删除集合中的一行、插入新的关联行。独立 Worker 成功读到更新值和新增关系，集合保留顺序及重复键，并验证关系省略与单模型缺失错误。SQLite 的读写分离夹具额外验证陈旧副本不会用于主模型恢复。Rust 1.94 全目标严格 Clippy、格式、620 次工作区测试、实际 `artisan inspire`、三语文档示例编译、站点构建和 18 项浏览器检查通过。提交前周额度剩余 86%，项目卷可用 60 GiB。

## 队列任务的缺失模型策略

对照固定 Laravel `CallQueuedHandler::handleModelNotFound`。Rust 处理器显式调用 `ModelIdentifier::restoreForJob` 保留模型恢复错误，派发时通过 `Payload::deleteWhenMissingModels` 设置策略。默认立即记录失败并释放唯一锁。静默确认同样释放唯一锁，并把当前批次成员记为成功。任务链不会从缺失模型继续，批次记录保留这一决定供重复投递复用。批次的未运行后续成员保留原待处理状态。普通查询和其他数据库错误仍使用原有契约。复核 `v0.x` 未发现对应实现，参考固定 Goravel 的编译注册任务签名。 [Laravel 处理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/CallQueuedHandler.php#L304) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md#ignoring-missing-models) · [Goravel 任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)

SQLite 与独立 MySQL 8.4.13 实测派发后删除模型、首次消费即失败、显式丢弃、唯一任务可再次派发、同步与持久链停止、批次成功记账与重复投递、硬超时隔离进程往返。全量工作区验收 620 项通过，随后用最终夹具补跑双库硬超时专项和 MySQL 批次专项。Rust 1.94 格式、全目标严格 Clippy、`artisan inspire`、三语示例编译、站点构建和 18 项浏览器检查通过。自定义 Pivot 模型标识恢复继续跟踪。

## 自定义 Pivot 的队列恢复

对照固定 Laravel `AsPivot`、`MorphPivot` 和 `EloquentPivotSerializationTest`。有独立主键时按主键恢复，否则按两端键恢复，多态 Pivot 同时匹配鉴别值。Rust 将键分开保存为类型化 JSON，并要求模型通过 `queueablePivots` 登记相同的关系定义，避免从载荷直接选择任意表和列。Worker 从写连接读取，并在模型水合时恢复中间表、关系键及时间戳设置。Pivot 集合返回查询匹配的记录，不重复恢复同一行，也不重放捕获的关系路径。旧版 Pivot 装配已有可复用的关系身份设计，当前实现沿用现行水合和持久化代码。固定 Goravel 的任务签名作为编译注册参考。 [Laravel Pivot](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/AsPivot.php#L258) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentPivotSerializationTest.php) · [Goravel 任务契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/queue/job.go)

SQLite 与独立 MySQL 8.4.13 通过 `queue:work --queue=pivot-restoration --once` 验收独立主键、复合键、多态同键隔离、派发后删除、集合重复键、未登记表拒绝，以及恢复后更新和删除。全量工作区 620 项通过，最终形态另通过双库关系和 Worker 专项。Rust 1.94 格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建与 18 项浏览器检查通过。05-02 仍需按固定关系和集合基线复核剩余差异。

## Eloquent 集合的主键字典

对照固定 Laravel 的 `Collection::getDictionary` 及合并、差集和交集测试，新增公开的 `getDictionary()`，与 `merge`、`unique`、`only`、`except` 共用索引构造。空主键跳过，重复主键保留首次出现的位置和最后出现的值。Rust 使用有序字节键和模型借用，保留二进制主键并避免复制模型。复核旧版 `v0.x` 未找到可直接复用的主键字典。固定 Goravel 的连接级 ORM 作为数据来源与装配对照。 [Laravel 集合源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php#L684) · [集合测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

继续核对关系存在查询的入口。固定 Laravel 的 `whereRelation`、`withWhereRelation` 及普通和多态关系的 OR、缺失变体尚无全部对应公开入口。现有 `whereHas`、`withWhereHas`、`whereHasMorph` 和 SQL 子查询构造可复用。集合的 `collapse` 已由 Support 集合承担，`flipBy` 由投影明确标量键，自动预加载的链式遍历继续由类型化关系批次执行。 [Laravel 关系查询源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php#L448)

SQLite 与独立 MySQL 8.4.13 验证有序主键、整数与数字字符串归一、后项覆盖、空集合、跳过空主键、保留二进制键及借用原模型。Rust 1.94 格式、全目标严格 Clippy、620 项工作区测试、实际 Artisan、三语示例编译、VitePress 构建及 18 项浏览器检查通过。

## 按关联列筛选及 OR 存在查询

对照固定 Laravel 的 `QueriesRelationships` 和 `EloquentWhereHasTest`。普通关系支持 `whereRelation`、`withWhereRelation`、缺失及 OR 变体。类型化关系追加列条件时保留已有约束，筛选和预加载共用同一描述符。多态查询在已登记别名的目标子查询追加列条件，复用原有计数和存在表达式。OR 与父模型的普通谓词一起组合，全局作用域仍约束整个组合。复核旧版 `v0.x` 未找到可提取的同名入口，复用现行关系 SQL。固定 Goravel ORM 为连接级查询参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentWhereHasTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

本批使用类型化关系描述符。Laravel 的字符串点分路径列筛选、列条件闭包重载仍需分别核对。现有命名路径存在与计数查询继续使用 `whereHasPath`、`whereDoesntHavePath` 和 `hasPathCount`。

SQLite 和独立 MySQL 8.4.13 验证普通与多态列筛选、缺失、OR、自关联别名、软删除、未知多态别名和筛选后预加载。首轮全量触发关系集成测试的 Debug 栈溢出。将新增验收拆为独立堆分配的异步夹具后，双库专项和全量 620 项通过。Rust 1.94 格式、全目标严格 Clippy、Artisan、三语示例编译、VitePress 构建和 18 项浏览器检查通过。

## 命名关系路径的末级条件

按固定 Laravel `hasNested` 和 `EloquentWhereHasTest` 处理点分路径。列条件和闭包作用于末级关系。否定匹配没有任何完整路径满足条件的父模型，OR 在父查询中组合整条路径。预加载沿原路径执行，仅末级使用同一条件。Rust 用类型擦除的查询条件闭包接受末级表限定名，以便支持自关联别名和多态分支。核对旧版 v0.x 后沿用当前命名关系注册与 SQL 子查询机制，参考 Goravel 的连接级查询入口。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php#L79) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentWhereHasTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证三段路径、OR 组合、整条路径缺失、末级闭包、自关联表限定、多态末级和条件预加载。全量 620 项及双库专项通过，8 项测试夹具维持忽略。Rust 1.94 格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多态通配路径的末级筛选与预加载

组合固定 Laravel 的通配别名发现、路径末级条件和按多态目标约束预加载。OpenRust 在异步路径入口读取实际别名，并将同一组别名用于生成相关子查询和准备预加载。末级支持列条件或分组闭包，缺失查询包含空多态目标。Rust 以异步入口显式区分额外数据库查询，目标注册仍使用编译期类型。已发现的目标须支持余下的路径。固定 Goravel 继续作为连接级装配参考，旧版 `v0.x` 未找到对应的通配路径约束。 [Laravel 关系查询](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/QueriesRelationships.php#L232) · [Laravel 多态预加载](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证混合目标、未出现在数据中的已登记目标、空目标缺失匹配、末级闭包筛选及同条件预加载。首轮 MySQL 专项触发测试夹具的 Debug 栈溢出，将夹具中的查询 future 放到堆上后双库专项通过。

Rust 1.94 格式、全目标严格 Clippy、工作区 620 项（8 项夹具忽略）、实际 `artisan inspire`、三语示例编译、站点构建和 18 项浏览器检查通过。核心账本仍为 25/66。

## 集合刷新时预加载多个命名关系

对照固定 Laravel 集合的 `fresh($with)`、空集合测试与 Collections 文档，集合刷新可指定多个关系和点分路径。Rust 保留无参 `fresh()` 和接受类型化描述符的 `freshWith()`，通过 `freshWithPaths(&[...])` 表达 Laravel 的路径列表。显式路径覆盖同名的默认预加载，其余默认预加载继续生效。原集合保留原来的属性和关系。固定 Goravel 用作连接级装配参考。旧版 `v0.x` 的单模型 `fresh_with` 已支持关系路径，本批沿用其刷新后加载的职责划分并复用现有集合水合与路径注册。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentCollectionTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md#method-fresh) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证多个子路径、重复路径、默认预加载覆盖、原集合保留、空集合及错误路径。MySQL 的两条子路径与一个其他默认关系共 5 次 SELECT，包括主模型回读。

Rust 1.94 格式、全目标严格 Clippy、工作区 620 项（8 项夹具忽略）、实际 `artisan inspire`、三语示例编译、站点构建和 18 项浏览器检查通过。核心账本仍为 25/66。

## 多态目标延后聚合与写回

按固定 Laravel 的 `Collection::loadMorphCount`、`Model::loadMorphAggregate` 及关系文档实现延后聚合写回。Rust 显式传入登记别名和类型化聚合描述符，复用现有批量聚合查询。每个主键批次共用查询，结果作为原始属性写回父模型缓存的目标快照，父模型 JSON 可读取。其他类型的同键目标不参与写回。先完成查询与校验再安装快照，避免聚合错误导致部分写回。已持有的目标引用仍指向旧快照。核对 `v0.x` 未找到同名多态聚合实现，固定 Goravel 用作连接级 ORM 参考。 [Laravel 集合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Laravel 模型](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#deferred-count-loading) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与 MySQL 8.4.13 验证父关系与 JSON 可见性、别名隔离、原始属性同步、重复聚合别名和目标类型错误。MySQL 为同组两个聚合执行一次 SELECT。普通多态延后加载 `loadMorph` 的缓存写回仍待核对。

Rust 1.94 全工作区 620 项通过、8 项夹具忽略。双库关系专项、格式、全目标严格 Clippy、实际 `artisan inspire`、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多态目标延后关系加载与写回

对照固定 Laravel 的 `Model::loadMorph`、`Collection::loadMorph` 及 Nested Lazy Eager Loading and morphTo。Rust 每次显式选择登记别名，接受类型化关系或通过 `loadMorphPaths` 接受多个已登记路径。关系和聚合共用目标快照替换，每个共享目标只复制一次，查询全部成功后更新父模型的关系缓存。已持有的目标引用继续指向旧快照。旧版 `v0.x` 未找到直接对应入口，固定 Goravel 用作连接级 ORM 参考。 [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#nested-lazy-eager-loading-morphto) · [Laravel 集合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Collection.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 实测已有关系覆盖、嵌套路径、同主键不同别名隔离、父模型 JSON、旧引用及 SQL 失败后保留关系。MySQL 同组重载一次 SELECT。SQLite 首轮测试的未限定列名被当作字面量，改用限定列名后双库失败路径均通过。最终工作区 620 项通过、8 项夹具忽略，Rust 1.94 格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建与 18 项浏览器检查通过。

## 声明端按多态目标预加载

对照固定 Laravel 的 `MorphTo::morphWith`、`morphWithCount`、`constrain` 与 `EloquentMorphEagerLoadingTest`。Rust 通过登记别名和类型参数选择目标，`morphWith` 接收类型化预加载描述符，`morphWithPaths` 接收已登记路径。同一目标类型和所有者键的预加载、计数及约束可组合。约束单独保存，最后应用到目标查询。旧版 `v0.x` 未找到可直接复用的同名接口。固定 Goravel 的连接级 ORM 继续用于装配参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentMorphEagerLoadingTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#nested-eager-loading-morphto-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证类型化关系、登记路径、计数和约束双向声明顺序，错误类型拒绝，以及筛选排除后的空目标。Rust 1.94 工作区 620 项通过、8 项夹具忽略，格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多态目标软删除选项

对照固定 Laravel 的 `MorphTo::withTrashed`、`withoutTrashed`、`onlyTrashed` 与 `EloquentMorphEagerLoadingTest`。Rust 对声明了软删除列的目标应用选项，并在目标约束之前运行。显式空值和非空值条件可以叠加，后续移除默认作用域时继续保留。未声明软删除列的目标仍按其普通查询加载。实现复用类型化关系批次及写事务路径。旧版 `v0.x` 未找到直接对应的混合目标实现，固定 Goravel 继续用作连接级 ORM 参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentMorphEagerLoadingTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#constraining-eager-loading-of-morph-to-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测混合目标预加载、软删除目标单笔读取、显式条件叠加及目标约束顺序。全工作区 620 项通过、8 项夹具忽略，Rust 1.94 格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多态目标默认模型

对照固定 Laravel 的 `MorphTo::newRelatedInstanceFor`、`SupportsDefaultModels` 和 `DatabaseEloquentMorphToTest`。Rust 通过 `withDefault::<Target>(alias, callback)` 绑定登记类型。关联键缺失、记录找不到或查询约束排除时，使用对应别名构造未保存模型。空类型通过 `defaultForMissingType` 显式指定默认别名，PHP 在空类型时使用声明端的模型类型。默认值只用于关系结果，存在查询仍以数据库记录为准。复用普通关系的默认模型构造及连接解析。旧版 `v0.x` 未找到直接对应的多态默认模型，固定 Goravel 用作连接级 ORM 参考。 [Laravel MorphTo](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentMorphToTest.php) · [Laravel 默认模型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#default-models) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证批量预加载、真实目标优先、被软删除作用域排除、缺失外键目标、空类型选型、未知别名错误、连接继承和 JSON。Rust 1.94 全工作区 620 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多态关联赋值与关系缓存

对照固定 Laravel 的 `MorphTo::associate`、`dissociate` 与 `DatabaseEloquentMorphToTest`。Rust 通过 `named` 指定关系缓存键，并继续使用已登记的别名及所有者键。赋值缓存传入模型的快照，解除关联缓存已加载的空值。其他已加载关系仍可读取。调用者负责保存父模型。匿名描述符保留既有清除缓存行为。复用 `v0.x` 的按名更新关系和独立保存契约。固定 Goravel 用作连接级 ORM 参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentMorphToTest.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#belongs-to-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证缓存快照与 JSON、无额外查询、类型和连接错误、保存前后、两列置空及其他关系保留。Rust 1.94 全工作区 620 项通过、8 项夹具忽略。格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 所属关系赋值与关系缓存

按固定 Laravel 的 `BelongsTo::associate` 与 `dissociate` 核对模型、标量键和空关系三种路径。Rust 通过 `named` 指定关系名。借用的模型以快照写入关系缓存，Laravel 接收的标量重载对应 Rust 的 `associateKey`，解除关联可用 `dissociate` 或 `disassociate`。赋值只修改内存，保存由调用者执行。复用 `v0.x` 按名更新、标量赋值和独立保存契约。固定 Goravel 用作连接级 ORM 参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsTo.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#belongs-to-relationships) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证模型快照、标量键赋值、空缓存、缺失所有者键、其他关系保留以及内存和数据库边界。MySQL 模型赋值后零 SELECT，标量赋值后首次访问执行一次 SELECT。Rust 1.94 工作区 620 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 单值关系身份比较

对照固定 Laravel 的 `ComparesRelatedModels` 和 `HasOneThrough::is`。普通所属、一对一与多态一对一关系比较关联键、表和连接，穿透关系和多选一以 SQL 存在查询确认候选记录。Rust 显式传入声明端模型和可空候选。多态所属先解析已登记别名及类型，同名但实际隔离的数据库连接视为不同来源。整数与文本混比时要求文本是完整整数，保留 Rust 的有符号及无符号范围。旧版 `v0.x` 未找到直接对应实现，复用当前关联键、作用域和多选一查询逻辑。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/ComparesRelatedModels.php) · [Laravel 穿透关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneThrough.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#comparing-models) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证自定义所有者键、空候选、不同连接、未知多态别名、普通比较零 SELECT、多选一落选、穿透关系和软删除作用域。Rust 1.94 工作区 620 项通过、8 项夹具忽略。格式、全目标严格 Clippy、实际 Artisan、三语示例编译、文档构建和 18 项浏览器检查通过。

## Pivot 写入事务入口

对照固定 Laravel 的 `InteractsWithPivotTable`。补齐 `attachOrFail`、`detachOrFail`、`updateExistingPivotOrFail`、`syncOrFail`、`syncWithoutDetachingOrFail`、`syncWithPivotValuesOrFail`，与已有 `toggleOrFail` 一起提供完整入口。Rust 复用原写入和触碰事务，普通同步已有事务，且多态关系沿用同一套接口。调用显式传入声明端及类型化键，`syncOrFail` 接收布尔值决定是否移除未列出的关系。旧版 `v0.x` 的 Pivot 同步要求调用者提供写事务，沿用其事务内读取、变更和触碰的边界。固定 Goravel 的 `Orm.Transaction` 用作编译型事务装配参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/InteractsWithPivotTable.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#inserting-and-updating-related-models) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证普通 Pivot 写入、多态删除隔离、同步保留与解除分支，以及自定义 Pivot 创建、更新、删除和先解除再新增时的事务回滚。回滚同时保留原关联并撤销事件写入。初次全量运行暴露验收夹具缺少 `withPivot(["label"])`，修正后双库专项和最终工作区 620 项通过、8 项夹具忽略。格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建与 18 项浏览器检查通过。

## 多对多创建或回查

对照 Laravel 13.40.0 的 `BelongsToMany::createOrFirst`、上游测试与 #62223。创建时合并匹配属性和附加属性，后者优先。唯一键冲突时从写连接查找未限定于当前关系的候选，再补关联。再次碰到唯一键冲突时，从写连接在当前关系中查找并返回已经挂载的候选。无法匹配时返回原冲突，其他数据库错误直接传播。Rust 用类型化属性和 `Arc<R>`，将首次模型写入和 Pivot 写入置于同一事务。事件仍执行，关联模型的保存不触碰所属端，关系触碰在挂载后执行。旧版没有对应的创建或回查接口，沿用其写事务内处理 Pivot 的边界。固定 Goravel 以 `Orm.Transaction` 作为事务装配参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToManyCreateOrFirstTest.php) · [Laravel 写连接修复](https://github.com/laravel/framework/pull/62223)

SQLite 分离读写文件且关闭 sticky，读库无相关表，验证两条唯一键冲突回退均使用写库。独立 MySQL 8.4.13 验证并发创建复用同一模型、保留已有 Pivot 值、无匹配冲突传播和 Pivot 写入失败后的模型回滚。Rust 1.94 工作区 621 项通过、8 项夹具忽略。格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建及 18 项浏览器检查通过。

## 多对多先查、创建和更新

对照 Laravel 13.40.0 的 `BelongsToMany::firstOrCreate`、`updateOrCreate` 及上游测试。先查当前关系，再按原模型作用域查未挂载候选，均未命中时复用 `createOrFirst`。补挂碰到唯一冲突时，用写连接确认关联。更新已有模型时应用值和模型事件，并沿用 Laravel 禁用模型所属端触碰的行为。Pivot 属性只用于新增关联。Rust 使用显式的匹配、模型值和 Pivot 属性映射，模型返回 `Arc<R>`，此批提供属性映射形式。旧版未找到这两个入口，复用其模型写入与 Pivot 写入分工，固定 Goravel 作连接事务参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToManyCreateOrFirstTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证既有关联、补挂未关联模型、创建、更新持久化、更新唯一键冲突及既有 Pivot 值保留。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多对多软删除恢复与延迟属性

按 Laravel 13.40.0 的 `restoreOrCreate` 和 `createOrRestore` 接入两种查找顺序，两者在候选查询中包含软删除模型，并复用现有模型恢复生命周期。插入或挂载发生唯一冲突时沿用写连接回查。使用类型化 `RelatedValues::deferred` 传入 Rust 闭包，原有属性映射调用仍可使用。查找命中或仅挂载既有模型时，`firstOrCreate` 无需计算值。插入优先方法在尝试插入时计算，`updateOrCreate` 在已有模型需要更新时也计算。旧版未找到对应多对多入口，沿用既有模型和 Pivot 分工。固定 Goravel 用作连接事务装配参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 延迟值测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToManyCreateOrFirstTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证已挂载软删除模型恢复、已有 Pivot 属性保留、插入冲突后补挂和恢复、未命中创建、更新以及延迟值调用次数。未启用软删除的模型会在写入前返回错误。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建与 18 项浏览器检查通过。

## 多对多计数器创建与递增

按固定 Laravel 的 `BelongsToMany::incrementOrCreate` 和模型递增生命周期实现。新模型使用初始值，已有模型使用数据库算术更新，并在同一语句中写入额外属性。已有关系沿用原 Pivot。Rust 显式传入列、初始值及步长，使用有限 SQL 数值。递增在写连接事务中读取预期值并派发 `Updating`，更新后回读已持久化值及配置的生成列，然后派发 `Updated`。MySQL 使用行锁，SQLite 使用现有事务并发重试。此入口使用 SQL 数值列，PHP 自定义类的递增转换协议单独跟踪。旧版未找到对应多对多入口，复用当前 Query Builder 原子递增和模型事件。固定 Goravel 作连接事务参考。 [Laravel 关系源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证初始值、额外属性、Pivot 保留、更新事件的计数值及观察者失败后的行与事件回滚。MySQL 两次并发递增分别返回 306 和 308，最终值为 308。首轮 SQLite 夹具在缓存查询后改表触发 SQLx 行列索引错误，调整为建表时声明列后重新验收。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、文档构建及 18 项浏览器检查通过。

## 直接、多态与穿透关系计数器

按固定 Laravel `HasOneOrMany` 与 `HasOneOrManyThrough` 补齐六种关系的 `incrementOrCreate`。直接和多态关系复用外键写入，并从写库按关系条件回查唯一冲突。穿透关系按连接条件查找，创建时由调用方给出中间模型外键，唯一冲突按固定上游用合并后的插入属性回查。各关系共用数据库原子递增和模型更新事件。旧版未找到对应入口，固定 Goravel 用作连接事务参考。 [Laravel 直接关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Laravel 穿透关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证六个关系入口创建与递增、直接外键覆盖、多态鉴别值隔离、穿透查找及显式中间键。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建及 18 项浏览器检查通过。

## 自定义计数器转换

按固定 Laravel 的模型递增与类转换机制，将逻辑新值和步长交给转换器，持久化仍使用数据库原子加法。Rust 通过可选的 `CastsInboundAttributes::increment` 显式启用，类型化类转换、直接自定义转换和仅写转换共用此接口。转换器读取事务中的当前存储值，`Updating` 看到转换后的模型属性，`Updated` 看到数据库回读值。额外存储列可随更新一起写入。此批范围是关系计数器递增。旧版未发现对应递增转换协议，固定 Goravel 用作事务装配参考。 [Laravel 模型](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 自定义转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#custom-casts) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证逻辑值 3 和存储值 300 的创建、递增后 5 和 500、转换拒绝保持原值，以及 MySQL 并发调用分别返回 7 和 9。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建及 18 项浏览器检查通过。

## 模型实例计数器

对照固定 Laravel 的 `incrementOrDecrement`、`incrementOrDecrementEach` 及模型测试。已持久化模型按原始身份在同一 SQL 更新中写入所有计数列，触发一次 `Updating` 和一次 `Updated`，并回读已写字段。新实例把操作转交作用域查询。Rust 用 `Attributes` 显式传递步长并返回影响行数，取消时返回 0。关系递增复用同一事务路径，类转换可以分别定义 `increment` 和 `decrement`。旧版未找到对应模型入口，固定 Goravel 用作事务装配参考。 [Laravel 模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证两列同时递增和递减、附加列写入、原始属性同步、非法步长整笔拒绝及自定义缩放递减。关系测试继续覆盖观察者报错回滚与 MySQL 并发递增。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 静默计数与 Eloquent 批量计数

对照固定 Laravel 的模型静默计数和 Eloquent Builder 算术更新。模型的四个 `Quietly` 入口复用现有事件抑制作用域和实例事务。Builder 的四个入口复用带作用域的查询及原子 SQL，只写入查询匹配行。Rust 接收显式步长并返回受影响行数，更新时间沿用现有 Builder 更新规则。旧版没有对应模型入口，固定 Goravel 用作 ORM 查询职责参考。 [Laravel 模型](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证四种静默计数写入、观察者抑制、模型状态同步，以及 Builder 的匹配行数、软删除排除、默认更新时间和显式时间戳。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建及 18 项浏览器检查通过。

## 直接关系批量保存与静默创建

对照固定 Laravel `HasOneOrMany` 的 `saveMany`、`createMany` 和静默变体。直接与多态的一对一、一对多共用既有保存与创建流程。Rust 的 `saveMany` 原位更新可变切片，`createMany` 返回按输入顺序排列的模型。批量操作逐行写入并传播首个数据库错误，静默入口复用既有事件抑制作用域。旧版未找到对应批量入口，固定 Goravel 用作 ORM 写入职责参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#the-save-method) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证批量顺序、父键、多态鉴别值、静默观察者抑制和中途失败后保留此前写入。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、文档构建及 18 项浏览器检查通过。

## 直接关系强制创建

按固定 Laravel `HasOneOrMany` 与 `MorphOneOrMany` 接入 `forceCreate`、`forceCreateMany` 和两种静默变体。复用旧版强制赋值仍执行属性转换的语义，使用当前异步属性赋值与关系保存路径。Rust 显式传入父模型，批量入口返回有序模型。关系保存以父键和多态类型为准。固定 Goravel 用作 ORM 职责参考。 [Laravel 直接关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Laravel 多态关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphOneOrMany.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证保护列写入、类转换、关系键优先、批量顺序和静默事件抑制。首次编译发现测试夹具的类转换切片需要常量绑定，修正后工作区 622 项通过、8 项夹具忽略。格式、严格 Clippy、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 多对多保存与创建

对照固定 Laravel `BelongsToMany` 的 `save`、`create`、批量、静默和强制创建变体。Rust 单条使用事务把模型与 Pivot 一起写入，失败时回滚当条且保留传入模型的原状态。批量按位置读取 Pivot 属性并逐条提交，全部成功后统一触碰。复用旧版模型写入、Pivot 属性保护和触碰的职责分工。固定 Goravel 用作事务装配参考。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) · [Laravel 关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#the-save-method) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证创建和挂载、重复 Pivot 回滚模型变更、批量位置属性、失败行回滚与前序提交、静默重试及保护字段写入。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、文档构建和 18 项浏览器检查通过。

## 直接、多态与穿透关系查找或创建

对照固定 Laravel 的 `HasOneOrMany` 与 `HasOneOrManyThrough`，补齐 `firstOrNew`、`firstOrCreate`、`createOrFirst`、`updateOrCreate`、`restoreOrCreate`、`createOrRestore`。直接与多态关系沿用写连接回查和延迟值。穿透关系接受属性映射，创建时由调用方提供中间模型外键，冲突回查沿用固定版本的查询路由。恢复使用模型生命周期，中间模型的软删除过滤独立生效。已有关系计数复用本次查找与创建流程。旧版未找到对应入口，固定 Goravel 用作 ORM 连接职责参考。 [Laravel 直接关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Laravel 穿透关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentHasManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证未保存结果、命中不求值、创建、模型更新、关系范围内唯一冲突回查、多态隔离和软删除恢复。Rust 1.94 工作区 622 项通过、8 项夹具忽略，格式、严格 Clippy、实际 Artisan、三语示例编译、文档构建及 18 项浏览器检查通过。

## 直接与多态关系批量写入

对照固定 Laravel 的 `HasOneOrMany::upsert`、`MorphOneOrMany::upsert` 和 Eloquent Builder。直接与多态的一对一、一对多在每行覆盖关系键，再复用 Query Builder 的方言 SQL。模型配置的 ID 和时间戳只补缺失列，冲突更新包含更新时间，未列入更新列的创建时间会保留。Rust 使用 `Vec<Attributes>` 表示批次，非空批次要求已有父键，保持现有关系写入契约。旧版有 Query Builder 批量写入，当前复用其在主分支上的对应入口。固定 Goravel 继续作为 ORM 连接职责参考。 [Laravel 关系源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphOneOrMany.php) · [Laravel Builder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Laravel Upserts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#upserts) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证空批次、批量插入、覆盖关系键、冲突更新、不同父键与多态类型隔离、自动 ULID 及更新时间。冲突范围由数据库唯一索引决定。穿透和 Pivot 的批量转发契约继续核对。

Rust 1.94 全工作区回归 624 项通过、8 项夹具忽略。格式、严格 Clippy、实际 Artisan、三语示例编译、文档构建及 18 项浏览器检查通过。

## 穿透与多对多关系批量写入

固定 Laravel 的穿透、普通与多态多对多关系经 `Relation::__call` 将 `upsert` 转发到 Eloquent Builder。Rust 在类型化关系上公开同名入口，复用关联连接、模型配置和现有批量写入。穿透关系使用每行显式提供的中间外键，多对多关系的 Pivot 由独立的关联写入接口管理。`MorphToMany` 沿用已有的 `BelongsToMany` 委托。旧版只找到 Query Builder 的 upsert 与回调测试，继续复用其对应的方言写入路径。固定 Goravel 用作连接职责参考。 [Laravel 转发源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) · [Laravel Builder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Laravel Upserts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#upserts) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 实测两种穿透关系以及普通和多态多对多关系的插入与冲突更新，确认中间外键按输入保存、缺少必要外键时写入失败、模型生成 ULID，且 Pivot 表保持空表。

Rust 1.94.0 格式与全目标严格 Clippy 通过。工作区锁定测试 624 项通过、8 项夹具忽略。实际 Artisan、三语示例编译、VitePress 构建及 18 项浏览器检查通过。

## 反向关系批量写入

固定 Laravel 的 `BelongsTo` 和 `MorphTo` 通过关系的转发机制调用 Eloquent Builder。声明模型的多态类型决定所属模型的表。OpenRust 复用关系连接与已注册目标，使用调用方提供的批次属性，写入后不改声明模型的关联。Laravel 在类型为 null 或空串时为预加载建立声明模型上的查询。OpenRust 的非空写入要求明确目标类型，缺失、未知或非法类型返回错误，空批次直接返回 0。旧版只有 Query Builder 批量写入，本批沿用其对应的方言入口。固定 Goravel 用作 ORM 连接职责参考。 [Laravel 关系构造](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php) · [Laravel 转发源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/MorphTo.php) · [Laravel Builder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBuilderTest.php) · [Laravel Upserts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#upserts) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 专项验证普通所属关系写入、多态选择两个目标表、冲突更新、目标模型自动 ULID、缺失和非法别名、空批次及声明模型关联不变。

Rust 1.94.0 格式、全目标严格 Clippy、锁定工作区测试 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 带作用域的关系与默认属性

按固定 Laravel 的 Scoped Relationships 接入 `Builder::withAttributes`，通过 `constrain` 在关系定义中使用。Rust 用 `Attributes` 和显式布尔参数表达 Laravel 的数组及 `asConditions`。条件可独立关闭，已赋值的模型属性优先。直接和多态关系先设置关系键，再为模型补缺失属性，继续使用属性转换。穿透关系沿用 Builder 创建默认值，多对多关系保留已有挂载事务。旧版未找到 pending attributes 机制，沿用主分支已有的属性转换和关系连接。固定 Goravel 用作连接级 ORM 参考。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#scoped-relationships) · [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) · [Laravel 关系源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 专项验收查询过滤、显式属性优先、仅默认值模式、直接创建和保存、未保存模型、多态强制创建、穿透创建，以及多对多创建和挂载。

Rust 1.94.0 格式、严格 Clippy 和锁定工作区回归通过，624 项通过、8 项夹具忽略。实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 递归保存模型与关系

对照固定 Laravel 的 `push`、`pushQuietly` 及单值、多值、空关系与循环测试。Rust 按加载或设置顺序遍历当前关系缓存，保留模型保存事件和停止语义。类型化 `setRelation` 支持将编辑后的快照放回关系缓存；每次遍历重用共享模型的已保存快照，并对活动路径上的同一快照停止递归。普通、多态、自定义 Pivot 和动态关系均接入缓存遍历。失败前已经写入的模型保留，先前借出的 `Arc` 继续引用原快照。旧版未找到递归保存入口，复用当前模型保存、事件抑制与连接选择。固定 Goravel 用作连接级 ORM 参考。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#recursively-saving-models-and-relationships) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L1224) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证多层及多态已加载关系、共享子模型快照、保存取消与数据库冲突后的前序写入、后续节点跳过、静默重试和缓存写回。框架的普通 Pivot 是只读属性投影，通过关系的 Pivot 写入接口管理。

Rust 1.94.0 格式、全目标严格 Clippy、锁定工作区回归 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 在子模型上自动加载父模型

对照固定 Laravel 的 `SupportsInverseRelations` 和四类反向关系测试。`HasMany`、`HasOne`、`MorphMany`、`MorphOne` 支持 `chaperone` 与 `inverse`，也可显式关闭。Rust 使用明确的反向名称和类型化缓存，保留多态别名。一次匹配中同一父模型下的子记录共享父快照。快照省略父模型的已加载关系，避免强引用环，和 PHP 共享原对象的方式有差异。普通创建、保存和缺省模型同样填入反向缓存。旧版没有对应入口，沿用当前关系批量匹配和模型快照，参考 Goravel 的连接级 ORM。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#automatically-hydrating-parent-models-on-children) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Concerns/SupportsInverseRelations.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentInverseRelationHasManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 专项验证批量预加载、直接读取、关闭反向缓存、创建、未保存模型和缺省模型。MySQL 普通预加载及反向读取共两次 SELECT，多态单值读取及反向读取共一次 SELECT。首次测试夹具的集合借用和临时描述符生命周期问题已修正并复验。

Rust 1.94.0 格式、严格 Clippy、锁定工作区回归 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 复用两段关系定义穿透关系

按固定 Laravel 的 `PendingHasThroughRelationship` 复用两段关系的四个键。Rust 使用 `Model::through(local).has(distant)` 和类型化描述符，编译时核对中间模型并决定结果基数。任一段为多值时产生 `HasManyThrough`。与固定源码一致，组合带入本地多态关系的类型条件，附加查询条件由返回的穿透关系配置。中间类型过滤复用现有连接、软删除和择一查询。旧版未找到链式构造，沿用当前穿透关系连接机制。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#has-many-through) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/PendingHasThroughRelationship.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentThroughTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证链式单值、多值、自定义列、软删除中间模型、相同外键值的多态隔离及存在查询。

Rust 1.94.0 格式、严格 Clippy、锁定工作区回归 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 预加载特定列

按固定 Laravel 的关系路径冒号列选择实现 `withPaths`、`loadPaths`、`loadMissingPaths`。同次调用可为中间关系和末级关系分别指定列。调用方保留匹配所需的主外键，框架为多对多和穿透关系继续附加内部匹配列。Rust 复用类型化关系约束和现有查询列验证，多态目标共用列清单。旧版未找到命名路径列选择入口，沿用当前模型选择和关系连接。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#eager-loading-specific-columns) · [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) · [Laravel 关系测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentBelongsToManyTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 专项覆盖嵌套列投影、主外键匹配、已加载值保留、多态目标、Pivot 和穿透内部匹配、无效列清单。MySQL 嵌套预加载连同根查询共 3 次 SELECT。

Rust 1.94.0 格式、严格 Clippy、锁定工作区回归 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 自定义多态类型映射

对照固定 Laravel 的 morphMap、enforceMorphMap、getMorphedModel、getMorphAlias 与 getMorphClass。Rust 映射位于应用容器，以编译类型登记，按模型连接查找。合并时新条目优先，原有别名继续用于读取。显式描述符继续可用，映射版描述符在构造时绑定当时的别名。Rust 通过 targetMapped 显式登记每个可水合的模型类型，并保留类型化的所有者键。旧版保留原始模型键与判别值，未找到应用级映射，现有 Rust 多态关系使用字符串别名。参考固定 Goravel 的应用级 ORM 连接职责。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#custom-polymorphic-types) · [Laravel Relation](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 与独立 MySQL 8.4.13 验证不同应用使用不同别名、缺少强制映射时拒绝解析、空别名注册失败、映射查询、合并优先级、关系写入与反查、旧别名兼容以及新关联选择首选别名。

Rust 1.94.0 格式、严格 Clippy、锁定工作区回归 624 项通过及 8 项夹具忽略、实际 Artisan、三语示例编译、站点构建和 18 项浏览器检查通过。

## 单模型延后加载关系聚合

按固定 Laravel 的 Deferred Count Loading 与 Other Aggregate Functions 接入单模型的 `loadCount`、`loadExists`、`loadSum`、`loadAvg`、`loadMin`、`loadMax` 与多态 `loadMorphCount`。Rust 用类型化关系描述符传递聚合参数，单模型和集合复用同一查询与回填路径。多个普通聚合可通过 `loadAggregates` 一次查询。旧版未找到延后关系聚合入口。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md#deferred-count-loading) · [Laravel Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php)

独立 MySQL 8.4.13 验证所有普通聚合入口、多个聚合共用一次 SELECT、脏值与原始属性同步、已删除行错误，以及多态计数写回。Rust 1.94.0 格式、严格 Clippy、MySQL 关系定向测试、实际 Artisan、三语示例编译、站点构建与 18 项浏览器检查通过。SQLite 与其他数据库按发布验收规则留待发布前复验。

## 模型集合工厂与未保存关联模型

对照固定 Laravel 的 `HasCollection::newCollection` 与 `HasOneOrMany::make`、`makeMany`。模型查询在水合与预加载后调用集合工厂。普通和多态的一对一、一对多关系复用已有未保存模型构造逻辑，批量构造也调用关联模型的工厂。Rust 保留 `Collection<M>` 返回类型，可通过工厂设置集合键，通过扩展 trait 增加业务方法。PHP 的 `CollectedBy` 子类替换及多值关系缓存的返回类型仍有差异。旧版未找到对应工厂和批量未保存构造。 [Laravel 自定义集合](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md#custom-collections) · [Laravel 集合工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/HasCollection.php) · [Laravel 关系测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentInverseRelationHasManyTest.php)

独立源码副本在 MySQL 8.4.13 通过关系定向验收，覆盖查询工厂、集合键、未保存批次顺序、作用域默认值、普通反向缓存与多态键，以及数据库未写入草稿。隔离副本 Rust 1.94.0 格式、严格 Clippy 和实际 Artisan 通过，三语示例编译、站点构建及 18 项浏览器检查通过。共享工作区的 Artisan 编译碰到同时修改的会话代码，隔离复验通过。SQLite 与其他数据库留到发布前。

## 批量模型工厂与队列集合恢复

对照 Laravel 13.40.0 的批量工厂和队列标识恢复。模型工厂的 `count(...).make()` 与 `count(...).create()` 使用 `Model::newCollection` 构造结果，保留既有状态、持久化和回调顺序。队列从写连接读取模型，先按载荷重组普通模型、保留重复 ID 并省略缺失行，再构造集合并补载关联。自定义 Pivot 保留既有去重规则。Rust 由已注册模型选择构造方式，队列 JSON 契约保持原样。旧版没有同契约的集合工厂或队列模型恢复实现。 [Laravel 工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php) · [Laravel 队列恢复](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/SerializesAndRestoresModelIdentifiers.php)

独立 MySQL 8.4.13 定向验收覆盖批量草稿与持久化结果的自定义键，以及真实 `queue:work` 恢复的顺序、重复 ID、缺失模型、关联和自定义 Pivot 集合键。Rust 1.94.0 格式、全目标严格 Clippy、实际 Artisan、三语示例编译、站点构建及 18 项浏览器检查通过。默认 Rust 1.99 的严格 Clippy 仍报告分页和 Redis 限流两处基线以外的告警。其他数据库留待发布前验收。

## 多值关系的自定义集合键

对照固定 Laravel 的 HasMany、HasOneOrMany 及关联测试。直接读取、延迟加载和预加载为每个父模型构造多值结果。HasMany、MorphMany、BelongsToMany、MorphToMany 与 HasManyThrough 调用关联模型的 `newCollection`。Rust 用 `RelatedCollection<M>` 保留键和 `Arc<M>` 快照。显式赋值可用 `fromShared`，递归保存和嵌套加载替换快照时保留键。列表键序列化为数组，自定义非列表键序列化为对象。工厂可为不同父模型生成独立快照，PHP 对象引用身份仍有差异。旧版多值缓存使用 `Vec<Arc<M>>`，沿用共享快照契约。Goravel 仅用于核对编译型装配，本批无启动变更。 [Laravel 多值匹配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentHasManyTest.php)

隔离 MySQL 8.4.13 验证直接读取、延迟加载、预加载、自定义键、JSON、空结果、反向缓存和查询计数。模型事件、递归保存、模型工厂、实际 TCP Resource 和真实队列 Worker 的 MySQL 定向验收通过。Rust 1.94.0 格式、全目标严格 Clippy、实际 Artisan、三语示例编译、VitePress 构建及 18 项浏览器检查通过。

## Eloquent 分页的集合工厂和键

以 `68006a8e` 为隔离基线。固定 Laravel 的 [Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) 将模型集合交给分页器，简易分页切片保留自定义键，[游标分页](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/CursorPaginator.php) 重新编号。Rust 在水合、事件与关系加载后调用 `Model::newCollection`。普通和简易分页保存集合键及自动加载绑定，支持 `getByKey`，转换后 JSON 仍保留键。游标页按列表呈现。Rust 先在查询层确定边界，仅将实际返回行交给模型工厂；简易和游标的预读行无需模型化。固定 Goravel ORM 仅作为查询分页参考，v0.x 未找到对应模型集合工厂。

真实 MySQL 8.4.13 和 SQLite HTTP 验证普通及简易分页对象键、查键、翻页、`through` 转换、游标重新编号与往返。多对多的工厂以已加载 Pivot 计算自定义键。6 项分页、6 项关系回归、Rust 1.94.0 全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress、三语集合和分页正文的 12 个访问组合及 12 项通用浏览器检查通过。既有 Sanctum 格式仅在隔离副本调整。测试容器已回收，清理独占 Cargo 产物 1.8 GiB 和站点产物。提交前周额度剩余 26%，项目卷可用 52 GiB。

## 按主键查找或构造关联模型

复核固定 Laravel 的 [HasOneOrMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrMany.php)、[HasOneOrManyThrough](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php) 和 [BelongsToMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) 的 `findOrNew`。复用现有关系查询、直接子模型 `make` 和 Pivot 水合。直接子关系继续沿用 Rust 无有效父键时的创建错误，正常父键下补齐外键、多态类型与创建默认值。穿透和多对多未命中时返回绑定连接的空模型，不赋联接键。固定 Goravel 的 ORM 用于核对编译型装配，v0.x 未找到同名关系入口。

隔离基线 `c7df2ade`。MySQL 8.4.13 及 SQLite 实际 HTTP 验证匹配主键、作用域过滤、其他父模型、软删除、草稿默认值、穿透与 Pivot。回读四张表的行数确认草稿没有写入。6 项关系和 6 项分页回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异只在隔离副本调整。已回收容器，清理本批隔离 Cargo 产物 1.8 GiB 和站点产物。提交前周额度剩余 26%，项目卷可用 52 GiB。

05-02 仍待核对关系查询的唯一结果与回调入口（如 `findSole`、`findOr`），以及与 02-09 的流式关系查询边界。05-06 保持前置依赖待关闭。

## 关系唯一结果与回调查询

对照固定 Laravel 的 [BelongsToMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L760) 和 [HasOneOrManyThrough](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L405)。直接、多态、穿透和多对多关系提供标量主键 `find`、`findOrFail`、`findSole`、`findOr` 及关系查询 `sole`、`firstOr`。复用既有约束、软删除、预加载及 Pivot 水合。唯一查询最多读取两条记录，分别以 `ModelNotFound` 和 `MultipleModelsFound` 表示两种错误。Rust 回调是返回同一模型类型的 Future，查询成功且为空时才调用。Goravel 固定 ORM 作为查询入口参考，v0.x 未找到对应唯一查询和关系回调入口。

隔离基线 `4de6816d`。MySQL 8.4.13 和 SQLite 实际 HTTP 验证唯一命中、关系范围、404、重复 Pivot、修复重复后重试、回调返回值及错误传播。重复 Pivot 的 409 由验收路由映射。SQLite 首轮的未限定列名被解析成文字，改用带表名的无效列后双库验证查询错误不会调用回调。12 项关系与分页回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式仅在隔离副本调整。临时 MySQL 已回收，清理隔离 Cargo 产物 1.8 GiB 及独占文档产物。提交前周额度剩余 26%，项目卷可用 52 GiB。

继续核对关系批量按键查询和流式读取，区分 05-02 的关系约束和 02-09 的惰性集合接线。05-06 的前置依赖保持待关闭。

## 关系批量主键查询

对照固定 Laravel 的 [BelongsToMany::findMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L796) 和 [HasOneOrManyThrough::findMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php)。直接、多态、穿透和多对多共用关系主键过滤，Rust 用 `&[Value]` 表达批量输入。结果按关系查询排序，经 `Model::newCollection` 构造，保留自定义键和 Pivot。空输入无需发起查询。关系父键、作用域和 Pivot 谓词继续限制结果。固定 Goravel ORM 只作为查询入口参考，v0.x 未找到同名批量方法。

以 `c00b5572` 为隔离基线。MySQL 8.4.13 和 SQLite 实际 HTTP 验证重复输入、缺失行、其他父模型、软删除、多态和自定义集合键。故意配置无效查询，非空失败，空输入仍返回空集合。未保存父模型的直接批量查询返回空集合。12 项回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本调整。容器已回收，清理隔离编译产物 1.8 GiB 及站点产物。提交前周额度剩余 26%，项目卷可用 52 GiB。

后续核对关系流式查询与 02-09 的接线，再做 05-02 整体对账。批量入口目前直接返回匹配集合，Laravel 的数组 `findOrFail` 和 `findOr` 重载仍待核对。

## 严格批量关系查询与回调

对照固定 Laravel 的 [BelongsToMany 数组查询](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L827) 和 [HasOneOrManyThrough 数组回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L465)。Rust 通过 `findManyOrFail`、`findManyOr` 表达数组重载，后者接受返回同类型集合的异步回调。复用 Eloquent 集合的主键核对。固定 Laravel 比较返回行数和去重后的请求键数，OpenRust 判断每个请求键是否真有匹配。重复 Pivot 无法掩盖另一个缺键，确认全匹配后仍返回原集合。查询及前置加载错误直接返回。固定 Goravel ORM 用作查询参考，v0.x 未找到对应方法。

隔离基线 `072e29d7`。MySQL 8.4.13 和 SQLite 实际 HTTP 验证重复输入、混合主键表示、重复 Pivot 保留、跨父模型缺失返回 404、回调、空输入和 SQL 错误传播。12 项关系与分页回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异只在隔离副本调整。容器已回收，清理独占编译产物 1.8 GiB 及站点产物。提交前周额度剩余 26%，项目卷可用 52 GiB。

下一阶段先处理 02-09 已记录的异步数据库游标接线。当前 Connection 查询返回完整 Vec，关系层需复用可短路的数据库游标后才能正式提供流式读取。

## 关系逐行游标

以 `67d28aab` 为隔离基线。对照固定 Laravel 的 [Relation 查询转发](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php)、[穿透关系游标](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L674) 和 [多对多游标](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L1246)。Rust 在可静态确定关联模型的描述符上提供 `cursor(parent)`，共用模型行流、父键约束、穿透联接和 Pivot 水合。多对多在读取事件及集合工厂前装配 Pivot，与既有分页水合顺序一致；固定 Laravel 游标在 Eloquent 游标返回模型后追加 Pivot。v0.x 的 BatchCursor 是键集分批标记，固定 Goravel ORM 用于核对编译型查询装配。

MySQL 8.4.13 及 SQLite 实际 HTTP 验证直接关系提前结束后继续查询、多态过滤、所属关系、穿透中间模型软删除、单条选择、自定义 Pivot 和空父键。12 项关系／分页回归、Rust 1.94.0 格式与严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 及 18 项浏览器检查通过。既有 Sanctum 排版仅在隔离副本整理。已回收测试容器，清理 1.9 GiB 编译产物及文档构建产物。周额度剩余 99%，项目卷约 119 GiB。

关系分批读取、动态 MorphTo 目标的游标边界及 05-02／05-06 整体固定矩阵仍需核对。

## 反向多态关系游标

以 `033673a3` 为隔离基线。固定 Laravel 从父模型的多态类型解析目标，再把 `cursor` 转发给关联模型的 Eloquent Builder。Rust 沿用编译注册的目标、已有父键验证及模型游标，返回可按别名识别或按类型取得的 `MorphTarget`。继续应用注册的目标条件和软删除选项。缺少类型或外键时为空流，有键但别名未注册时返回查询错误。游标遍历实际记录，默认模型由关系加载提供。固定 Goravel ORM 的 `Cursor` 返回行通道；v0.x 只有键集分批标记。 [Laravel 类型解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php) · [Laravel MorphTo 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentMorphToTest.php) · [Goravel ORM 契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go)

独立 MySQL 8.4.13 实际 HTTP 核对两类目标、提前结束后继续查询、类型化结果、目标过滤、软删除、空引用与未知别名。`pagination_on_mysql_and_http`、`relationship_keys_and_query_counts_on_mysql` 和 `related_upsert_on_mysql` 均通过。Rust 1.94.0 全目标严格 Clippy、实际 `./artisan inspire`、六个三语代码片段编译、VitePress 与 18 项浏览器检查通过。全工作区格式检查在隔离副本格式化既有 Sanctum 测试后通过，该格式差异排除在提交范围外。测试容器已回收，清理本批 1.9 GiB Cargo 产物及独立文档和浏览器缓存。当前剩余关系分批读取及 05-02／05-06 固定矩阵对账。

## 偏移量分批读取

以 `70ba0667` 为隔离基线。固定 Laravel 的 `BuildsQueries::chunk` 逐页执行查询，尊重初始偏移量及总行数限制，回调从第 1 页开始并可返回 false 停止。Query Builder 要求显式排序，Eloquent 无显式排序时按主键排序。Rust 的异步回调接收当前批次和页码，通过 `Result<bool>` 表达错误与是否继续。同一轮在应用查询作用域后复用查询定义，每批独立执行 SQL 和预加载。直接子关系、穿透关系和多对多复用其关系查询，以既有分页的顺序装配 Pivot，随后运行读取事件、预加载和集合工厂。Laravel 多对多在其 Builder 返回当前集合后加上 Pivot。v0.x 的 SeaORM BatchReader 提供了异步批回调参考，其键集翻页契约与本批偏移量不同。 [固定 Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php) · [固定 Laravel BelongsToMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L1160)

MySQL 8.4.13 实际 HTTP 验证父键、多态判别、穿透软删除、排序与 offset／limit、每批预加载、Pivot 及自定义集合键、短批、回调提前结束与错误传播。另外验证未排序的原生查询、零批量和零查询上限。分页 HTTP、关系查询计数和 Query Builder 三项定向回归通过。Rust 1.94.0 全目标严格 Clippy、实际 `./artisan inspire`、三语示例、VitePress 及 18 项浏览器检查通过。既有 Sanctum 测试格式在隔离副本修正以运行全工作区检查。测试容器已回收，清理隔离 Cargo 产物和站点构建缓存。关系反向入口、按键分批及惰性分批仍待分别验收。

## 反向关系分批读取

以 `f065df95` 为隔离基线。固定 Laravel 的 `BelongsTo` 按子模型外键约束目标查询，`MorphTo` 先解析类型并创建该类型的查询，关系通过 Builder 执行 `chunk`。默认模型属于关系加载结果。Rust 将外键约束及多态目标解析与游标共享，每批继续执行模型读取事件、预加载及集合工厂。普通反向关系回调收到类型化的 Eloquent 集合，多态反向关系收到保留具体模型集合键的 `Collection<MorphTarget>`。回调仍用 `Result<bool>` 表达继续、停止和错误。v0.x 的 BatchReader 提供异步批回调契约参考，固定 Goravel ORM 的 Association 与 Cursor 用于对照。 [固定 Laravel BelongsTo](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsTo.php#L94) · [固定 Laravel 多态类型解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php#L317) · [固定 Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go)

独立 MySQL 8.4.13 实际 HTTP 核对反向外键、非主键目标的跨批顺序、每批预加载、集合键、软删除选择、目标约束、未知别名、缺失关联、默认模型边界和回调取消及报错。分页 HTTP、关系查询计数和关系批量写入三项定向回归通过。Rust 1.94.0 严格 Clippy、实际 `./artisan inspire`、六个三语示例片段编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整。已回收容器，清理本批 1.9 GiB Cargo 产物与独立文档缓存。后续验收按键与惰性分批。

## 按键分批读取

以 `8d61103b` 为隔离基线。对照固定 Laravel 的 `orderedChunkById` 与多对多特化。首批应用初始偏移量，后续按 `>` 或 `<` 寻找下批，保留总行数限制。回调先于最后一行键的校验，停止和错误无需继续读取。查询层默认 `id`，Eloquent 默认模型主键，多对多默认带表名的关联键并水合 Pivot。Rust 从 SQL 投影取原始边界值，避免模型转换改变数据库比较值；把已有查询条件分组后叠加边界，保留 OR 查询和关系约束。联合查询由 `fromSub` 表达单一来源，确保键值条件作用于全部分支。v0.x BatchReader 的升降序和键集规则可参考，当前 API 采用 Laravel 的列及别名契约。 [Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php#L153) · [Laravel 多对多按键分批](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php#L1227)

MySQL 8.4.13 实际 HTTP 验证默认键、升降序、替换既有键排序、首批偏移和总上限、投影别名、OR 条件、多对多 Pivot 与自定义集合键。缺键只在回调要求继续时返回错误，回调错误保持原样。分页 HTTP、关系查询计数与 Query Builder 三项定向回归通过。Rust 1.94.0 全目标严格 Clippy、实际 `./artisan inspire`、三语示例、VitePress 与 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整，回收测试容器与独占构建缓存。待继续验收其余关系入口和惰性分批。

## 关系按键分批读取

以 `7f6dea5f` 为隔离基线。固定 Laravel 的关系将按键分批转发给 Eloquent Builder，穿透关系显式限定关联表主键，并以未限定的主键名作为默认投影别名。OpenRust 在已有关系查询上添加批次边界，保留父键、单条选择、穿透软删除及多态目标约束。多态反向关系与已有偏移量分批共用目标选择，保留 `MorphTarget` 集合键。固定 Goravel 用作 Association 与 Cursor 装配对照，v0.x 的复合键批读取案例用于对照降序和回调失败边界。 [固定 Laravel 关系转发](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) · [固定 Laravel 穿透关系](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L679)

独立 MySQL 8.4.13 实际 HTTP 覆盖 HasMany、HasOne、MorphMany、MorphOne、HasManyThrough、HasOneThrough、BelongsTo、MorphTo，并回归多对多按键分批。实测父键、多态判别、软删除、限定列与默认投影别名、单条选择、缺失引用、无效批量和跨批次顺序。分页 HTTP、关系查询计数和关系批量写入三项定向回归通过。Rust 1.94.0 严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整。测试容器已回收，清理隔离 Cargo 构建产物与文档缓存。后续验收按条回调与惰性分批。

## 分批逐条回调

以 `73876418` 为隔离基线。固定 Laravel 的 `each` 将批内集合键交给回调，`eachById` 将 `(页码 - 1) × 批量` 加到集合键。Rust 的 Eloquent `each` 传递 `ArrayKey`，`eachById` 使用有符号整数并报告无效键或算术溢出。两者接收拥有所有权的行、模型或 `MorphTarget`，回调可跨 await 并通过 `Result<bool>` 停止或返回错误。查询、模型及关系复用原有批次，保留预加载、Pivot 和自定义集合键。v0.x 的 BatchReader 用于对照分批回调错误传播和停止行为。 [固定 Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php) · [固定 Laravel 穿透关系 eachById](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L713)

独立 MySQL 8.4.13 实际 HTTP 验证批内索引重置、按键索引跨批递进、自定义整数键、Pivot 字符串键、字符串键校验、回调早停与错误传播。分页 HTTP、关系查询计数及 Query Builder 三项定向回归通过。Rust 1.94.0 严格 Clippy、`./artisan inspire`、三语示例、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式仅在隔离副本处理。测试容器已回收，清除隔离构建和文档缓存。后续验收惰性分批及批量映射。

## 惰性分批与批量映射

以 `ae2ca44a` 为隔离基线。固定 Laravel 的 `lazy`／`lazyById` 按需逐批取数，逐条交付结果。按键读取在短尾批结束，满批之后才检查投影的边界键。Rust 的三种惰性入口返回一次性异步流，首次轮询执行验证和查询，丢弃流结束后续取数。关系沿用已验收的限定条件、每批水合、预加载与 Pivot。固定 Laravel 的 `chunkMap` 汇总映射值，Rust 支持异步映射和错误传播，返回重新编号的集合。v0.x BatchReader 的按批读取和错误边界用于对照。本批复用现有 Query Builder 的批次查询，未引入额外执行器。 [Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php) · [Laravel 穿透关系惰性读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L782)

独立 MySQL 8.4.13 实际 HTTP 验证未轮询即丢弃、首批提前结束、继续轮询时的下一批错误、按键降序及投影别名、初始偏移及总行数、短批与满批缺键差异、预加载、Pivot、多态目标、映射顺序和失败短路。分页 HTTP、关系查询计数和 Query Builder 三项定向回归通过。Rust 1.94.0 全目标严格 Clippy、`./artisan inspire`、三语示例、VitePress、18 项浏览器检查通过。既有 Sanctum 测试格式仅在隔离副本调整。已回收测试容器并清除隔离 Cargo 与站点构建缓存。后续按固定测试矩阵复核 05-02 与 05-06 的整单元。

## 关系、加载及集合复核

[固定测试矩阵](test-matrix.md)按 05-02 的外键、预加载、查询计数、集合键和关系写入归并目前证据。关系分页复核发现四个转发入口缺口，修复与验收见 [05-06 单条和反向关系分页](../05-06/README.md#单条和反向关系分页)。动态对象共享、集合分层输入契约及未逐项核对的关系用例仍沿矩阵跟踪。

## Pivot 枚举键、返回类型与仅创建时间

以 `4984830e` 为隔离基线。固定 Laravel 用例覆盖带值枚举与标量混合输入、toggle 往返、字符串返回类型及自定义 Pivot 只有创建时间。Rust 用既有 `CastableEnum::intoValue()` 将枚举显式转为数据库值。同步与切换复用关联键声明的整数、字符串或枚举转换，在匹配前转换输入，并在提交前转换解绑报告。未声明转换时沿用输入或数据库键类型。字符串绑定还保留 `01` 与 `1` 的区别，避免 MySQL 的数值比较合并文本键。v0.x 用于对照分组去重和增删改报告。 [固定枚举键用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToManyPivotEnumKeyTest.php) · [固定返回类型用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentBelongsToManySyncReturnValueTypeTest.php) · [固定时间戳用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentPivotWithoutTimestampTest.php)

独立 MySQL 8.4.13 实测类型化枚举同步与切换、UUID、超出 i64 的数字文本、前导零、变化报告类型及 Pivot 默认属性。物理表仅含 `created_at`，实测自定义 Pivot 新增写入创建时间，更新不改变创建时间。关系和查询计数、关系 upsert、实际分页 HTTP 三项定向回归通过。Rust 1.94.0 全目标严格 Clippy、`./artisan inspire`、三语示例、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式仅在隔离副本调整。测试容器与独占构建产物已回收。
