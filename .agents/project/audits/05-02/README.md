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
