# 05-04 模型工厂与 Seeder 集成

- 依照固定 Laravel 的工厂定义、状态、回调、创建和关系章节。核对本地 `v0.x` 和 `6d1a8192`，其中同名 `database/query/factory.rs` 负责查询构造。当前实现复用模型转换、持久化、关系描述符、生成器写入和 Seeder 连接选择。 [Laravel 工厂文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md)
- Rust 显式声明 `HasFactory`、模型和关系类型。状态与序列按顺序执行，工厂属性绕过批量赋值保护并保留修改器和转换。支持一对多、显式上级、已存在上级复用及带属性的中间表。批次内新上级按模型创建。动态关系名、多态工厂和完整 Faker 集成仍有接口差异。各模型按序写入，后续关系失败时之前的写入可能保留。
- 以 `9114963a` 为隔离基线。Rust 1.94.0 格式、449 项锁定离线工作区测试和全目标严格 Clippy 通过。SQLite 与真实 MySQL 验证未保存模型、状态、批次序列、回调、上下级及中间表持久化。
- 实际运行 `./artisan make:model FactoryProbe`、`./artisan make:factory FactoryProbeFactory --model=FactoryProbe`、同名拒绝和 `--force`。将生成文件纳入隔离应用后运行 `./artisan db:seed --database=sqlite --force`，SQLite 查得两条按序生成的模型。 `./artisan inspire`、三语 VitePress 构建和 12 项浏览器检查通过。

## 多态关系工厂与 MySQL Seeder

固定 Laravel 的 `Relationship` 支持普通及多态子关系，`BelongsToRelationship` 延迟解析上级并在同批复用。OpenRust 的 `has`、`r#for`、`forExisting` 和 `hasAttached` 接收显式关系描述符。每次执行各自持有上级缓存，独立调用重新解析，避免跨应用或连接复用。中间表沿用关系定义的关联键和类型字段。旧版只有查询构造工厂和自定义种子回调，复用当前的关系和 Seeder 契约。 [Laravel 多态工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#polymorphic-relationships)

独立 MySQL 8.4.13 验证普通模型工厂及实际 `db:seed --database=alternate --class=PolymorphicFactories --force`，覆盖同批共享上级、独立调用、已有上级、MorphMany 和带属性的多态中间表。Rust 1.94 格式、全目标严格 Clippy、三语示例编译、VitePress 及 18 项浏览器检查通过。工厂仍需显式传入关系描述符。Faker 与按类型回收模型池继续跟踪。

## 复用已有模型

对照 Laravel 的 `recycle` 与上级解析。Rust 的 `recycle` 和 `recycleMany` 可在 `count` 前后调用，类型化输入接受已持久化模型。模型池传递到嵌套工厂，按模型类型和连接匹配并随机选择。每个批次只解析一次上级，其他连接或无候选时生成新模型。旧版工厂仅用于查询构造，Seeder 装配继续沿用现有编译注册。 [固定 Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#recycling-an-existing-model-for-relationships)

MySQL 8.4.13 的工厂定向测试及实际 `db:seed --database=alternate --class=PolymorphicFactories --force` 通过。覆盖批内共享、独立连接隔离、无匹配时创建和多态中间表下的嵌套复用。Rust 1.94 格式、全目标严格 Clippy、Artisan、三语示例和 18 项浏览器检查通过。Faker、关联状态及回调阶段继续跟踪。

## 批量工厂回调阶段

对照固定 Laravel Factory 的 make、store 和 create。先构造整个批次并执行 afterMaking，再保存各模型及关系，最后执行批次的 afterCreating。嵌套子批次按相同阶段运行。Rust 回调通过 Result 传播失败。afterMaking 失败时当前批次无写入，构造上级产生的写入可保留。旧版查询工厂及 Seeder 回调没有对应模型生命周期。Goravel 的 Provider 装配仅作为编译注册参考。 [固定 Laravel 工厂源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php)

MySQL 8.4.13 验证批量回调、子关系写入顺序、失败前未写入及实际 Artisan Seeder。Rust 1.94 格式、全目标严格 Clippy、实际 Artisan、三语示例、站点构建及 18 项浏览器检查通过。关联状态合并及 Faker 仍需继续核对。

## 工厂关系属性合并

固定 Laravel 的 Relationship 与 BelongsToManyRelationship 先将关系查询默认属性合入工厂状态，再应用显式状态。直接及多态子关系最后补入外键与类型字段，afterMaking 可观察完整属性。Rust 继续通过类型化描述符解析并校验连接。旧版查询工厂与 Seeder 没有对应状态链。 [固定 Laravel Relationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Relationship.php) · [固定 Laravel BelongsToManyRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToManyRelationship.php)

MySQL 8.4.13 工厂测试及实际 Artisan Seeder 通过，覆盖显式状态读取和覆盖默认值、回调前填充关系键、附加关系默认值及多态嵌套回归。Rust 1.94 格式、全目标严格 Clippy、Artisan、三语示例、VitePress 和 18 项浏览器检查通过。Faker 与动态关系名称继续跟踪。

## Faker 数据生成

固定 Laravel 的工厂定义和 fake 助手从 app.faker_locale 选择数据源。Rust 通过 Factory::withFaker 从选定数据库连接的应用配置解析语言，也支持作用域和显式语言。fake-rs 提供原生数据及英文回退。已接姓名、安全邮箱、城市及句子生成，FakerPHP 的其他 formatter 和 unique 仍有差异。旧版无模型工厂 Faker 实现。Goravel 的启动装配无需调整。 [固定 Laravel 工厂文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md) · [fake-rs](https://github.com/cksac/fake-rs)

独立 MySQL 8.4.13 工厂测试及真实 db:seed 验证繁中姓名持久化、显式语言、非法语言拒绝和多态工厂回归。Rust 1.94 格式、严格 Clippy、实际 Artisan、三语示例及 18 项浏览器检查通过。

## Faker 唯一值生成

对照 FakerPHP 的 unique 修饰器，按生成方法记录已返回值，支持清空历史，最多尝试 10000 次。Rust 的 tryWithFaker 通过 Result 传播耗尽，生成器随单次模型工厂执行，整个批次共享，独立调用互相隔离。旧版没有对应修饰器。Goravel 的应用装配不涉及生成器内部状态。 [FakerPHP UniqueGenerator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/UniqueGenerator.php)

MySQL 8.4.13 工厂测试与实际 Artisan Seeder 通过，覆盖批内唯一值、重置、确定性耗尽、重复调用和失败批次无写入。Rust 1.94 格式、全目标严格 Clippy、Artisan、三语示例、站点构建及 18 项浏览器检查通过。FakerPHP 其余生成器及动态关系名称继续跟踪。

## 工厂按名称解析关系

对应固定 Laravel 工厂关系名称及魔术方法。OpenRust 使用 hasNamed、forNamed、hasAttachedNamed，调用时显式声明 Rust 描述符类型。复用应用动态登记和模型静态登记。名称缺失及类型不匹配会返回错误。旧版没有对应工厂机制。Goravel 的编译型装配无需调整。 [固定 Laravel 工厂关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#factory-relationships)

MySQL 8.4.13 工厂测试及实际 Artisan Seeder 通过，覆盖动态子关系、静态多态上级及中间表附加、错误名称及类型在创建上级前失败。Rust 1.94 格式、严格 Clippy、Artisan、三语示例及 18 项浏览器检查通过。PHP 魔术方法调用语法及 FakerPHP 其余生成器仍有差异。

## 常用 Faker formatter

姓名、邮箱和城市之外，补充 firstName、lastName、freeEmail、userName、streetName、postcode、phoneNumber、company 与 word，普通生成和 unique 使用同一套方法。Rust 使用 fake-rs 数据集，各地区样本和 FakerPHP 数据集仍有差异。旧版未发现等价 formatter。Goravel 启动装配无需调整。 [FakerPHP formatter](https://fakerphp.org/formatters/) · [fake-rs](https://docs.rs/fake/5.1.0/fake/faker/index.html)

独立 MySQL 8.4.13 工厂测试及实际 Artisan Seeder 验证生成器调用、繁中名字去重与持久化。Rust 1.94 格式、严格 Clippy、Artisan、三语示例和 18 项浏览器检查通过。FakerPHP 日期、数字和其他 formatter 继续跟踪。

## Faker 整数和日期区间

对照 FakerPHP v1.24.1，numberBetween 接受任意顺序的整数端点，包含两端。randomDigit 和 randomDigitNotNull 分别生成 0 至 9 和 1 至 9。dateTimeBetween 按整秒取样，拒绝逆序时间区间。Rust 显式传入两个 UTC 日期时间，调用方可以先解析或计算日期，也可将结果转换到所需时区。四个方法均支持 unique。旧版未发现等价生成器，Goravel 的装配无需调整。其他 FakerPHP formatter 继续跟踪。 [FakerPHP 数字实现](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php) · [FakerPHP 日期实现](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php)

独立 MySQL 8.4.13 的真实 `db:seed --database=alternate --class=PolymorphicFactories --force` 通过，核对整数端点、逆序参数、逆序日期报错、唯一值耗尽和写入数据库后的分数及时间戳。MySQL 与 SQLite 工厂回归、Rust 1.94 格式和全目标严格 Clippy、实际 Artisan、三语示例编译、VitePress 构建及 18 项浏览器检查通过。

## 工厂原始属性与静默创建

对照固定 Laravel 的 Factory::raw 和 createQuietly。Rust 以 Factory 和 FactoryBatch 分别返回属性映射和属性映射列表。属性展开复用定义、状态及上级解析，跳过目标模型转换、工厂回调及子关系。为解析关联键可以创建上级。静默创建复用任务作用域的 Model::withoutEvents，模型和关系照常写入，工厂回调照常执行。旧版未发现等价入口，Goravel 的启动装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php)

MySQL 8.4.13 的实际 db:seed 验证原始属性顺序、空批次、同批上级键复用、目标模型未写入，以及静默创建的回调和事件恢复。MySQL 和 SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译和 18 项文档浏览器检查通过。独立记录的 FakerPHP formatter 差异及工厂其他创建变体仍待核对。

## 单条与逐条创建

对照固定 Laravel Factory 的 createOne、createMany 及静默变体。createOne 忽略工厂数量，createMany 支持默认数量、指定数量和逐条属性。Rust 用 FactoryRecords 表达这三种输入，保留工厂原有数量供后续调用。逐条应用覆盖属性并执行保存、关系和回调，遇错停止，先前写入可以保留。同次调用共享上级解析和 Faker，Rust 序列继续使用批内索引。旧版未发现等价入口，Goravel 装配无需改动。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L253) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L103)

MySQL 8.4.13 实际 db:seed 验证默认和显式数量、空批次、属性覆盖、回调顺序、中途失败保留先前写入、静默创建及事件恢复。MySQL 与 SQLite 工厂回归、Rust 1.94 格式和全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 未保存模型与延迟创建

对照固定 Laravel Factory 的 makeOne、makeMany 与 lazy。单条构造忽略已配置数量，多条构造支持默认数量、显式数量和逐条覆盖属性，均执行 afterMaking。Rust 沿用 FactoryRecords 和显式连接入口。lazy 返回可重复调用的异步闭包，使用 Send 的 DatabaseFuture，执行时解析连接并保留单模型或批量创建的生命周期。旧版未发现对应工厂入口，Goravel 启动装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L342)

从磁盘停点恢复后，在隔离副本使用 Rust 1.94 通过格式和工作区全目标严格 Clippy 检查。MySQL 8.4.13 的实际 db:seed 验证单条和批量构造不写入目标表、默认数量、属性覆盖、零条结果、延迟写入及重复调用。MySQL 与 SQLite 工厂回归、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

在 `a6c9a039` 的隔离副本复验已提交的 `1a233c5e`。Rust 1.94.0 格式及工作区全目标严格 Clippy 通过。强制连接 MySQL 8.4.13 的 Seeder 集成用例及 MySQL／SQLite 工厂回归通过。另用应用二进制执行 `artisan db:seed --database=mysql --force`，确认 `makeOne` 和 `makeMany` 返回未保存的模型，零条返回空集合，单条及批次 `lazy` 在等待前保持零写入，重复调用后数据库共四条记录。`./artisan inspire`、三语 Seeder 示例编译、VitePress 构建及根路径和 `/docs/` 下六项浏览器检查通过。

固定 Laravel 的 `makeMany` 会清空工厂计数。Rust 的 `FactoryBatch` 在共享借用下保留其计数。三语文档同步说明了该差异和解析上级模型可能产生写入的边界。复验后清理隔离副本的 Cargo 产物 2.2 GiB 和站点产物 37 MiB。

在 `0ad665aa` 的干净隔离副本再次验证 `1a233c5e` 的三种入口。Rust 1.94.0 的 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通过。MySQL 8.4.13 与 SQLite 跑过 9 项 Seeder 测试和 2 项工厂回归。使用隔离应用的临时 Seeder 运行 `main artisan db:seed --database=mysql --force`。在 MySQL 回查确认单条、默认批量、逐条覆盖和零条预构造共写入零行，批量延迟回调返回的 Future 在等待前仍为零行，等待后批量写入两行，单条回调重复执行再写入两行。原始应用 Seeder 已在隔离副本恢复。

`./artisan inspire`、三语工厂示例编译、VitePress 构建，以及三语页面在站点根路径和 `/docs/` 下的六项浏览器检查通过。VitePress 报告大于 500 kB 的构建分块提示。验收后清理本次隔离副本 2.2 GiB Cargo 构建产物及约 37 MiB 站点产物，磁盘复查剩余 85 GiB。

### 当前数量语义复验

隔离基线 `dd8118ab`，复验 `1a233c5e` 的入口及 `ebfa2a31` 的计数重置。固定 Laravel 的 `makeMany` 取用已配置数量后清空计数。Rust 的 `FactoryBatch` 共用该行为，后续调用按一条处理，继续返回集合。三语示例使用独立的批量工厂演示 `lazy`，避免继承预构造已消耗的数量。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L469)

Rust 1.94.0 通过 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`，以及锁定的 38 项 Seeder 测试和 MySQL 8.4.13／SQLite 两项工厂回归。独立 MySQL 8.4.13 用应用二进制运行 `artisan db:seed --database=mysql --force`。回查确认 `makeOne`、默认数量、逐条覆盖和零条 `makeMany` 都没有向目标表写入，批次数量取用后回到一条。创建并调用 `lazy` 在等待 Future 前仍为零行，两次批量和两次单条调用依次写入四条和两条。原始 Seeder 已在隔离副本恢复。

`./artisan inspire`、三语示例编译、VitePress 构建及根路径和 `/docs/` 下六项浏览器检查通过。站点构建仍报告分块超过 500 kB 的提示。 验收后清理本批独占的 2.2 GiB Cargo 产物和约 38 MiB 站点产物，项目卷剩余 83 GiB。

### 当前主线恢复复验

隔离基线 `46630b49`。全量 Seeder 首轮复验发现延迟属性与模型主键／关系缓存用例遗留 `seed_factory_children` 和 `seed_factory_owners`，导致三个后续用例建表失败。给两项回归补齐测试表清理后，在全新的 MySQL 8.4.13 容器重跑 `cargo test --workspace --locked --test seeding --test eloquent_factories -- --test-threads=1`，49 项 Seeder 和 2 项 MySQL／SQLite 工厂回归通过。另用独立 MySQL 实例实际执行 `mysql_fresh_seeds_the_selected_database`。

使用实际应用二进制运行 `main artisan db:seed --database=mysql --force`。单条预构造、默认批量、逐条覆盖和零条批量均未写入目标表，默认批量消耗计数后回到一条。调用 `lazy` 并取得 Future 时仍为零行。两次批量、两次单条执行后按顺序回查到六条记录。

Rust 1.94.0 格式、工作区全目标严格 Clippy 和 `./artisan inspire` 通过。三语示例展示创建 Future 后再等待及重复调用，均编译通过。VitePress 构建和双挂载六项浏览器检查通过，保留已有分块大小提示。已恢复隔离副本的原始 Seeder，清理 2.2 GiB Cargo 产物和 38 MiB 站点产物，项目卷剩余 82 GiB。05-04 的 Faker provider 差异继续跟踪。

### 2026-11-15 工厂预构造与延迟创建复验

隔离基线 `3e1d2fc5`。对照固定 Laravel，`makeOne` 保留原工厂的批量计数，`makeMany` 取用并重置计数。Rust 的 `lazy` 返回异步回调，调用后开始等待 Future 时才执行创建。三语文档明确这三个入口组合使用时的数量与写入时机。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L342)

实际应用二进制在独立 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`。单条预构造、默认批量、逐条属性覆盖和零条预构造均保持目标表零写入，两次默认批量依次返回两条和一条。调用 `lazy` 取得 Future 后仍为零行。两次批量及两次单条回调执行后按主键回查到四条批量和两条单条记录。锁定的 Seeder 测试及 MySQL／SQLite 两项工厂回归通过。其中 `mysql_fresh_seeds_the_selected_database` 使用单独的临时数据库和显式 `OPENRUST_TEST_MYSQL_WIPE_URL` 执行，共 49 项 Seeder 均有执行结果。

Rust 1.94.0 格式、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译和 VitePress 构建通过，根路径和 `/docs/` 共六项浏览器检查通过。VitePress 仍有既有的分块大小提示。已恢复临时 Seeder 并清理本批 2.7 GiB Cargo 产物和 39 MiB 站点产物。提交前周额度剩余 50%，项目磁盘可用 82 GiB。

## 逐项序列与笛卡尔积序列

对照固定 Laravel 的 forEachSequence、crossJoinSequence 和 CrossJoinSequence。逐项序列按属性组数设定批次数量。笛卡尔积序列保留独立数量，最后一个序列变化最快，同名属性由后者覆盖。Rust 按批内索引直接计算组合，避免预先分配笛卡尔积，组合用完后循环。传入空子序列时返回错误。旧版未发现对应实现，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L636)

隔离基线 1a233c5e。实际 MySQL 8.4.13 Seeder 验证逐项数量、空批次、组合顺序、覆盖、循环与写入，MySQL／SQLite 工厂回归通过。Rust 1.94 格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 与 18 项浏览器检查通过。Factory::insert 仍依赖 Eloquent fillAndInsert。

## Eloquent 批量插入准备

对照固定 Laravel Eloquent Builder 的 fillForInsert、fillAndInsert、fillAndInsertOrIgnore 与 fillAndInsertGetId。复用模型默认值、查询预设属性、写入修改器和转换、唯一标识与时间戳。显式值优先，批量赋值保护和模型持久化事件不参与批量插入。Rust 使用显式属性列表和数据库 Value，Query Builder 校验各行列集合一致。旧版未发现对应入口，Goravel 装配无需调整。 [固定 Laravel Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php#L495)

隔离基线 020235d2。MySQL 8.4.13 实际 db:seed 验证默认值优先级、修改器、JSON 写入转换、UUID、显式时间戳、空输入、事件抑制、重复键忽略和生成 ID。MySQL／SQLite 工厂回归、Rust 1.94 格式和全目标严格 Clippy、Artisan inspire、VitePress 和 18 项浏览器检查通过。下一批用该入口接 Factory::insert。

## 工厂批量插入

对照固定 Laravel Factory::insert。单模型和批量工厂复用构造阶段并执行 afterMaking，目标行通过同一条插入语句写入。目标模型的持久化事件、子关系创建和 afterCreating 跳过，构造阶段仍可解析并写入上级。Rust 使用已完成转换的存储属性，保留隐藏列，省去序列化和第二次写入转换，共用 fillForInsert 的标识与时间戳准备逻辑。旧版未发现对应入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L460)

隔离基线 c57eff7a。MySQL 8.4.13 实际 db:seed 覆盖单条、批量、空批次、隐藏列、afterMaking、UUID、时间戳、模型事件和子关系跳过，以及重复键导致该批次整条语句失败。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂回调控制

对照固定 Laravel 的 withoutAfterMaking 和 withoutAfterCreating。单模型及批量工厂可独立清除对应阶段的已登记回调，再登记新回调。Rust 消费构造器并返回更新后的实例，保留批次数量。另一阶段的回调和 Eloquent 模型事件照常执行。旧版未发现等价入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L772)

隔离基线 46826a87。MySQL 8.4.13 实际 db:seed 覆盖回调清除、重新登记、另一阶段回调、批量写入和模型事件仍能取消写入。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂状态前置与单属性覆盖

对照固定 Laravel 的 prependState 和 set。prependState 在所有已登记状态前执行，后续状态能读取其结果。set 在末尾追加单属性覆盖。单模型和批量工厂均可配置状态，直接传入创建方法的属性最后应用。Rust 复用现有状态闭包和类型化属性映射。旧版未发现等价入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L594)

隔离基线 14e03b32。MySQL 8.4.13 实际 db:seed 验证前置顺序、状态读取、单属性覆盖、批次数量及逐条覆盖实际入库。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂连接选择

对照固定 Laravel Factory 的 connection 和 getConnectionName。工厂覆盖优先于模型连接及当前应用默认连接，单模型与批量入口共用解析，lazy 在执行时选取连接。Rust 保留显式 On 入口，并优先使用调用方传入的连接。嵌套工厂按自身覆盖、模型连接及外层连接解析，保留既有关系连接校验。可从外层连接所属应用解析命名连接。旧版未发现等价工厂入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L845)

隔离基线 e6986332。MySQL 8.4.13 实际 db:seed 验证显式连接写 SQLite、On 写 MySQL、lazy 批量写入、清除覆盖回退至 Seeder 连接、嵌套上级连接和未知连接错误。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂附加关系的中间表属性

对照固定 Laravel Factory::hasAttached 和 BelongsToManyRelationship。单个属性映射继续共用。列表按关联模型顺序循环，父模型解析器对每条关联执行一次。Rust 用 FactoryPivot 表达这些输入，调用方通过 count 指定关联数量，解析器返回 Result。列表或解析器沿用单条附加行为，前一条写入可在后续错误时保留。旧版未发现对应工厂入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L685) · [固定 Laravel BelongsToManyRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToManyRelationship.php#L47)

隔离基线 31cc40e4。MySQL 8.4.13 实际 db:seed 验证多态中间表顺序、列表循环、解析器逐条运行及父模型已持久化。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂附加已有模型

对照固定 Laravel Factory::hasAttached 与 BelongsToManyRelationship 的已有模型分支。Rust 支持传入单个模型、列表或 Eloquent 集合，沿用共享、序列和回调中间表属性。关系要求模型已持久化、关联键存在且连接来源一致，校验目标后才写中间表。外层模型已在关系写入前保存。旧版未发现对应工厂入口，Goravel 装配无需调整。 [固定 Laravel BelongsToManyRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToManyRelationship.php#L47)

隔离基线 9ad57012。MySQL 8.4.13 实际 db:seed 覆盖单模型和集合、多态中间表顺序、原关联模型行数、未保存目标和 SQLite 跨连接拒绝。测试夹具的新增路径独立装箱，按默认栈复验通过。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## 工厂外键属性展开

对照固定 Laravel Factory 的 expandAttributes 和 withoutParents。Rust 用 parentAttribute 声明工厂外键属性，展开时优先读取显式属性，按需生成上级或复用同类型同连接模型。按属性名或模型类型排除时生成 Null，显式 for 与 forExisting 关系继续解析。批次中每个未覆盖的属性独立展开。旧版未发现对应入口，Goravel 装配无需调整。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L550)

隔离基线 7081089a。MySQL 8.4.13 实际 db:seed 验证全部禁用、按名及类型排除、显式覆盖、批次生成、模型复用、显式关系与实际入库。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## Faker 数字排除与位数生成

对照 FakerPHP 1.24.1 的 randomDigitNot 和 randomNumber。randomDigitNot 从九个候选值中取样并跳过指定值。randomNumber 支持随机位数、严格位数和非严格零位，保留上游的 32 位最大随机值约束。Rust 使用 Option 和 Result 表达缺省值及非法位数，拒绝严格零位。两个方法沿用 unique 去重。旧版未发现对应入口，Goravel 装配无需调整。 [FakerPHP Base](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L45)

隔离基线 8bea2ca1。MySQL 8.4.13 实际 db:seed 验证排除值、严格和缺省位数、非法边界、零位及唯一值耗尽。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。

## Faker 浮点数生成

对照 FakerPHP 1.24.1 的 randomFloat。按取样后舍入实现，支持逆序区间、缺省精度与上界、中点远离零及负小数位。Rust 使用有限 f64 与 Result，拒绝非有限边界和舍入溢出，使用原生随机数生成器。unique 统一正负零。旧版未发现对应入口，Goravel 装配无需调整。 [FakerPHP Base](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L104)

隔离基线 459e681e。MySQL 8.4.13 实际 db:seed 验证正负中点、逆序区间、默认上界、非有限值拒绝及零值去重。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。本机 PHP 缺少动态库，浮点舍入未运行 PHP 对照脚本。

## Faker Unix 时间与日期格式

对照 FakerPHP 1.24.1 的 unixTime、dateTime、date、time 和 iso8601。时间以整秒取样，缺省上界为当前时间，可传入早于纪元的端点。Rust 接收 Chrono 的 UTC 日期时间，date 和 time 接收 Chrono 格式并对无效格式返回错误。返回的 UTC 日期时间可由调用方转换时区。五个入口共用 unique。旧版未发现对应生成入口，Goravel 装配无需调整。 [FakerPHP 日期实现](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L35)

隔离基线 101a1e85。MySQL 8.4.13 实际 db:seed 验证纪元、早于纪元的区间、缺省上界、格式、无效格式和唯一值耗尽。MySQL／SQLite 工厂回归、Rust 1.94 格式及全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 18 项浏览器检查通过。PHP 相对时间字符串、日期区间变体与其余 formatter 仍待核对。

## Faker 日期区间

对照 FakerPHP 1.24.1 的 `dateTimeInInterval`、`dateTimeThisCentury`、`dateTimeThisDecade`、`dateTimeThisYear` 和 `dateTimeThisMonth`。Rust 用 UTC 日期与带符号的 Chrono `TimeDelta` 表达区间，沿用整秒闭区间取样、端点检查和 `unique`。负时长交换端点。月和年回溯保留日历溢出归一化，当年起点为元旦 UTC 零点。旧版有 PHP 日期解析器及溢出归一化，可参考其规则，当前有类型的接口只需按原日序归一化。Goravel 装配无需调整。任意 PHP 相对时间表达式及其余日期格式化入口继续跟踪。 [FakerPHP 日期区间](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L167)

隔离基线 `9baa878f`。PHP 8.5.12 验证月底减月、闰日减年和元旦零点。Rust 1.94.0 格式及工作区全目标严格 Clippy 通过。MySQL 8.4.13 的 Seeder 回归覆盖正负时长、零时长、溢出、无效上界、默认区间和唯一值耗尽。应用二进制实际运行 `artisan db:seed --database=mysql --force` 并回查纪元记录。MySQL／SQLite 工厂回归、`./artisan inspire`、三语日期与 Seeder 示例编译、VitePress 和六项浏览器检查通过。

## Faker 日期组成格式

对照 FakerPHP 1.24.1 的 `dateTimeAD`、`amPm`、`dayOfMonth`、`dayOfWeek`、`month`、`monthName`、`year` 和 `century`。Rust 保留 UTC 和有类型的时间上界，沿用 64 位公元日期下界、补零字符串、英文日期名称及 I 至 XXI 的候选集。全部接入 `unique`。旧版有日期格式解析，未发现同类 Faker 生成入口。Goravel 装配无需调整。时区标识生成、默认时区和任意 PHP 相对时间表达式继续跟踪。 [FakerPHP 日期实现](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L72)

隔离基线 `265f9646`。PHP 8.5.12 验证 64 位日期下界、纪元输出和有符号年份格式。MySQL 8.4.13 的 Seeder 集成用例验证八个入口、去重和罗马数字耗尽。应用二进制运行 `artisan db:seed --database=mysql --force`，写入并回查日期组成值。MySQL／SQLite 工厂回归、Rust 1.94.0 格式及工作区全目标严格 Clippy、`./artisan inspire`、三语日期示例编译、VitePress 和六项浏览器检查通过。

## Faker 时区标识

对照 FakerPHP 1.24.1 的 `timezone` 和 `randomElement`。内置与 chrono-tz 0.10.4 同版的 IANA 2025b 公共领域 `zone.tab`，首次使用时按国家或地区建立索引。通用候选包含 UTC，筛选按原代码大小写匹配，未知代码返回 `None`，对应 PHP 的 null。`unique` 按 formatter 跟踪重复结果，未知代码也参与去重。旧版未发现国家到时区的标识列表。Goravel 装配无需调整。默认时区和 PHP 相对时间表达式继续跟踪。 [FakerPHP](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L313) · [IANA 2025b](https://data.iana.org/time-zones/tzdb-2025b/zone.tab)

隔离基线 `943690e6`。PHP 8.5.12 对照全部 419 个通用候选和 247 个国家或地区候选集，全部标识可由锁定的 chrono-tz 解析。MySQL 8.4.13 Seeder 用例验证 CN、TW、未知和小写代码、全表回退与去重耗尽。实际应用二进制运行 `artisan db:seed --database=mysql --force`，写入并回查 `Asia/Taipei`。MySQL／SQLite 工厂回归、Rust 1.94.0 格式和工作区全目标严格 Clippy、`./artisan inspire`、三语日期示例编译、VitePress 和六项浏览器检查通过。

## Faker 默认时区

固定 Laravel 装配从 `app.timezone` 设置应用时区，FakerPHP 的日期格式化采用显式时区、Faker 覆盖值和应用默认值。Rust 从应用或工厂连接的容器读取 `app.timezone`，无作用域时使用 UTC。覆盖值由当前生成器持有，支持设置、查询和清除。日期字符串使用有效时区，`dateTimeInTimezone` 支持单次显式时区。已公开的 `DateTime<Utc>` 方法和 UTC 区间保持兼容。旧版日期解析有独立时区上下文，没有对应的 Faker 配置。Goravel 装配无需调整。PHP 相对日期输入和按本地日历计算的区间边界继续跟踪。 [Laravel 配置加载](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/LoadConfiguration.php#L59) · [FakerPHP 时区解析](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L330)

隔离基线 `fdcd57f0`。PHP 8.5.12 对照 UTC、台北与纽约的纪元日期和偏移。MySQL 8.4.13 的 Seeder 回归验证应用配置、显式语言继承时区、生成器覆盖和清除、单次覆盖、生成器间隔离及工厂连接配置。应用二进制使用 `APP_TIMEZONE=Asia/Taipei` 执行 `artisan db:seed --database=mysql --force`，写入并回查 `1970-01-01T08:00:00+0800`。MySQL／SQLite 工厂回归、Rust 1.94.0 格式及工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 本地日历区间

对照 FakerPHP 1.24.1 中由 PHP 默认时区解释的日历相对量。世纪、十年、月回溯和元旦零点按应用时区计算，省略区间起点时也按应用时区回溯三十年，两个参数都省略时默认终点增加五个本地日历日。显式 UTC 起点及 `TimeDelta` 沿用原有契约。复用 v0.x `support/calendar/date/zone.rs` 的 timelib 回拨选择和跳时前偏移规则。生成器覆盖只影响输出，取样继续返回 UTC。任意 PHP 日期表达式和旧版扩展年份的完整时区规则仍待移植。 [FakerPHP 日期区间](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L167)

隔离基线 `62db8916`。PHP 8.5.12 对照纽约与巴黎的回拨、纽约和 Lord Howe 的跳时及月底溢出，Rust 隔离探针逐项通过。实际应用二进制使用 `APP_TIMEZONE=Asia/Taipei` 运行 `artisan db:seed --database=mysql --force`，验证台北元旦零点并写入回查。MySQL 8.4.13／SQLite 工厂回归、Rust 1.94.0 格式和全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 字符串日期端点

对照 FakerPHP 1.24.1 的 `dateTimeBetween` 和 `getMaxTimestamp`。Rust 保留类型化方法，新增 `dateTimeBetweenExpressions` 接收绝对和相对日期字符串。两个端点共用显式参考时刻，应用时区用于缺项，表达式可指定时区，数字终点按 Unix 秒处理。字符串与类型化调用共享 unique 记录。复用 v0.x `6d1a8192` 的扫描器、公历运算、PHP 8.5.0 / IANA 2025b 历史及 POSIX 尾规则，按 PHP 的显式时区行为调整回拨选择。超出 Chrono 可表示范围或无效输入以 Rust 错误返回。Goravel 装配无需调整。字符串区间另一个入口 `dateTimeInInterval` 的 PHP DateInterval 语义、其他 formatter 的字符串上界及全语法差分覆盖仍待完成。 [FakerPHP 字符串区间](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L136)

隔离基线 `4f560aea`。PHP 8.5.12 对照月底溢出、纽约跳时、巴黎显式时区回拨、2100 年纽约偏移、相对星期和纪元前时间。MySQL 8.4.13 Seeder 回归使用同一组结果，并验证非法日期、反向范围和去重耗尽。应用二进制执行 `artisan db:seed --database=mysql --force` 写入并回查 2100 年纽约样本。Rust 1.94.0 格式、工作区全目标严格 Clippy、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 字符串相对区间

对照 FakerPHP 1.24.1 `dateTimeInInterval` 和 PHP 8.5.0 `DateInterval::createFromDateString`／`timelib_add`。复用 v0.x 已移植的日期词法和相对运算，提取相对组件并按起始日期时区相加，按起始夏令时状态解析回拨重复时刻。表达式入口默认三十年前和五个日历日，与类型化 `dateTimeInInterval` 共享去重历史。类型化 `TimeDelta` 仍表示经过时长。v0.x 的 Carbon interval 入口面向请求值转换，Faker 沿用日期解析器实现 PHP 日期相加。Goravel 装配无需调整。其他 formatter 的字符串上界和全语法差分覆盖仍待完成。 [FakerPHP 日期区间](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L167) · [PHP 日期相加](https://github.com/php/php-src/blob/php-8.5.0/ext/date/lib/interval.c)

隔离基线 `5a2c334f`。PHP 8.5.12 对照月底、相对星期、负区间、非法日期元素、纽约跳时以及纽约和巴黎回拨。Rust 固定端点回归及 MySQL 8.4.13 Seeder 集成通过，应用二进制用 `artisan db:seed --database=mysql --force` 写入并回查纽约跨夏令时样本。Rust 1.94.0 格式和工作区全目标严格 Clippy、MySQL／SQLite 工厂回归、`./artisan inspire`、三语代码示例编译、VitePress 和六项浏览器检查通过。

## Faker 字符串日期上界

对照 FakerPHP 1.24.1 `getMaxTimestamp`，由 `resolveDateMax` 统一解析字符串上界，所有接受 UTC 日期上界的 formatter 可直接消费同一类型值。日期区间也共用该解析器和参考时刻。复用 v0.x 已迁入的数字词法和时区日期解析。数字字符串按 Unix 秒向零截断，空字符串取当前时间，日期表达式默认按应用时区解析。无效、纯空白和超出 Chrono 范围的值返回错误。独立解析保留 Rust 日期方法的返回类型与 unique 契约。Goravel 装配无需调整。完整 PHP 日期语法差分和 PHP 的无效值／溢出强制转换继续跟踪。 [FakerPHP 上界解析](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L14)

隔离基线 `e9b4659f`。PHP 8.5.12 对照小数、指数、零、空值和应用／显式时区。MySQL 8.4.13 Seeder 集成验证解析、错误和共享上界；应用二进制实际执行 `artisan db:seed --database=mysql --force` 写入并回查台北格式化结果。Rust 1.94.0 格式、全目标严格 Clippy、SQLite／MySQL 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 扩展日期的 Unix 秒上界

对照 FakerPHP 1.24.1 的 `unixTime` 和 PHP 数字字符串转整数。字符串上界在整数层解析，`unixTimeExpression` 支持扩展年份与完整 i64 取样区间，并与 `unixTime` 共用 unique。有限数字溢出按 PHP 规则饱和，指数溢出至无穷大取零，兼容 PHP 数字空白。复用 v0.x 已迁入的日期词法和扩展公历运算。日期对象方法仍要求 Chrono 可表示的时间，非法日期表达式仍返回错误。Goravel 装配无需调整。 [FakerPHP Unix 秒](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L14)

隔离基线 `f71294a5`。从 v0.x 日期夹具筛选 265 个无自定义格式、字符串长度不超过 1024 的日期样本，在固定 UTC 参考时刻逐项对照 PHP 8.5.12 `strtotime`。原日期对象路径有五项因 Chrono 范围失败，独立 Unix 秒路径 265 项全部匹配。MySQL 8.4.13 实际应用二进制执行 `artisan db:seed --database=mysql --force` 写入并回查扩展年份取样。MySQL／SQLite 工厂回归、Rust 1.94.0 格式和严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。样本集之外的日期语法和 FakerPHP 无效输入的隐式零值继续跟踪。

## Faker 无效日期的数值回退

固定 FakerPHP 1.24.1 将 `strtotime` 失败得到的 false 传给数字取样，效果为 Unix 零秒。Rust 的 `unixTimeExpression` 和 `dateTimeBetweenExpressions` 复现这一行为，起点晚于终点仍返回错误。严格入口 `resolveDateMax` 保留输入校验，字符串相对区间保留无效日期错误。复用 v0.x 对语法、数值范围和资源限制的区分，仅语法错误进入零值回退。Goravel 装配无需调整。 [FakerPHP 日期端点](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L136) · [PHP strtotime](https://github.com/php/php-src/blob/php-8.5.0/ext/date/php_date.c)

隔离基线 `fca741e0`。实际 FakerPHP 1.24.1 和 PHP 8.5.12 验证无效上界、双端点零值、反向区间和相对区间报错。MySQL 8.4.13 Seeder 回归另外验证超长输入不会转成零，应用二进制运行 `artisan db:seed --database=mysql --force` 写入并回查零值。Rust 1.94.0 格式与工作区全目标严格 Clippy、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。全日期语法及其他 Faker provider 的覆盖继续跟踪。

## Faker 浮点舍入差分

对照 FakerPHP 1.24.1 `randomFloat` 和 PHP 8.5.0 `round`，按十进制边界判断半值，恢复缩放时丢失的整数，并在高精度时通过十进制重读避免二次舍入。超出可用正精度或舍入后超范围时保留取样值，负精度低于 −308 时取零。Rust 继续使用现有随机源，拒绝非有限边界，unique 合并正负零。v0.x 无对应生成器。Goravel 装配无需调整。 [FakerPHP 随机浮点](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L104) · [PHP 8.5 舍入](https://github.com/php/php-src/blob/php-8.5.0/ext/standard/math.c#L165)

隔离基线 `2d29104f`。实际 FakerPHP 1.24.1／PHP 8.5.12 与 Rust 舍入探针对照 7,911 组固定上下界样本，包含半值、极小正负值、正负精度和溢出，全部一致。固定回归从大型异步 Seeder 夹具拆到独立测试，默认栈复验。实际应用二进制运行 `artisan db:seed --database=mysql --force`，在 MySQL 8.4.13 写入并回查高精度样本。MySQL／SQLite 工厂回归、Rust 1.94.0 格式和全目标严格 Clippy、`./artisan inspire`、三语示例、站点构建和六项浏览器检查通过。其他 Faker provider 继续跟踪。

## Faker 文本生成

对照 FakerPHP 1.24.1 Lorem 的固定数量、浮动数量、数组／拼接文本和 `text` 的 25、100 字节分段。Rust 使用 `AsText` 入口表达字符串返回值，保留已有按范围取样的 `sentence`，增加 `sentenceWithWordCount`。复用 fake-rs 词库与生成器，类型化列表支持独立去重，文本保持 UTF-8 边界。PHP 通过反复取样避开超长生成项，Rust 在重试耗尽后报错。v0.x 未发现对应生成器。Goravel 装配无需调整。 [FakerPHP Lorem](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Lorem.php#L51)

隔离基线 `c01db420`。固定 FakerPHP 和 PHP 8.5.12 实测零数量、固定数量及 5、24、25、99、100、200 字节。Rust 回归验证数量、连接符、限制、空列表去重及重置和中文文本，MySQL 8.4.13 应用二进制使用 `APP_FAKER_LOCALE=zh_CN` 执行 `artisan db:seed --database=mysql --force` 写入并回查。MySQL／SQLite 工厂回归、Rust 1.94.0 格式及全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 占位符格式化

对照 FakerPHP 1.24.1 Base 的 `numerify`、`lexify`、`bothify`、`asciify`、`randomLetter` 和 `randomAscii`。Rust 用 Option 表达默认模板，使用一次字符扫描和现有随机源替换占位符。保留非占位符 Unicode 字符及 `bothify` 的字母／数字等概率选择。六个入口支持 unique。v0.x 未发现对应模板实现，Goravel 装配无需调整。正则表达式生成继续跟踪。 [FakerPHP Base](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L437)

隔离基线 `ebee71a3`。FakerPHP 1.24.1／PHP 8.5.12 对四种模板各运行 64 次并校验字符类别。Rust 回归覆盖默认值、空模板、超过九位的数字串、繁體字面值与去重重置。实际应用二进制执行 `artisan db:seed --database=mysql --force`，在 MySQL 8.4.13 写入并回查混合编号。MySQL／SQLite 工厂回归、Rust 1.94.0 格式和全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 正则样本

对照 FakerPHP 1.24.1 Base 的 `regexify`，接通可选斜杠及外层锚点、字面值、ASCII 字符集、数字和小写字母转义、点号、分组、分支及重复。每次生成对 `*` 和 `+` 分别取一至九的上限。Rust 复用锁定的 regex-syntax 解析，按语法树生成并限制复杂度。对无效语法、未支持结构和超限请求返回错误。去重重试复用语法树并重新采样。v0.x 未发现对应生成器，Goravel 装配无需调整。 [FakerPHP Base](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L541)

隔离基线 `e5ccdfb1`。固定 FakerPHP 1.24.1／PHP 8.5.12 三组模式各生成 64 个结果，全部满足原正则。Rust 回归逐个匹配 256 个样本，并验证空模式、语法错误、不支持的特性、展开限制和去重重置。实际应用二进制运行 `artisan db:seed --database=mysql --force`，在 MySQL 8.4.13 写入并回查含繁體字面值的样本。MySQL／SQLite 工厂回归、Rust 1.94.0 格式和全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

## Faker 集合采样与布尔值

对照 FakerPHP 1.24.1 Base 和 Miscellaneous，接入 randomElement、randomKey、randomElements、shuffleArray、shuffleString 和 boolean。Rust 用切片表示有序输入，按位置不放回采样，保留相同值的独立位置。使用稀疏 Fisher–Yates，辅助空间随采样数量增长。映射键与枚举变体由调用方提供切片，字符串使用 UTF-8 Unicode 标量，数组与字符串各用类型化方法。泛型 unique 以 JSON 序列化结果为键，序列化失败返回错误。v0.x 未发现等价实现，Goravel 启动装配无需调整。 [FakerPHP Base](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L193) · [FakerPHP Miscellaneous](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Miscellaneous.php)

隔离源码基线 `0ad665aa`。固定 FakerPHP／PHP 8.5.12 对照通过，Rust 验证空输入、抽取数量、重复值、放回、Unicode 标量、布尔极值和 unique 耗尽。MySQL 8.4.13 实际应用二进制运行 `artisan db:seed --database=mysql --force`，持久化打乱后的 Unicode 字符串并回查。Rust 1.94.0 格式、全目标严格 Clippy、10 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 产物及约 37 MiB 站点产物。其他 Faker provider 继续跟踪。

## Faker 语言环境、国家、货币和表情样本

对照 FakerPHP 1.24.1 Miscellaneous，按原目录接通 languageCode、countryCode、countryISOAlpha3、currencyCode、emoji 及随机语言环境。Rust 的 randomLocale 对应上游 locale 生成器，保留已有 locale 配置读取接口。样本目录保留固定上游的历史标识，随机抽取不改变应用语言配置。六个入口支持 unique。目录附上游 MIT 声明。v0.x 未发现等价生成器，Goravel 启动装配未涉及。 [FakerPHP Miscellaneous](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Miscellaneous.php#L280)

隔离基线 `fab2ddcf`。在 PHP 8.5.12 读取固定版本六组目录，与 Rust 逐项比对 1,145 个值，全部一致。Rust 验证各目录去重耗尽、重置、语言环境格式和配置读取。MySQL 8.4.13 实际运行 `artisan db:seed --database=mysql --force` 写入六种样本并回查。Rust 1.94.0 格式、工作区全目标严格 Clippy、11 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 产物和约 37 MiB 站点产物。Miscellaneous 的摘要生成器后续补齐。

## Faker 摘要样本

对照固定 FakerPHP 1.24.1 Miscellaneous 的 md5、sha1、sha256，从闭区间 0 至 2,147,483,647 抽取整数，对十进制文本计算小写十六进制摘要。三种生成器支持各自的 unique 历史。Rust 继续使用现有 Faker 随机源，输出长度与算法对应。SHA-1 和 SHA-256 复用 AWS-LC，MD5 直接引用锁文件已有的 md-5 0.11.0。旧版签名模块的 SHA-256 同样输出小写十六进制，没有 Faker 随机入口。Goravel 启动装配未涉及。 [FakerPHP Miscellaneous](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Miscellaneous.php#L250)

隔离基线 `2811a5c2`。PHP 8.5.12 对固定 FakerPHP 的三个入口各运行 64 组固定种子，验证输入边界和摘要规则。Rust 记录 256 个随机输入及各自三种摘要，与 PHP 逐项对照一致。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查三种摘要。Rust 1.94.0 格式、工作区全目标严格 Clippy、12 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。已清理本批 2.2 GiB Cargo 产物和约 37 MiB 站点产物。其余 provider 继续跟踪。

## Faker 网络地址样本

对照固定 FakerPHP 1.24.1 Internet 的 ipv4、localIpv4、ipv6 与 macAddress。普通 IPv4 等概率选择两个上游范围，保留私有及特殊用途地址。私有 IPv4 等概率选择三个网段，再均匀抽取网段内地址。IPv6 使用八组不补零、不压缩的小写十六进制，MAC 使用六组补零的大写十六进制。Rust 用无符号位模式和标准库 IPv4 格式化，四个入口均支持独立 unique 历史。v0.x 未发现对应生成器，Goravel 启动装配未涉及。 [FakerPHP Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php#L225)

隔离基线 `14dbc536`。固定 FakerPHP 与 PHP 8.5.12 对四个入口各检查 256 次，Rust 回归也对四个入口各检查 256 次，覆盖地址范围、格式和去重。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查四种地址。Rust 1.94.0 格式、全目标严格 Clippy、13 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。Internet 的域名、URL、slug 和其余文本生成器继续跟踪。

## Faker 域名、URL 和 slug

对照固定 FakerPHP 1.24.1 Internet，接通 domainWord、tld、domainName、slug 和 url，保留十种 URL 模板的权重、受支持地区的后缀分布、固定或浮动词数和独立 unique 历史。Faker 复用基于 v0.x 转写实现移植的 Str，按上游过滤姓氏字符。Rust 使用 fake-rs 的语言资料和现有转写表，姓氏、词库与部分字符转写可以和 PHP 不同。姓氏转写结果为空时返回错误。Rust 由 Option 表达默认词数。Goravel 启动装配未涉及。 [FakerPHP Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php#L157)

隔离基线 `510bde88`。固定 FakerPHP 与 PHP 8.5.12 用实际生成器检查域名、URL 和 slug，以及 zh_CN、zh_TW、fr_FR 后缀集合。Rust 回归检查相同契约、去重耗尽和重置。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查域名、URL 和 slug。Rust 1.94.0 格式、工作区全目标严格 Clippy、14 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。Internet 的 email、邮件域名和密码样本继续跟踪。

## Faker 邮件地址和密码样本

对照 FakerPHP 1.24.1 Internet，接通 email、companyEmail、safeEmailDomain、freeEmailDomain、password，更新已有 userName、safeEmail、freeEmail。简体中文的通用邮件走免费邮箱名单，其他地区在随机域名及免费邮箱模板之间等概率取样。九个受支持地区的免费邮箱目录逐项沿用固定上游。用户名基于常用模板、fake-rs 词库及 v0.x 移植的 Str 转写。地区专有用户名数据仍有差异，转写无可用字符时生成一个 ASCII 字母，以保留既有无错误返回签名。密码以 Option 表达省略长度，包含两端且允许反向边界，从 ASCII 33–126 生成，分配失败返回错误。所有入口支持独立 unique 历史。旧版未发现对应 Faker 生成器，Goravel 启动装配未涉及。 [FakerPHP Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php#L47)

隔离基线 `68eb269e`。实际 FakerPHP 与 PHP 8.5.12 运行 128 轮邮件域名、字符范围和密码边界对照。Rust 回归覆盖简体中文邮箱、反向与零长度、分配错误和去重，复验域名保持小写。MySQL 8.4.13 实际运行 `artisan db:seed --database=mysql --force` 写入并回查邮件和密码样本。Rust 1.94.0 格式、全目标严格 Clippy、15 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。

## Faker 电话号码和 IMEI

对照固定 FakerPHP 1.24.1 PhoneNumber。简体中文 phoneNumber 使用运营商前缀和 11 位号码，繁体中文使用固定移动及市话格式。其他语言的显示号码沿用 fake-rs。e164PhoneNumber 使用固定国际格式及八个地区的覆盖。美、德区号复用已接入的 Faker 正则生成，首次编译后缓存，保留美国交换码规则。imei 保留字符串及前导零，按 Luhn 计算第 15 位。三个入口支持各自的 unique 历史。目录附上游 MIT 声明。v0.x 未发现等价生成器，Goravel 启动装配未涉及。 [FakerPHP PhoneNumber](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/PhoneNumber.php)

隔离基线 `53af1cbf`。PHP 8.5.12 与 Rust 回归验证五个地区的国际号码前缀、长度、数字格式，以及简繁中文显示号码和 IMEI 校验位。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并原样回查简繁电话、国际号码及 IMEI。Rust 1.94.0 格式、工作区全目标严格 Clippy、16 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。独立 Seeder 夹具有一处未使用变量警告。清理本批 2.2 GiB Cargo 及约 37 MiB 站点产物。

## Faker 邮寄地址和称谓

对照固定 FakerPHP 1.24.1 Address、Person 及地区 provider。接通 streetAddress、address、buildingNumber、secondaryAddress、cityPrefix、citySuffix、streetSuffix、country、state、stateAbbr 和三种称谓入口。组合器覆盖美国、法国、简体及繁体中文的地址排列。邮编对齐美国五位或九位与简体中文的六位格式。复用 OpenRust 的 fake-rs 城市、街道和部分组件目录，地址样本可含独立抽取的城市和区。Rust 用 Option 表达省略称谓组，不同 formatter 保持独立 unique 历史。上游数据附 MIT 声明。v0.x 未发现对应 Faker 入口，Goravel 启动装配未涉及。 [FakerPHP Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Address.php#L84) · [FakerPHP Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Person.php#L113)

隔离基线 `6acdb796`。PHP 8.5.12 和 Rust 各验证四种地区的 64 组地址契约及简繁称谓。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查三种地址和称谓，保留中文及美式换行。Rust 1.94.0 格式、全目标严格 Clippy、17 项 Seeder、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。地区性别名字、新地区地址规则及坐标生成器继续跟踪。

## Faker 按性别生成名字

对照固定 FakerPHP 1.24.1 Person。接通 firstNameMale、firstNameFemale、firstNameWithGender 和 nameWithGender，使用 13 个地区的固定名字词库和加权全名模板。未识别分组按上游随机取样。繁体中文名字按上游有放回取一个或两个字符。Rust 保留已有无参方法并共享对应 unique 历史，姓氏继续使用 fake-rs 数据，未覆盖地区使用英语名字词库和模板。荷兰语及土耳其语全名复用对应称谓组。目录保留上游 MIT 声明。v0.x 未发现等价 Faker 入口，Goravel 装配未涉及。 [FakerPHP Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Person.php#L48)

隔离基线 `fcb6d868`。PHP 8.5.12 执行固定生成器，并逐项比对 13 个地区的 9,185 条名字数据及全名模板、前后缀。Rust 回归覆盖 13 个地区、英文回退、简繁中文字符、显式分组和 unique。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查简体、繁体和英文全名。Rust 1.94.0 格式、工作区全目标严格 Clippy、18 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。清理本批 2.2 GiB Cargo 与约 37 MiB 站点产物。Address 的坐标及其余地区规则继续跟踪。

## Faker 经纬度和地区坐标

对照固定 FakerPHP 1.24.1 Address 及 hu_HU、me_ME、zh_TW 覆盖。latitude、longitude 复用六位精度的 randomFloat，保留反向边界，非有限输入返回错误。Rust 用 Option 表达省略边界，用可序列化 Coordinates 表达带字段的坐标。成对坐标沿用匈牙利和黑山的上游地区范围，台湾地区范围由独立方法提供，成对入口保持上游默认范围。五个入口各自维护 unique，坐标值的正负零视作相同结果。v0.x 未发现等价 Faker 生成器，Goravel 装配未涉及。 [固定 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Address.php#L134)

隔离基线 `dc92addb`。PHP 8.5.12 固定 FakerPHP 及 Rust 对四种 locale 验证坐标边界和六位小数，Rust 补验无效输入及去重耗尽。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 把匈牙利和台湾坐标写入数值列并原样回查。Rust 1.94.0 格式、工作区全目标严格 Clippy、19 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。已清理本批 2.2 GiB Cargo 与约 37 MiB 站点产物。

## 工厂软删除状态

固定 Laravel Factory 的动态 trashed 检查模型是否支持软删除，把模型自定义删除列追加到状态链，接受显式日期，省略日期时使用 Carbon 的前一日。Rust 以 trashed(Option<Value>) 表达可选日期，对不支持的模型返回 DatabaseError。默认状态在注册时捕获参考时刻，生成时以所选连接的应用时区计算前一日并沿用模型日期格式。复用旧版已移植的日历运算、模型元数据和日期转换。原有状态和序列的公开签名保留，之后追加的状态与显式属性可覆盖该值。Goravel 启动装配未涉及。 [Laravel Trashed State](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#trashed-state) · [Laravel 工厂测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L760)

隔离基线 `91fe3dd9`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 验证自定义删除列、批量默认日期、显式日期、后续清空和查询作用域。SQLite 与 MySQL 回归补验未保存模型不落库及不支持软删除的错误。Rust 1.94.0 格式、全目标严格 Clippy、20 项 Seeder 测试、2 项工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 与约 37 MiB 站点产物。

## 工厂条件链

对照固定 Laravel Factory 的 Conditionable 和条件回调测试，单模型与批量工厂提供 when、unless、whenElse、unlessElse。布尔条件在配置时决定分支，选中的闭包接收并返回工厂，之后继续按声明顺序合并状态。Rust 使用显式布尔值和第三个参数的 Else 方法表达默认分支，动态真值、可变回调返回类型及 HigherOrderWhenProxy 由显式 Rust 表达式替代。起步 API 与现有图片构建器的 when、unless 一致。v0.x 未发现模型工厂的对应实现，Goravel 装配未涉及。 [Laravel Conditionable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Conditionable/Traits/Conditionable.php) · [Laravel 工厂条件测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L744)

隔离基线 `5d81ba91`。PHP 8.5.12 执行固定 Conditionable 验证分支及备用回调。MySQL 8.4.13 实际 `artisan db:seed --database=mysql --force` 验证状态覆盖顺序和两条批量记录，双库回归覆盖 raw、make、create 及跳过分支。Rust 1.94.0 格式、全目标严格 Clippy、21 项 Seeder、2 项工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。

## 模型与命名工厂绑定

对照固定 Laravel 的 HasFactory、UseFactory、UseModel 与 configure。生成工厂实现 FactoryDefinition，使用关联类型绑定模型，每个新工厂自动执行配置钩子。既有 HasFactory::factory 显式选择命名工厂，后续状态与批次数量仍由调用方链式设置。Rust 使用已编译的模块和关联类型表达 PHP 的类发现及选定模型，不引入全局名称推断。v0.x 的查询工厂没有对应实现，Goravel 的编译型装配作为参考。 [Laravel 工厂发现约定](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#factory-and-model-discovery-conventions) · [Laravel HasFactory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/HasFactory.php)

隔离基线 `ec764c4a`。实际 Artisan 生成 BindingProbe 模型和工厂，在隔离项目声明模块、显式绑定并重新构建。MySQL 8.4.13 `artisan db:seed --database=mysql --force` 查得一条配置默认值和两条调用处状态覆盖记录。SQLite 回归验证调用之间的工厂状态隔离。Rust 1.94.0 格式、全目标严格 Clippy、22 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。

## 关系工厂的父模型上下文

对照固定 Laravel Factory 的状态闭包和 afterCreating、Relationship 与 BelongsToManyRelationship 的子模型创建。直接和多态子关系及生成型 hasAttached 传入当前父模型，构造回调只接收子模型。Rust 的 stateWithParent 和 afterCreatingWithParent 通过显式模型类型提供可选借用，独立生成或类型不匹配得到 None。原有回调形状、状态顺序和外键补写保留。v0.x 未发现等价工厂回调，Goravel 装配未涉及。 [Laravel 工厂关系状态](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#has-many-relationships) · [Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php)

隔离基线 `540470e2`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 验证父模型身份、直接外键、附件中间表及独立创建回退。SQLite 和 MySQL 双库回归覆盖同一契约和类型不匹配。首轮既有多态 Seeder 夹具出现 Debug 栈溢出，批量 createOn 的内部 Future 改在堆上后默认栈复验通过。Rust 1.94.0 格式、全目标严格 Clippy、23 项 Seeder、2 项工厂回归、Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过。清理本批 2.2 GiB Cargo 和约 37 MiB 站点产物。

## 关系展开默认值与恢复复验

对照固定 Laravel Factory 的 `expandRelationshipsByDefault`、`dontExpandRelationshipsByDefault`、`flushState` 和 `withoutParents`。默认允许展开。在 Rust 中所有模型类型共享一个原子默认值，工厂创建时读取一次。禁用后，隐式父工厂属性解析为 null，即使池中有可复用模型。显式外键和已有模型关联保留。整组抑制后再传单项选择器，仍保持整组抑制。v0.x 未发现等价入口，Goravel 装配未涉及。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php)

隔离基线 `0938c8ff`。Rust 1.94.0 格式和工作区全目标严格 Clippy 通过。MySQL 8.4.13 和 SQLite 共通过 24 项 Seeder 与 2 项工厂回归，包含 `makeOne`、`makeMany`、`lazy` 与跨模型类型的展开默认值。实际应用二进制两次执行 `artisan db:seed --database=mysql --force`。一轮验证预览和零数量不写入、延迟任务执行前零写入及重复执行后四条记录，另一轮验证默认值快照、显式外键和已有模型关联。MySQL 无符号主键按数据库读回值校验，两库共有断言。另通过 `./artisan inspire`、三语示例编译、VitePress 构建及根路径和 `/docs/` 下六项浏览器检查。

验收后清理本批隔离副本的 Cargo 构建产物 2.2 GiB 和站点产物约 37 MiB。复查项目磁盘可用 87 GiB。05-04 继续跟踪显式外键覆盖 `for` 时的延迟解析。

## 父关系外键按需解析

固定 Laravel 的 `BelongsToRelationship` 先登记外键解析闭包，多态类型是独立默认值。显式状态或调用属性覆盖外键时，连同 `null` 一起按属性值处理，旧闭包不执行。同一外键最后登记的父关系覆盖前项，也覆盖工厂定义。Rust 通过类型化描述符识别这些列，沿用每次批量操作的父模型缓存。待解析外键在 Rust 的状态字典中省略，Laravel 状态闭包可以看到 PHP 的占位闭包。手写 `FactoryParent` 实现可提供列元数据来参与此优先级，默认继续使用现有的关联契约。v0.x 未发现等价实现，Goravel 装配未涉及。 [Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L552) · [Laravel BelongsToRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToRelationship.php#L49)

隔离基线 `d4f34329`。实际应用二进制在 MySQL 8.4.13 执行 `artisan db:seed --database=mysql --force`。实测状态及调用属性覆盖、重复父关系登记、保留多态类型默认值、批量只写一条上级记录并保存四条子记录。MySQL 与 SQLite 回归还断言覆盖后没有错误的关系缓存。Rust 1.94.0 格式、全目标严格 Clippy、25 项 Seeder、2 项工厂回归、`./artisan inspire`、三语示例编译、VitePress 和根路径与 `/docs/` 下 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物与约 37 MiB 站点产物，磁盘可用 87 GiB。后续仍需核对固定 Laravel 的公开回收模型查询入口，以及构建器多次调用间的父关系缓存生命周期。

## 回收模型查询与父关系缓存

固定 Laravel 的 `getRandomRecycledModel` 按模型类随机选取，`BelongsToRelationship` 在关系对象上保存首次成功解析的父键。Rust 的单条和批量构建器使用 `getRandomRecycledModel::<Owner>() -> Option<&Owner>`，按编译期模型类型从已有持久化模型池借用。公开查询按类型选取。实际关联保留连接筛选，并将成功解析的模型按子连接和上级连接的查询源组合缓存到关系声明。这样可沿用类型化关联校验、支持多数据库以及合并并发初始化。失败保留重试能力。显式覆盖和隐式 `parentAttribute` 的独立解析行为保留。固定 Goravel 的 Factory 契约提供 Count、Make、Create 和 CreateQuietly，v0.x 未发现等价回收模型接口。 [Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L800) · [Laravel BelongsToRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToRelationship.php#L68) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `2f1a0a51`。实际应用二进制 `artisan db:seed --database=mysql --force` 在 MySQL 8.4.13 和隔离 SQLite 连接验证首次失败后重试、raw 与 make 并发只建一条上级记录、构建器转换为批次后沿用父键、连接切换及类型化回收查询。已有的多态 Seeder 验证两次 create 使用同一父键。Rust 1.94.0 格式、工作区全目标严格 Clippy、26 项 Seeder、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物及约 37 MiB 站点产物，磁盘可用 87 GiB。05-04 后续核对模型元数据入口和固定 Laravel 工厂创建快捷方法。

## 工厂创建快捷入口与模型元数据

固定 Laravel 的 `new` 先登记状态，再调用 `configure`。`times` 在配置后的新构建器上设定数量，`newModel` 直接构造模型。Rust 的 `FactoryDefinition` 用 `new(attributes)` 和 `newWithState(callback)` 表达 Laravel 同一参数接受属性或闭包的行为，`times(usize)` 返回类型化批次。单条与批量构建器的 `modelName()` 返回静态绑定类型名，`newModel(attributes)` 复用模型批量赋值规则，保留已有 `Factory::new(definition)` 及模型 `factory()` 入口。固定 Goravel 的 Factory 以 Count 构造批次，旧版未发现等价命名模型工厂接口。 [Laravel Factory 创建入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L214) · [Laravel 模型构造](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L949) · [固定 Goravel 工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `0dbdc71b`。MySQL 8.4.13 实际 `artisan db:seed --database=mysql --force` 验证初始属性和闭包先于配置、调用状态可覆盖、直接构造不执行工厂定义并遵守 fillable、零数量无写入以及两条批次落库回查。Rust 1.94.0 格式、工作区全目标严格 Clippy、27 项 Seeder、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 及 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物与约 37 MiB 站点产物。下一批核对固定 Laravel 的模型反查工厂入口及 `raw` 操作的临时状态参数。

## 从模型获取工厂与原始属性临时状态

固定 Laravel 的 `factoryForModel` 通过类名查找并新建工厂，Rust 用 `Factory::<Model>::factoryForModel()` 复用 `HasFactory` 的编译期绑定。运行时工厂命名空间与类名推断由显式类型绑定表达。`rawWith` 与 `rawWithState` 对应 Laravel `raw` 的属性或闭包参数。状态仅在本次调用追加，对每行执行于关系展开前。已有无参调用与选定连接的入口均保留。固定 Laravel 另有可选的父模型参数，供状态回调读取。现有公开 `raw` 系列暂无该参数。旧版和固定 Goravel 未发现等价公开入口。 [Laravel raw](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L247) · [Laravel factoryForModel](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L1026) · [Laravel 工厂测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L230) · [固定 Goravel 工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `43ae01c1`。MySQL 8.4.13 应用二进制 `artisan db:seed --database=mysql --force` 验证工厂绑定、覆盖只影响当次调用、批次闭包读取先前状态、显式 null 跳过父工厂、需要解析时只生成一个父模型、空批次跳过状态和定义。Rust 1.94.0 格式、工作区全目标严格 Clippy、28 项 Seeder、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和 6 项浏览器检查通过。

本批隔离副本清理 2.2 GiB Cargo 产物及约 38 MiB 站点产物。项目磁盘可用 87 GiB。

## 原始工厂属性的父模型上下文

固定 Laravel 的 `raw($attributes, $parent)` 把父模型交给状态链。Rust 的单条和批量构建器提供 `rawWithParent` 和 `rawWithParentOn`，复用类型化的 `stateWithParent`，本次闭包接收同一模型的借用。现有无参和临时属性入口继续表达省略父模型的调用。父模型仅作为上下文，显式外键状态仍优先于关系解析。两条内部关系创建路径继续注入其默认和强制属性。固定 Goravel 的 Factory 接口及 v0.x 未发现等价入口。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L247) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `cc29539d`。实际应用二进制使用 MySQL 8.4.13 执行 `artisan db:seed --database=mysql --force`，验证单条、批量、上下文无隐式外键和显式外键覆盖不生成额外父记录，随后的普通调用独立解析父模型。Rust 1.94.0 格式、全目标严格 Clippy、29 项 Seeder、2 项 MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译、VitePress 和 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物与约 38 MiB 站点产物。后续核对 `make` 与 `insert` 的同类父模型参数。

## 构造和批量插入的父模型上下文

固定 Laravel 的 `make($attributes, $parent)` 把父模型交给状态链，`insert` 复用同一构造路径。Rust 用类型化借用提供 `makeWithParent`、`insertWithParent` 及显式连接变体，单条和批量构建器均可调用。父模型作为状态上下文，属性覆盖仍在关系键展开前应用。插入继续复用已有批量持久化路径。固定 Goravel 的 Factory 提供 Make 与 Count，v0.x 未发现对应父模型参数。 [Laravel make](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L428) · [Laravel insert](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L493) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `c72f95bd`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 验证单条与批量预览零写入、零条插入、三条正常插入回查、afterMaking 属性和显式外键覆盖。回归覆盖后续无父模型调用以及插入跳过 afterCreating。Rust 1.94.0 格式、全目标严格 Clippy、30 项 Seeder、2 项 MySQL／SQLite 工厂回归、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物与约 38 MiB 站点产物。后续按工厂文档和验收清单复核整个 05-04，记录尚未覆盖的行为。

## 05-04 文档覆盖复核与附加数量推断

| 固定 Laravel 章节 | OpenRust 实现与验收记录 | 后续边界 |
| --- | --- | --- |
| 定义工厂、状态、回调 | 编译期模型绑定、Artisan 生成、状态合并及回调阶段，已实测配置顺序与实例隔离 | PHP 类名推断由显式绑定表达。FakerPHP 的其余 formatter 和地区行为仍需逐项核对 |
| 构造、持久化、序列 | make、raw、lazy、create、insert、静默创建以及序列有双库与实际 Seeder 证据 | Rust 以单条和批量构建器表达结果形状 |
| 工厂关系 | 一对多、反向、多对多、多态、外键展开和回收池已经有数据库证据。本批补验中间表序列的数量推断 | PHP 动态关系方法由 Rust 描述符和命名入口表达 |
| Seeder 中使用工厂 | 三语 Seeder 示例传递选定连接。既有实际命令覆盖子 Seeder、选定数据库、生产确认和失败退出 | 固定 Seeder 文档中的整棵调用链静默事件入口单独复核，工厂 createQuietly 已验收 |

固定 Laravel 在 `hasAttached` 收到非空中间表属性列表和未计数工厂时使用列表长度。Rust 直接接收 `Factory<R>` 表达未计数状态，显式 `FactoryBatch<R>` 保留其数量。空列表或共享映射默认生成一条。命名关系和类型化关系共用注册规则。v0.x 未发现对应工厂实现，Goravel 的 Factory 契约无附加关系入口。 [Laravel 中间表属性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#pivot-table-attributes) · [Laravel hasAttached](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L685) · [Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L578) · [Laravel Seeder](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md#using-model-factories) · [固定 Goravel](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `0bf1b7a2`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 回查未计数序列、显式三条、显式零条、空序列及共享属性，在五种场景中合计七条关联记录。Rust 1.94.0 格式、全目标严格 Clippy、31 项 Seeder、2 项 MySQL／SQLite 工厂回归、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查通过。

清理本批隔离副本 2.2 GiB Cargo 产物与约 38 MiB 站点产物。05-04 保留未闭合状态，下一批核对 Seeder 的整棵调用链静默模型事件。

## Seeder 调用链静默模型事件

固定 Laravel 的 `Seeder::__invoke` 识别 `WithoutModelEvents`，并在 `Model::withoutEvents` 范围内执行。OpenRust 在编译期注册用 `SeederEntry::withoutModelEvents()` 选择同一行为，沿用从 v0.x 移植的任务局部 Eloquent 事件静默机制。普通子 Seeder 继承当前调用链范围，返回或失败后恢复。固定 Goravel 的注册、调用和签名组织提供装配参考。 [Laravel Seeder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Seeder.php#L166) · [Laravel WithoutModelEvents](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Seeds/WithoutModelEvents.php) · [Laravel 静默模型事件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md#muting-model-events) · [固定 Goravel Seeder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/seeder/seeder.go)

隔离基线 `29cbef89`。真实应用二进制执行 MySQL 8.4.13 `artisan db:seed --database=mysql --force`，确认子 Seeder 在静默范围内写入两条记录，成功和失败返回后同一进程的监听器重新取消普通写入。MySQL／SQLite Artisan 回归还验证未处理错误的退出码 1、错误文本和失败前写入。通过 32 项 Seeder、2 项双库工厂回归、格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查。

本批隔离清理 2.2 GiB Cargo 构建产物和约 38 MiB 站点产物。05-04 仍跟踪 FakerPHP 生成器与地区行为差异。

## Faker 条码与 ISBN

对照固定的 Barcode provider 及 Ean、Isbn 计算器。接入 `ean8`、`ean13`、`isbn10` 和 `isbn13`，复用现有数字采样与 unique 历史。Rust 按字节计算十进制校验位并保留字符串输出。ISBN-10 的校验值 10 写为 X，ISBN-13 使用 978 或 979 前缀。按上游格式验证输出，随机序列由各语言的生成器决定。固定 Goravel 的 Factory 契约提供编译型生成入口，v0.x 未发现对应格式化器。 [固定 Barcode](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Barcode.php) · [固定 EAN](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Calculator/Ean.php) · [固定 ISBN](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Calculator/Isbn.php) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

PHP 8.5 执行固定 provider 和校验器，四种格式各 128 条通过。Rust 以独立校验公式验证样本，并逐格式验证 128 条唯一值。隔离基线 `aa7d03df` 的实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查四种格式及带前导零的样本。Rust 1.94.0 格式、全目标严格 Clippy、33 项 Seeder、2 项 MySQL／SQLite 工厂回归、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查通过。

清理本批隔离构建和站点产物。05-04 仍跟踪其他生成器和地区差异、应用默认 Seeder 与工厂模块接线，以及工厂跨次状态生命周期等未闭合项。

## 默认用户工厂与应用 Seeder

对照固定 Laravel 应用骨架的 `UserFactory`、`User` 和 `DatabaseSeeder`。应用编译 `database/factories`，在用户工厂模块显式实现 `HasFactory`。默认 Seeder 通过选定连接创建已验证的 Test User，并以 `withoutModelEvents` 注册。用户工厂保留随机姓名、安全邮箱、十位字母数字令牌与未验证状态。密码由模型的配置化 hashed cast 处理。Laravel 工厂在 PHP 进程内缓存默认密码的哈希，Rust 复用模型写入路径，每次创建时按当前应用的哈希配置处理。重复填充按邮箱唯一约束失败。v0.x 未找到相同的默认用户工厂和 Seeder。Goravel 的固定应用骨架提供迁移，编译注册参照其 Seeder 合约。 [固定 Laravel UserFactory](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/database/factories/UserFactory.php) · [固定 Laravel DatabaseSeeder](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/database/seeders/DatabaseSeeder.php) · [固定 Goravel Seeder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/seeder/seeder.go)

隔离基线 `9092a042`。实际应用二进制在空 SQLite 和 MySQL 8.4.13 执行 `artisan migrate --force` 与 `artisan db:seed --database … --force`，回查示例用户、验证时间和令牌，验证 bcrypt 可校验密码。两库重复填充退出码为 1，用户记录仍为一条。另通过 `migrate --seed`、34 项 Seeder 测试、2 项 MySQL／SQLite 工厂回归、格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查。

本批隔离构建与站点产物已清理。后续继续核对公开工厂创建入口、跨次状态和其余生成器的差异。

## 工厂与中间表序列的跨次索引

对照固定 Laravel `Sequence`、`CrossJoinSequence` 和 `BelongsToManyRelationship`。索引属于序列实例，在同一构建器的不同操作和父模型之间接续。Rust 使用独立原子计数，保留共享借用调用方式，并发时按实际状态执行顺序分配。新构建器从零开始。笛卡尔积仍按索引计算组合。旧版未发现对应工厂序列，Goravel 的工厂契约提供批次 Count，沿用现有编译装配。 [固定 Laravel Sequence](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Sequence.php#L55) · [固定 Laravel CrossJoinSequence](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/CrossJoinSequence.php) · [固定 Laravel 工厂序列文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#sequences) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `41f22255`。PHP 8.5 执行固定 Sequence 确认索引跨调用接续与新实例归零。MySQL 8.4.13 实际 `artisan db:seed --database=mysql --force` 验证 raw、make、insert、create 接续及两个父模型的附加顺序，回查六条关联模型记录。35 项 Seeder、2 项 MySQL／SQLite 工厂回归、格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 及 6 项浏览器检查通过。

已清理本批隔离构建与站点产物。下一批核对 `makeMany` 消耗默认数量后的状态生命周期。

## 多条操作消耗工厂默认数量

固定 Laravel 的 `makeMany` 与 `createMany` 先取默认数量，再清除构建器的数量设置。显式记录参数也会清除该设置，静默创建沿用同一流程。Rust 批量构建器使用原子操作领取并重置默认数量，保留共享借用与固定的集合返回类型。后续操作默认处理一条。显式零条或模型生成失败均消耗默认数量。v0.x 未发现对应入口。固定 Goravel 通过 Count 设定批量数量，编译装配沿用现有路径。 [Laravel makeMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L469) · [Laravel createMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L290) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `aff5761a`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 验证默认三条被领取后降为一条、显式零条及失败后的重置、make 不落库、静默创建三条与后续单条创建。通过 36 项 Seeder、2 项 MySQL／SQLite 工厂回归、格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查。

已清理本批隔离构建及站点产物。下一批核对 `create`、`createQuietly` 和 `lazy` 的公开父模型上下文。

## 创建、静默创建和延迟创建的父模型上下文

固定 Laravel 的 create、createQuietly 和 lazy 将可选父模型交给状态与 afterCreating。Rust 单条与批量构建器用 `createWithParent`、`createQuietlyWithParent` 和 `lazyWithParent` 传递类型化借用。即时创建可显式指定连接，延迟回调在执行时解析连接，并沿用已有静默事件和回调阶段。传入父模型本身不填外键。固定 Goravel 的工厂提供 Make、Create、CreateQuietly 和 Count，v0.x 未发现等价父模型入口。 [Laravel create 与 lazy](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L325) · [固定 Goravel Factory](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `ebfa2a31`。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 注册能够取消写入的模型监听器，验收状态和回调读取父模型、静默与恢复、延迟等待前零写入、重复延迟创建及九条未关联外键的子记录。37 项 Seeder、2 项 MySQL／SQLite 工厂回归、格式、全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查通过。

已清理本批隔离构建与站点产物。后续核对 `insert` 的属性转换和批量写入行为。

## 工厂批量插入的属性投影

固定 Laravel Factory::insert 在 make 阶段后清空隐藏列表和 appends，通过 attributesToArray 投影，然后用无全局作用域的 fillAndInsert 重新应用写入转换。Rust 复用模型输出属性的访问器及序列化转换，保留可见白名单，跳过追加字段，然后走同一 fillAndInsert。投影在 JSON 编码之前输出数据库类型，保留二进制参数等 Rust 存储值。原有批量语句、ID、时间戳与静默职责沿用插入构建器。v0.x 没有等价的 Factory insert 或 fillAndInsert，固定 Goravel 的工厂契约主要提供 Make 与 Create。 [Laravel Factory::insert](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L493) · [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) · [固定 Goravel](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 与独立 MySQL 定向回归确认访问器与修改器顺序、隐藏列落库、追加字段跳过、JSON 和日期回查，以及重复唯一键导致整批零写入。Rust 1.94.0 格式与全目标严格 Clippy、Artisan inspire、三语示例编译、VitePress 和 6 项浏览器检查通过。测试按项目约定使用独立 MySQL 实例。

合入同时提交的 Sanctum 基线 `b4d23daf` 后复验格式、严格 Clippy、MySQL 定向测试和实际 Seeder。清理本批独立构建与站点产物。后续继续核对其余 Faker 生成器和地区行为。

## Faker 银行标识

对应固定 Laravel 的模型工厂数据生成，OpenRust 的 `fake()` 增加 `iban` 和 `swiftBicNumber`，并分别维护 `unique` 历史。对照 FakerPHP v1.24.1 Payment provider，迁移 63 个国家的账号格式，保留前缀、校验位、未收录代码的 24 位数字回退和 BIC 可选分行后缀。Rust 使用类型化可选参数，对非两字母国家代码、非大写 ASCII 字母或数字的前缀、超过 30 字符的账号部分返回错误。固定 Goravel 保留现有编译工厂装配，v0.x 未找到对应方法。 [Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [FakerPHP Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Payment.php#L241)

PHP 8.5 执行固定 provider 和校验器，校验 63 种格式各 8 条 IBAN、128 条 BIC，并将 PHP 格式表与 Rust 数据逐项比较。Rust 回归覆盖模 97、前导零、自定义与默认长度、非法参数和唯一历史。独立 MySQL 8.4.13 用实际二进制运行 `artisan db:seed --database=mysql --force`，回查德国、英国及未收录国别的三条账号与 BIC。Rust 1.94.0 格式及全目标严格 Clippy、39 项 Seeder 测试、MySQL 与 SQLite 两项工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

下一批核对 Payment provider 的信用卡 formatter，其他地区特有的银行方法单独跟踪。

已清理本批隔离副本的 2.2 GiB Cargo 产物与约 38 MiB 文档站产物，复查项目卷剩余 84 GiB。

## Faker 信用卡样本

对应固定 Laravel 工厂的 `fake()`。移植 FakerPHP v1.24.1 Payment provider 的 `creditCardType`、`creditCardNumber`、`creditCardExpirationDate`、`creditCardExpirationDateString` 和 `creditCardDetails`，包括通用与巴西葡语的品牌权重、卡号格式和 Luhn 校验、默认分隔符、到期日窗口及每个 formatter 的 unique 历史。繁中沿用通用 Payment provider，与上游委托一致。Rust 用结构体表达详情，JSON 字段保留 `type`、`number`、`name`、`expirationDate`。未知品牌返回类型化错误，自定义日期格式复用现有 Chrono 指令和时区规则。v0.x 未找到对应方法，固定 Goravel 的工厂装配沿用现有路径。 [Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [FakerPHP Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Payment.php#L147) · [巴西葡语 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_BR/Payment.php)

隔离基线 `1d712dfe`。PHP 8.5 执行固定通用、巴西葡语及繁中 provider，验证号码格式、Luhn、日期和详情。两套品牌权重与 61 条卡号格式逐项比对一致。Rust 定向回归验证分组、品牌范围、两种日期窗口、错误参数和唯一品牌耗尽。真实 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 写入并回查完整卡片样本、到期字符串和巴西 Elo 格式化卡号。Rust 1.94.0 格式、全目标严格 Clippy、MySQL 定向工厂回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。日常数据库验收按项目约定限定 MySQL。

下一批按地区核对 `bankAccountNumber`、`bankRoutingNumber` 及其他 Payment provider 特有接口。

清理本批独立 Cargo 产物 2.2 GiB 和站点产物约 38 MiB，复查项目卷剩余 83 GiB。

## Faker 地区账号与路由号

对应固定 Laravel 模型工厂的 `fake()`。FakerPHP v1.24.1 有 45 个地区提供 `bankAccountNumber`。Rust 复用 IBAN 生成器，保留 41 个地区别名的默认国家和可覆盖参数，以及埃及固定 IBAN 和美国、马来西亚、尼泊尔的本地格式。美国地区另提供 `bankRoutingNumber` 与公开的 `calculateRoutingNumberChecksum`。两种生成方法各有独立 `unique` 历史。Rust 使用统一签名，对无参地区传入非默认参数、缺少 formatter 的地区和不合法的校验输入返回错误。旧版未找到对应方法，工厂装配沿用当前框架路径。 [Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [FakerPHP 美国 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Payment.php) · [FakerPHP 英国 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_GB/Payment.php)

隔离基线 `e8940a08`。PHP 8.5 从固定源码检查 45 个地区各 32 条账号及 32 条美国路由号。Rust 定向回归覆盖每个账号地区、参数覆盖、两种校验位、唯一历史和错误路径。实际 MySQL 8.4.13 用 `artisan db:seed --database=mysql --force` 回查美国账号和路由号、带前导零前缀的英国 IBAN 和尼泊尔账号。Rust 1.94.0 格式、全目标严格 Clippy、MySQL 定向工厂回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。

下一批核对地区银行名称、分类及本地 SWIFT 接口。地区税号另行按其 formatter 契约验收。

已清理本批独占 Cargo 产物 2.2 GiB 和文档站产物约 38 MiB，项目卷剩余 81 GiB。

## Faker 地区金融机构与 SWIFT

对应固定 Laravel 模型工厂的 `fake()`。迁移 FakerPHP v1.24.1 的 11 个地区银行名称、马来西亚的银行分类、保险和 `currencySymbol`，以及尼泊尔的商业银行、开发银行、金融公司、小额信贷和数字钱包。马来西亚与尼泊尔的 `swiftCode` 使用各自的固定目录，马来西亚 `bank` 保留先随机选分类的机制。Rust 对缺少 formatter 的地区返回错误，各方法沿用独立的 `unique` 历史。v0.x 未发现对应方法，现有 Goravel 式编译装配保持适用。 [Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [马来西亚 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Payment.php) · [尼泊尔 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ne_NP/Payment.php)

隔离基线 `5633e19e`。PHP 8.5 对照 12 个地区的上游 formatter，每个方法抽样 32 次。1,487 条目录项按顺序逐项比对一致。Rust 定向回归覆盖目录和唯一值耗尽、地区别名、地区互斥、小额信贷后缀及 SWIFT 格式。实际 MySQL 8.4.13 用 `artisan db:seed --database=mysql --force` 回查简中银行、马来西亚外资银行、尼泊尔小额信贷与两个地区 SWIFT。Rust 1.94.0 格式、全目标严格 Clippy、MySQL 工厂回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。VitePress 仍报告分块大小提示。

下一批核对固定 Payment provider 的地区 VAT formatter。

已清理本批独占 Cargo 产物 2.2 GiB 和文档站产物约 38 MiB，项目卷剩余 81 GiB。

## Faker 地区 VAT 样本

对照固定 Laravel 模型工厂的 `fake()`，补齐 FakerPHP v1.24.1 所有九个 `vat` 地区，包括 Payment 的保加利亚、奥地利、西班牙、比利时法语及荷语、法国，以及 Company 的英国、意大利和荷兰。保留不同地区的默认分组、校验计算和抽样规则。法国复用 `siren`，意大利和荷兰保留 `vatId` 和 `btw`。Rust 以 `vat(Option<bool>)` 表达默认空格选项，以 `vatWithType` 表达英国分支、政府和医疗类型，公开校验助手返回数字，并对非法输入返回错误。生成入口具有各自的 `unique` 历史，英国类型入口与普通 VAT 共用历史。v0.x 未发现对应 formatter，固定 Goravel 的编译式工厂装配沿用现有路径。 [Laravel 工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [FakerPHP 法国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_FR/Payment.php) · [FakerPHP 英国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_GB/Company.php)

隔离基线 `ce744b02`。PHP 8.5 对照九个上游 provider，每地区至少 64 条。Rust 回归覆盖空格与紧凑格式、英国类型、法国和意大利的 Luhn、法国和比利时的模 97、别名、unique 与错误路径。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 原样回查法国、比利时、英国分支和意大利样本。Rust 1.94.0 格式、全目标严格 Clippy、MySQL 定向工厂回归、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

Payment provider 的公开生成器已验收。下一批回到 05-04 交付清单，核对模型工厂、Seeder 和文档覆盖，继续记录其他 Faker provider 的差异。

验收期间主分支合入 `f6cfc23e`，其差异仅为 Sanctum 验收文档。本批独占 Cargo 产物 2.2 GiB 和文档站产物约 38 MiB 已清理，项目卷剩余 83 GiB。

## 05-04 复核与延迟属性展开

对照固定 Laravel 文档和工厂测试。工厂生成、模型绑定、状态与回调、构造和持久化、序列、普通和多态关系及复用池都有实现和验收。Seeder 的选定连接、子调用、静默模型事件及失败退出有实际 Artisan 记录。固定测试另外覆盖了属性闭包读取已展开值，以及闭包返回关联工厂，这两条此前只有状态闭包及静态 `parentAttribute`。

新增 `attribute` 和 `parentAttributeWith`。状态与显式覆盖先合并，登记的延迟定义再按顺序求值。由前一个定义产生的值可供后续定义使用。关联工厂沿用连接选择、回收模型、主键和排除规则。Rust 将延迟回调登记在工厂构建器上，数据库 `Attributes` 仍只承载值。v0.x 未找到等价接口，固定 Goravel 的工厂装配方式保持适用。 [Laravel Factory::expandAttributes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L591) · [Laravel 回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L167)

隔离基线 `38fe8c46`。MySQL 8.4.13 定向回归与实际 `artisan db:seed --database=mysql --force` 核对显式覆盖阻止关联写入、按依赖计算、父模型持久化、批量覆盖、排除和错误边界。Rust 1.94.0 格式及全目标严格 Clippy、实际多态 Seeder 和插入投影回归、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。

下一批继续核对固定测试中模型值直接展开、关系触碰与关系缓存行为。PHP 的运行时宏与其余 Faker provider 差异继续跟踪。

隔离 Cargo 目标目录与约 38 MiB 站点产物已清理，项目卷剩余 83 GiB。

## 工厂模型主键与关系缓存生命周期

固定 Laravel 的属性展开把模型值转成主键，工厂保存后会移除已缓存的空关系集合，再创建子关系。Rust 以 `modelAttribute(name, &model)` 捕获主键，也可表达未赋主键的 null，显式覆盖沿用现有规则。工厂顶层和嵌套构造共用类型化空集合检测，保留非空集合和单值关系，包括已缓存的 null。关系加载保持异步；多对多写入继续走现有 Pivot 触碰路径。v0.x 没有找到对应工厂入口，Goravel 式编译装配适用。 [Laravel 工厂保存](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L376) · [Laravel 属性展开](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L591)

隔离基线 `40da2c1f`。MySQL 8.4.13 定向回归与实际 `artisan db:seed --database=mysql --force` 验证已有模型主键转换、未保存模型的 null、预构造保留空缓存、保存后的空缓存清理和重新查询两条子记录。多态 Seeder 回归、Rust 1.94.0 格式及严格 Clippy、Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过。

固定 Laravel 针对触碰后的多对多关系读取仍需专项验收，包括自定义 Pivot 和命名关系。该差异继续保留在 05-04。

已清理本批隔离构建和站点产物，项目卷剩余 83 GiB。

## 命名工厂关系的 Pivot 触碰与缓存

对照固定 Laravel 对触碰模型的多对多工厂用例。OpenRust 的 Pivot 写入沿用关系描述符的触碰判定和事务路径。现在 `hasAttachedNamed` 在附加完成后重新加载已缓存的命名关系，或加载触碰了关联端的命名关系。按原名称和描述符重新查询，保留 Pivot 属性、自定义 Pivot 类型和既有筛选。刷新结果在 `afterCreating` 中可读取。Rust 关系由异步查询加载，匿名 `hasAttached` 通过其描述符按需读取。v0.x 未发现对应命名工厂入口。 [固定 Laravel 工厂测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L473)

隔离基线 `e44ad4c9`。独立 MySQL 8.4.13 定向回归和真实 `artisan db:seed --database=mysql --force` 覆盖已有和生成模型、已有旧缓存与首次加载、成员时间戳、未关联模型的时间戳、自定义 Pivot 类型和逐条属性。Rust 1.94.0 格式、全目标严格 Clippy、多态 Seeder 回归、`./artisan inspire`、三语示例编译、VitePress 构建及六项浏览器检查通过。

本批隔离编译与站点产物已清理。后续核对固定工厂测试中的运行时宏扩展与模型发现的 Rust 对应机制。

## 嵌套工厂模型发现与扩展

固定 Laravel 的工厂和模型发现保留嵌套命名空间，`make:factory` 支持 `-m`。Rust 修复默认推断丢失嵌套路径的问题，并接通 `-m` 显式覆盖。模块声明后由 `FactoryDefinition::Model` 和 `HasFactory` 编译时绑定。Laravel 的运行时宏调用用可导入的 Rust 扩展 trait 表达，适用于工厂构建器及命名状态。该机制允许编译器检查模型、方法、参数与返回类型。固定 Goravel 沿用类型显式的工厂契约，v0.x 未发现等价工厂生成器。 [Laravel 生成命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Factories/FactoryMakeCommand.php) · [Laravel 嵌套发现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L713) · [Laravel 工厂宏](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L807) · [固定 Goravel](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)

隔离基线 `b4d185c4`。两个实际 Artisan 回归覆盖斜杠、反斜杠、默认导入、`-m`、重复生成、`--force`、空模型名和发布模板。通过真实 `make:model Fleet/Flight` 与 `make:factory Fleet/FlightFactory` 生成代码，声明模块并编译，再以 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 验证嵌套模型绑定及扩展 trait 生成的 `scheduled` 状态。Rust 1.94.0 格式、严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。

已清理本批隔离编译与站点产物。后续核对 05-04 的工厂功能闭合状态与 Faker 差异。

## 05-04 交付清单复验

| 固定 Laravel 章节 | 本轮结论 | 对应验收 |
| --- | --- | --- |
| 定义模型工厂、状态和回调 | 嵌套生成、编译期模型绑定、扩展 trait、状态与回调有验收。PHP 动态发现和宏由显式 Rust 模块与类型表达。 | 本轮真实 `make:factory` 回归和前批生成源码编译，见[嵌套工厂](#嵌套工厂模型发现与扩展) |
| 使用工厂创建模型 | 预构造、持久化、序列、延迟计算和批量插入已有行为证据。Rust 用返回类型区分单个模型和集合。 | 本轮 MySQL 复验延迟属性，见[延迟属性](#05-04-复核与延迟属性展开) |
| 工厂关系 | 多态填充、命名多对多、自定义 Pivot 和触碰后缓存通过复验。 | 本轮定向多态 Seeder 与命名 Pivot 回归 |
| Seeder 使用工厂与静默事件 | 默认用户实际迁移和填充通过，二次填充因重复账号退出 1，仍只有一条记录。选定连接和子 Seeder 已复验。 | 本轮默认与选定连接 MySQL Artisan 用例，静默范围及失败后的恢复见[静默模型事件](#seeder-调用链静默模型事件) |
| Faker 生成数据 | 地区和核心格式已有显式实现及证据。固定 Laravel 的 `fake()` 按应用和语言缓存生成器，当前独立调用新建 Faker，工厂在单次构造批次内共用。跨调用 `unique` 仍有差异。 | [固定 Laravel fake 助手](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/helpers.php) |

隔离基线 `8ce37bf4`。将默认 Seeder 回归拆成 MySQL 和 SQLite 独立入口，以便日常验收锁定 MySQL，缺少必需 URL 时失败。本轮用 MySQL 8.4.13 跑五项定向测试，覆盖默认 Seeder 的 `migrate`、`db:seed`、哈希和重复执行，选定连接、关系填充、Pivot 缓存和延迟属性。另通过 Rust 1.94.0 格式、全目标严格 Clippy、嵌套工厂生成回归、`./artisan inspire`、六个三语示例的编译、VitePress 和两种挂载路径下的六项导航回归。保留 SQLite 回归供发布验收。

05-04 继续开放。下一批优先核对 Faker 按应用和规范化语言复用的生命周期及跨调用唯一值，分别验证同作用域复用、不同语言和不同应用隔离。目录差异继续跟踪 File、Image、UserAgent、Uuid、Color、Medical、HtmlLorem、Text、Biased、修饰器和其余地区 formatter。05-05 已有独立闭合记录，本次不重复计入。

本批独占 Cargo 产物 2.2 GiB 与文档构建产物已清理，项目卷剩余 82 GiB。

## 按应用与语言复用 Faker

固定 Laravel 的 `fake()` 以语言为容器单例键，工厂通过独立的数据库服务绑定取得生成器。OpenRust 用容器单例保留这两个入口，按规范化语言复用，语言别名共用同一历史。工厂以数据库连接所属应用为准。使用 `Arc<Faker>` 保持共享生成器的所有权，时区覆盖可以通过共享句柄读写。应用之外仍可创建无共享历史的生成器。与 PHP 数据库 Provider 的进程静态缓存相比，Rust 把工厂生成器限定在应用容器，支持同一进程的多应用隔离。v0.x 未发现 Faker 生命周期实现，Goravel 的工厂装配方式无需调整。 [Laravel fake 助手](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/helpers.php#L475) · [Laravel 数据库 Provider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/DatabaseServiceProvider.php)

隔离基线 `b96b9a07`。MySQL 8.4.13 定向回归在同一进程运行两个独立应用的 Artisan Seeder，验证别名共用、不同语言隔离、助手与工厂绑定隔离、工厂跨调用耗尽及落库行数。多态 Seeder 和应用默认 Seeder 回归验证地区选择、共享时区覆盖、历史重置和默认用户。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器导航检查通过。

实际应用二进制的 MySQL Seeder 再验收一次，确认语言别名共享去重，第二次构建失败后仍只写入一条记录。已清理本批独占编译和站点产物。05-04 继续跟踪其余 Faker provider。

## Faker 文件样本与 UUID

固定 Laravel 工厂通过 `fake()` 使用 FakerPHP provider。本批迁移 File 的 461 个 MIME 分组与全部扩展名，保留先选分组再选扩展名的抽样权重。新增 FakerPHP 兼容的版本 3 UUID，复用现有 MD5、UUID 类型及随机数。文件入口对两个目录显式取路径，复制顶层可读文件、保留扩展名并按选项返回目标路径或文件名。Rust 使用 `PathBuf` 和 I/O 错误，检测路径别名并以独占创建避免覆盖。v0.x 未发现对应 Faker 入口，Goravel 的工厂装配无需变动。 [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) · [FakerPHP File](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/File.php) · [FakerPHP UUID](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Uuid.php)

隔离基线 `ee0b9250`。PHP 8.5 运行固定 provider 校验抽样、复制和 UUID，再逐项比对 461 个分组。Rust 回归验证 MIME 唯一值耗尽、非递归、二进制内容、完整路径或文件名、扩展名及无效目录。真实 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 复制 JSON 后回查文件名、MIME 与扩展名。默认 Seeder 回归、Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 及双挂载的六项浏览器检查通过。

已清理本批独占编译与文档构建产物。后续按 05-04 记录继续核对 Image 与 UserAgent。

## Faker 占位图片

对照固定 FakerPHP v1.24.1 Image provider，保留域名、默认尺寸、格式常量、背景和表单文字编码，以及 HTTP 失败返回无结果的行为。Rust 用 `ImageOptions` 表达默认参数，`imageUrl` 支持 `unique`，异步下载返回 `io::Result<Option<PathBuf>>`。远端响应完整后才发布本地文件。本地错误和非法格式保留错误信息。随机词复用现有 Rust Faker 词表。v0.x 未发现对应 provider，编译式工厂装配沿用当前路径。 [固定 FakerPHP Image](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Image.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md)

隔离基线 `0daefc6e`。PHP 8.5 对照 URL、格式列表及错误输入。Rust 两项回归验证 URL 编码、去重、完整响应、404 和截断响应后的文件清理。实际 MySQL 8.4.13 `artisan db:seed --database=mysql --force` 下载远端 PNG，读取并验证 16×12 尺寸，落库回查 URL 与字节数。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与双挂载的六项浏览器检查通过。

后续继续核对 UserAgent。

隔离 Cargo 产物和文档构建产物已清理，项目卷剩余 82 GiB。

## Faker 浏览器 User-Agent

对照固定 FakerPHP v1.24.1，接通随机 User-Agent、六种浏览器及六种平台或处理器标识。保留各平台的抽样权重、历史版本范围和格式，包括 Edge 的构建号拼接、iOS 版本和 Safari 的移动端标识。Firefox 在应用时区抽取 2010 年以来的 Gecko 日期。Rust 复用已有日期采样和 `unique` 历史，日期范围错误通过 `DatabaseResult` 返回，其余格式直接返回值。各候选平台只生成被抽中的分支。v0.x 未发现相应生成器，Goravel 的编译式工厂装配沿用当前路径。 [固定 FakerPHP UserAgent](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/UserAgent.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `28a735a7`。PHP 8.5 运行固定 provider，按格式和范围对照 13 个入口的 3,328 条样本。两项 Rust 回归覆盖浏览器格式、版本日期、平台标识、唯一值耗尽与重置。真实 MySQL 8.4.13 运行 `main artisan db:seed --database=mysql --force`，逐项写入并回查 13 个 formatter 样本。49 项 Seeder 与 2 项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。

已恢复隔离副本的原始 Seeder，并清理 2.2 GiB Cargo 和 38 MiB 站点产物。磁盘剩余 82 GiB。05-04 继续核对 Color、Medical、HtmlLorem、Text、Biased、修饰器和地区 formatter。

## Faker 颜色样本

对照固定 FakerPHP v1.24.1 Color provider，接通十个 formatter。保留普通十六进制与 RGB 的非零范围、`safeHexColor` 的 256 个配对十六进制输出、RGBA 的一位小数舍入与 HSL 的整数范围。颜色名称按标准化语言选择上游词表，继承和默认回退保留原列表及重复项的抽样权重。Rust 用定长整数数组表达 RGB 和 HSL，通过整数拆分 RGB 通道，全部入口复用 `unique`。v0.x 未发现相应生成器，Goravel 工厂装配沿用现有路径。 [固定 FakerPHP Color](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Color.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `f59ee745`。PHP 8.5 执行固定 provider，逐项比对 22 组默认及地区词表和 2,560 条样本的格式与范围。两项 Rust 回归覆盖范围、简繁中文和法语等名称、回退及唯一值耗尽和重置。真实 MySQL 8.4.13 运行 `main artisan db:seed --database=mysql --force`，回查十种格式和繁体中文名称。49 项 Seeder、两项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式与工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。

已恢复隔离副本原始 Seeder，清理 2.2 GiB Cargo 与 38 MiB 站点产物，磁盘剩余 82 GiB。05-04 下一批核对 Medical，再继续 HtmlLorem、Text、Biased、修饰器及其余地区 formatter。

## Faker 血型样本

对照固定 FakerPHP v1.24.1，Generator 的三个公开方法经 BloodExtension 使用 Core Blood，Medical provider 的对应方法拥有相同取值。OpenRust 接通 `bloodType`、`bloodRh` 和 `bloodGroup`，按上游独立采样 ABO 和 Rh 并直接拼接。单项返回静态字符串，组合返回 String，三个入口共用已有 `unique` 机制并分别记录历史。v0.x 未发现对应生成器，Goravel 工厂装配沿用现有路径。 [FakerPHP Blood](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/Blood.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `02b07d7d`。PHP 8.5 对固定上游三个入口各取样 512 次，覆盖全部四种 ABO、两种 Rh 和八种组合。Rust 回归核对英文、简体与繁体中文的取值、去重耗尽、独立历史及重置。运行实际 `main artisan db:seed --database=mysql --force` 后，在 MySQL 8.4.13 回查全部 14 条样本。49 项 Seeder 测试、两项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 及双挂载六项浏览器检查通过。

隔离副本已恢复原始 Seeder，清理 2.2 GiB Cargo 与 38 MiB 站点产物，磁盘剩余 82 GiB。下一批核对 HtmlLorem，复用现有 Lorem 和安全域名入口。

## Faker HTML 样本

对照固定 FakerPHP v1.24.1 HtmlLorem。接通 `randomHtml`，保留默认深度和宽度、标题、登录表单、子树选择规则、叶节点类型、表格尺寸以及单文档内数字 ID 去重。Rust 通过 `Option<i32>` 表达默认参数，复用 Lorem 和安全域名，用现有转义工具写入 HTML。数字 ID 采样的重试耗尽通过 `DatabaseResult` 返回，`unique` 单独记录完整文档。输出结构和范围跟随上游，文本来自现有 Rust Lorem。v0.x 未发现对应入口，Goravel 的工厂装配沿用现有路径。 [固定 FakerPHP HtmlLorem](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/HtmlLorem.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `aa718050`。PHP 8.5 的 DOM 对照验证七组深度与宽度参数共 448 份样本，覆盖表单、叶节点、表格、列表和 ID。Rust 回归验证文档可解析、表单、默认及边界参数、ID 唯一性和完整文档去重。MySQL 8.4.13 的多态工厂 Seeder 回归通过。实际 `main artisan db:seed --database=mysql --force` 写入精简及默认深度 HTML 并完整回查。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。

隔离副本已恢复原始 Seeder，并清理约 2.2 GiB Cargo 与 38 MiB 站点产物，磁盘剩余 82 GiB。下一批继续核对 Text 的 Markov 文本与地区语料。

## Faker 地区语料与 Markov 文本

对照固定 FakerPHP v1.24.1 Text provider，接通 `realText`、`realTextBetween` 及各自的 `unique` 历史。保留 30 个地区映射、26 份不同语料、英语回退、1 至 5 个词或字符的上下文、按出现次数加权的后继采样及约定的重试上限。繁体中文、日语、捷克语和波斯语保留各自的处理规则。Rust 按语料和上下文长度惰性共享不可变索引，使用 UTF-8 Unicode 字符数模拟启用 mbstring 的基线。波斯语 `realText` 沿用字节计数，继承的大写起点规则在 `realTextBetween` 无法找到候选时由 Rust 有界结束并返回错误。语料来源及许可随数据保留。v0.x 未发现等价入口，Goravel 式工厂装配沿用现有路径。 [固定 FakerPHP Text](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Text.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `cb376ae9`。PHP 8.5 校验 30 个地区映射的语料 SHA-256 和 450 条样本。Rust 两项回归覆盖各地区名称、字符与字节边界、默认和无效参数、去重、捷克语和波斯语差异。MySQL 8.4.13 多态 Seeder 回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回查英文、简体回退、繁体、日语和波斯语共五条样本。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。

隔离副本已恢复原始 Seeder，清理 2.2 GiB Cargo 与 38 MiB 站点产物，磁盘剩余 82 GiB。下一批核对 Biased 与生成器修饰器。

## Faker 偏置取样与修饰器

对照固定 FakerPHP v1.24.1 的 Biased、ChanceGenerator、ValidGenerator 与 UniqueGenerator。接通 `biasedNumberBetween` 的默认平方根和自定义接受曲线，使用整数运算映射到闭区间，覆盖完整 i64 范围。Rust 将随机分数约束在 [0, 1)，避免上游浮点端点可能越界，并为反向范围、非法概率和取样耗尽返回错误。 `optional` 保留百分制兼容与延迟调用，`valid` 和 `uniqueWithRetries` 保留原重试边界。Rust 用显式类型化闭包替代 PHP 的动态方法转发。每个 UniqueFaker 句柄可配置次数，历史继续由当前 Faker 共享。v0.x 未发现对应实现，Goravel 工厂装配沿用现有路径。 [FakerPHP Biased](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Biased.php) · [FakerPHP Generator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php)

隔离基线 `c675f09e`。PHP 8.5 验证 1,536 条偏置样本与三种修饰器边界。Rust 两项回归覆盖曲线方向、上下界、错误传播、默认值、重试耗尽和重置。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回查偏置整数、默认值和通过验证的样本，并在真实 Seeder 中验证唯一值耗尽。多态 Seeder、Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与双挂载六项浏览器检查通过。

恢复隔离副本原始 Seeder，清理 2.2 GiB Cargo 与 38 MiB 站点产物，磁盘剩余 82 GiB。后续按 FakerPHP provider 面逐项对照地区 formatter，核对 05-04 的余项。

## Faker 地区身份编号

对照固定 FakerPHP v1.24.1 Person provider，接通 `en_US` 的 `ssn`、`en_GB` 的 `nino` 和 `zh_TW` 的 `personalIdentityNumber`。保留美国区号的两段等权抽样、英国前缀及后缀规则、台湾地区字母表、七位序号和校验位。Rust 复用规范化语言及 `unique`，未接入的地区返回明确错误。当前 `ssn` 与 `personalIdentityNumber` 的其他地区实现仍待逐个对照。v0.x 未发现对应方法，Goravel 编译式工厂装配沿用现有路径。 [美国 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Person.php) · [英国 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_GB/Person.php) · [台湾 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Person.php)

隔离基线 `d85de5cf`。PHP 8.5 对照 1,024 条固定上游样本。Rust 回归覆盖取值范围、受限前缀、性别位、校验位、语言别名、错误与去重。MySQL 8.4.13 用实际二进制执行 `artisan db:seed --database=mysql --force`，回查美国和英国各一条与台湾两个性别位样本。多态 Seeder、Rust 1.94.0 格式与全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。

隔离副本原始 Seeder 已恢复，清理 2.2 GiB Cargo 和 38 MiB 站点产物，磁盘剩余 82 GiB。下一批继续核对地区 Person 的共有方法与签名差异。

## Faker 地区 SSN

对照固定 FakerPHP v1.24.1 Person provider，补齐泰国、奥地利、冰岛的 `ssn`。泰国沿用与美国相同的两段区号抽样。奥地利和冰岛保留最近 100 年生日采样、模 11 校验以及校验值为 10 时重抽序号的规则。奥地利的 PHP 可选生日参数由 Rust 的 `ssnWithBirthdate(Option<NaiveDate>)` 表达，并与 `ssn` 共用去重历史。复用既有日期生成与时区处理。旧版没有找到对应实现，编译式工厂接线沿用现有路径。 [泰国 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/th_TH/Person.php) · [奥地利 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_AT/Person.php) · [冰岛 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/is_IS/Person.php)

隔离基线 `60a1af47`。PHP 8.5 校验 1,024 条样本，Rust 两项地区回归覆盖闰日、日期、校验位、范围与唯一值。MySQL 8.4.13 多态 Seeder 和实际 `main artisan db:seed --database=mysql --force` 通过，后者写入并回查四条 SSN。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项双挂载浏览器检查通过。已恢复隔离副本的原始 Seeder，清理 2.2 GiB 编译产物和 39 MiB 站点产物，磁盘剩余 82 GiB。其余地区 `personalIdentityNumber` 继续跟踪。

## Faker 欧洲身份编号

对照固定 FakerPHP v1.24.1 Person，补齐立陶宛、拉脱维亚、芬兰、挪威、瑞典和波兰的 `personalIdentityNumber`。保留地区日期顺序、世纪码、性别位、序号和校验规则。瑞典复用现有 Luhn，波兰使用身份证格式，挪威沿用上游随机末两位。Rust 用显式的 `WithBirthdate` 和 `WithSerial` 表达 PHP 的可选参数，保留无参生日采样和共享去重历史。立陶宛要求由三个数字组成的序号前缀和一位世纪码。v0.x 未发现相应实现，工厂接线沿用现有路径。 [固定 FakerPHP 地区 Provider](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)

隔离基线 `8dee8ce2`。PHP 8.5 校验 768 条样本及固定闰日序号 `0079`。三项 Rust 地区回归覆盖日期、性别、校验位、错误和重试。MySQL 8.4.13 多态 Seeder 与实际 `main artisan db:seed --database=mysql --force` 通过，后者写入并回查七条编号。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和双挂载六项浏览器检查通过。已恢复隔离副本原始 Seeder，清理 2.2 GiB Cargo 和 39 MiB 站点产物，磁盘剩余 82 GiB。地区专用编号入口继续核对。

## Faker 波兰 PESEL 与 NIP

对照固定 FakerPHP v1.24.1 波兰 Person provider，接通 `pesel` 和 `taxpayerIdentificationNumber`。保留 PESEL 五组世纪月份编码、缺省奇数性别位、固定或采样生日及模 10 校验。NIP 保留三个非零前置数字与模 11 重试。Rust 用 `Option<NaiveDate>` 表达可选生日，对 1800–2299 年之外的日期返回错误，避免复用世纪码产生歧义。两者共用现有日期、数字掩码和 `unique` 基础设施。v0.x 未找到对应方法，工厂接线沿用现有路径。 [固定波兰 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/Person.php) · [Laravel 模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)

隔离基线 `c78431be`。PHP 8.5 校验 1,920 条固定上游样本。Rust 四项地区身份回归、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过。真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回查指定闰日和默认日期的 PESEL 以及 NIP。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 构建和六项浏览器检查通过。现有需单独设置清库 URL 的 MySQL fresh 用例未启用。隔离副本恢复原始 Seeder，清理 2.2 GiB 编译产物及 39 MiB 站点产物，磁盘剩余 82 GiB。其他地区的专用编号方法继续核对。

## Faker 巴西 CPF、RG 与 CNPJ

对照固定 FakerPHP v1.24.1 巴西 Person 与 Company，接通 `cpf`、`rg`、`cnpj`。沿用数字掩码、圣保罗 RG 数字校验位、CNPJ 默认分支 0001 与共用模 11 校验。Rust 用 `Option<bool>` 表达默认标点或纯数字格式，各方法支持 `unique`。v0.x 未找到等价入口，工厂接线沿用现有路径。 [固定巴西 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_BR/Person.php) · [固定巴西 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_BR/Company.php)

隔离基线 `0bf9cc02`。PHP 8.5 校验 1,536 条固定上游样本。Rust 五项地区身份回归、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回查三个入口的带标点与纯数字共六条样本。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。单独清库 URL 未启用。已恢复隔离副本原始 Seeder，清理 2.2 GiB Cargo 和 39 MiB 站点产物，磁盘剩余 82 GiB。下一批继续核对西语地区的 DNI 与相关编号方法。

## Faker 西班牙和秘鲁 DNI、秘鲁 RUC 与驾照类别

对照固定 FakerPHP v1.24.1 Person 与 Company。西班牙 DNI 使用八位数字与模 23 校验字母。秘鲁 DNI 保留八位数字，RUC 的 `10` 和 `20` 前缀后保留九位随机数字。 `licenceCode` 接通西班牙和南非两个地区的固定类别。所有方法支持 `unique`，Rust 以 `Option<bool>` 表达 RUC 的默认参数。v0.x 未找到对应实现，编译式工厂接线沿用现有路径。 [西班牙 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_ES/Person.php) · [秘鲁 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_PE/Person.php) · [秘鲁 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_PE/Company.php) · [南非 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_ZA/Person.php)

隔离基线 `f7a2b9c7`。PHP 8.5 校验 1,536 条固定上游样本，覆盖格式、校验字母、前缀和完整驾照类别。Rust 六项地区身份回归、49 项 Seeder 测试和两项 MySQL／SQLite 工厂回归通过。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回查六条样本。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。单独清库 URL 未启用。恢复原 Seeder 后清理 2.2 GiB Cargo 和 39 MiB 站点产物，磁盘剩余 82 GiB。地区 `idNumber` 和其他专用编号继续核对。

## Faker 南非、荷兰、沙特、蒙古与埃及地区编号

对照固定 FakerPHP v1.24.1 Person 和埃及 Address。接通南非、荷兰、沙特、蒙古的 `idNumber`，以及沙特和埃及的 `nationalIdNumber`、沙特的 `foreignerIdNumber`、蒙古字母与埃及省份编号。南非保留出生日期、公民位、性别区间、双侧序号补零与 Luhn。荷兰保留模 11 及余数为 10 时的修正，沙特保留前缀与 Luhn。埃及保留日期范围、登记奇偶和随机末位。Rust 使用显式有参方法表达 PHP 可选参数，与无参方法共用去重历史。v0.x 未找到等价入口，Goravel 式编译装配沿用现有路径。 [固定 FakerPHP 地区实现](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)

隔离基线 `d48d9892`。PHP 8.5 校验 1,792 条上游样本。Rust 七项地区回归、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回查七条样本，含蒙古西里尔字母。Rust 1.94.0 格式与全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。需要独立清库 URL 的 MySQL fresh 用例未启用。已恢复原 Seeder，清理 2.2 GiB Cargo 和 39 MiB 站点产物，磁盘剩余 82 GiB。其余地区专用编号继续对照。

## Faker 土耳其身份编号与立陶宛、拉脱维亚护照

对照固定 FakerPHP v1.24.1 Person。立陶宛护照为八位数字，拉脱维亚为两个小写字母加七位数字。土耳其 `tcNo` 保留上游九位随机前缀和带符号的余数，包括前导零以及可能产生负号的后缀。 `tcNoChecksum` 对非九位 ASCII 数字回报错误，`tcNoIsValid` 沿用上游对两字符后缀的比较行为。两个生成方法支持 `unique`。v0.x 未找到等价入口，工厂接线沿用现有路径。 [土耳其 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/tr_TR/Person.php) · [立陶宛 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lt_LT/Person.php) · [拉脱维亚 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lv_LV/Person.php)

隔离基线 `c8b60fb8`。PHP 8.5 对照 1,536 条样本及正、负余数固定输入。Rust 八项地区身份回归、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回查三条样本。Rust 1.94.0 格式、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。需独立清库 URL 的 MySQL fresh 用例未启用。恢复隔离副本原 Seeder，清理 2.2 GiB Cargo 和 39 MiB 站点产物，磁盘剩余 82 GiB。其余地区公司与税务编号继续核对。

## Faker 美国 EIN 与俄罗斯 INN、KPP

对应固定 Laravel [Eloquent Factories — Defining Model Factories](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，按 FakerPHP v1.24.1 的 [美国 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Company.php) 和[俄罗斯 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ru_RU/Company.php) 实现。EIN 保留 83 个前缀及七位数字后缀，INN 保留地区范围、模 11 再模 10 校验和旧名 `inn`，KPP 保留四位前缀和 `01001`。四个格式支持 `unique`。Rust 通过可选参数表达缺省值，要求显式地区编号和校验前缀采用 ASCII 数字，验证接口对格式无效的号码返回 false。公开格式各自保留去重历史。v0.x 未找到等价入口，Goravel 式编译装配沿用原入口。

隔离基线 `3e1d2fc5`。PHP 8.5 生成并核对 2,048 条上游样本，逐项比对全部 83 个 EIN 前缀。Rust 九项地区回归、49 项 Seeder、两项 MySQL／SQLite 工厂回归通过，MySQL fresh 单独提供清库 URL 实际执行。MySQL 8.4.13 通过实际二进制 `main artisan db:seed --database=mysql --force` 写入并回查 EIN、INN、旧名 INN 和 KPP 四条记录。Rust 1.94.0 格式及全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 及双挂载六项浏览器检查通过。站点保留既有分块大小提示。恢复原 Seeder，Cargo 清理输出为 2.2 GiB，另清理 39 MiB 站点产物。提交前周额度剩余 50%，项目卷可用 81 GiB。后续核对法国 SIRET 和波兰 REGON。

## Faker 法国 SIRET 与波兰 REGON

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 FakerPHP v1.24.1 的 [法国 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_FR/Company.php) 与[波兰 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/Company.php) 实现。SIRET 复用 SIREN 和 Luhn，保留三种机构序号掩码、默认分组以及纯数字格式。Rust 以 `Option<bool>` 表达 PHP 可选格式参数。REGON 保留奇数地区码和模 11 校验，`regonLocal` 复用有效的九位前缀并按本地登记号权重计算末位。三个方法独立维护 `unique` 历史。v0.x 未找到对应入口。现有编译式装配继续调用同一个 Faker。

隔离基线 `53f8edc7`。PHP 8.5 生成并核对 2,048 条固定上游样本。Rust 1.94.0 格式、工作区全目标严格 Clippy、10 项地区回归、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过，MySQL fresh 已在独立临时数据库另行执行。MySQL 8.4.13 通过实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查四条样本，覆盖 SIRET 两种输出和 REGON 两种长度。Artisan inspire、三语示例编译、VitePress 和根路径及 `/docs/` 下六项浏览器检查通过，保留已有分块大小提示。下一批核对捷克 IČO、丹麦 CVR 与 P 编号及冰岛 VSK。

已恢复临时 Seeder，清理本批 2.2 GiB Cargo 产物及 39 MiB 站点产物。提交前周额度剩余 50%，项目卷可用 81 GiB。

## Faker 捷克 IČO、丹麦 CVR 与 P 编号、冰岛 VSK

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [捷克 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/cs_CZ/Company.php)、[丹麦 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/da_DK/Company.php) 与[冰岛 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/is_IS/Company.php)。IČO 复用数字掩码与加权求和，保留余数为 0、1、10 的特殊映射。丹麦 CVR 与 P 编号及冰岛 VSK 分别保留 8、10、5 位非零开头的数字掩码。Rust 接口保留上游命名，按地区拒绝不适用的 formatter，四个方法均支持独立的 `unique` 历史。v0.x 未找到等价实现，Goravel 式装配继续调用既有 Faker。

隔离基线 `5ddb1d38`。PHP 8.5 核对 2,048 条固定上游样本，覆盖 IČO 全部 11 种余数。Rust 1.94.0 格式、工作区全目标严格 Clippy、11 项地区测试、49 项 Seeder 和两项 MySQL／SQLite 工厂回归通过，独立临时 MySQL 实例实际执行 fresh 用例。MySQL 8.4.13 通过实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查四条样本。Artisan inspire、三语示例编译、VitePress 和双挂载六项浏览器检查通过，保留已有分块大小提示。

已恢复临时 Seeder，清理本批 2.2 GiB Cargo 产物与 39 MiB 站点产物。提交前周额度剩余 50%，磁盘 81 GiB。下一批核对沙特与埃及企业编号。

## Faker 沙特企业编号与埃及企业税号、商业登记号

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [沙特 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_SA/Company.php) 与[埃及 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_EG/Company.php) 接通 `companyIdNumber`、`companyTaxIdNumber` 和 `companyTradeRegisterNumber`。复用数字掩码和 Luhn 校验，保留沙特 `700` 前缀、埃及前导零与三个方法的独立去重历史。Rust 返回字符串以保留前导零，并对不适用的地区返回错误。v0.x 未发现等价入口，Goravel 式装配继续使用现有 Faker。

隔离基线 `e677f2bf`。PHP 8.5 核对 1,536 条固定上游样本。Rust 1.94.0 格式、全目标严格 Clippy、12 项身份编号测试及独立 MySQL 8.4.13 工厂回归通过。用实际二进制 `main artisan db:seed --database=mysql --force` 写入并回查三条样本。Artisan inspire、三语示例编译、VitePress 与双挂载六项浏览器检查通过，站点保留已有分块大小提示。按日常验收约定，本批数据库只运行 MySQL。后续核对哈萨克斯坦 BIN 和南非企业登记号。

已恢复临时 Seeder，清理 1.7 GiB Cargo 及 39 MiB 站点产物。提交前周额度剩余 50%，磁盘 82 GiB。

## Faker 哈萨克斯坦 BIN 与南非企业登记号

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[哈萨克斯坦 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/kk_KZ/Company.php)和[南非 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_ZA/Company.php)。BIN 用 `Option<NaiveDate>` 表达可选登记日期，保留两位年份、两位月份、两位实体分类和六位随机尾号。未指定日期时，复用现有 Faker 本年日期取样。南非企业登记号从过去五十年至当前时间取样，保留六位非零开头的序号以及全部 23 种实体代码。两者支持独立的 `unique` 历史，沿用现有 Faker 日期范围和时区机制。v0.x 未找到等价实现，Goravel 式编译装配继续使用现有 Faker。

隔离基线 `50bae68f`。PHP 8.5 核对 1,536 条固定上游样本，另核对南非实体代码表和 23 种代码的样本覆盖。Rust 1.94.0 格式、全目标严格 Clippy、13 项身份编号测试及独立 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查显式日期 BIN、缺省日期 BIN 和南非登记号。Artisan inspire、三语示例编译、VitePress 和双挂载六项浏览器检查通过。站点保留已有分块大小提示。按日常验收约定，本批数据库只运行 MySQL。

临时 Seeder 已恢复，清理本批 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对台湾 VAT 及希腊税务办公室和经营类别。

## Faker 台湾 VAT 与希腊税务办公室、经营类别

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[台湾 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Company.php)和[希腊 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_GR/Company.php)。`VAT()` 保留八位整数，既有 `vat(...)` 入口支持同一地区并按其字符串契约返回，无国家前缀。Rust 两个入口的去重历史按各自方法名保存。希腊 `doy()` 和 `object()` 直接借用静态目录文本，保留全部 21 个税务办公室与 19 种经营类别，支持 `unique` 并在候选耗尽时返回错误。v0.x 未发现等价实现，编译式装配沿用现有 Faker。

隔离基线 `c9b18a9b`。PHP 8.5 核对 1,536 条上游样本，逐项比对希腊两份目录及完整采样覆盖。Rust 1.94.0 格式、全目标严格 Clippy、14 项身份编号测试和 MySQL 8.4.13 工厂回归通过。实际二进制 `main artisan db:seed --database=mysql --force` 写入并回查台湾 VAT 与两条希腊文本。Artisan inspire、三语示例编译、VitePress 和根路径及 `/docs/` 下六项浏览器检查通过，站点保留既有分块大小提示。按日常验收约定，本批数据库只运行 MySQL。

已恢复临时 Seeder，清理 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 49%，磁盘可用 82 GiB。下一批核对其余地区公司名称与后缀格式，对照现有 company() 的语言覆盖。

## Faker 美国、法国、意大利与波兰公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照固定 [FakerPHP v1.24.1 Company](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 接入美国、法国、意大利和波兰四种名称模板与后缀。保留重复模板的抽样权重。美国、法国及意大利复用框架现有的姓氏生成器，姓名词库仍有差异。波兰此前会回退英语，现改用固定上游的波兰姓氏和机构前缀。其余地区延续 fake-rs 公司名称行为。新增 `companySuffix()`，与 `company()` 各自支持 `unique`。v0.x 未发现等价入口，Goravel 式装配继续使用现有 Faker。

基线 `3d68ad4d`。PHP 8.5 从固定上游抽样 2,048 个名称和 2,048 个后缀，校验后缀覆盖、意大利必带后缀及波兰模板与姓氏。Rust 1.94.0 格式、全目标严格 Clippy、15 项 Faker 回归与 MySQL 8.4.13 工厂回归通过。用实际二进制运行 `main artisan db:seed --database=mysql --force`，写入并回查四种地区各一组名称和后缀。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过，保留已有分块大小提示。按日常验收约定本批数据库只运行 MySQL。临时 Seeder 已恢复，清理 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对东亚公司前缀、后缀与组合规则。

## Faker 东亚公司名称、前缀和后缀

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照固定 FakerPHP v1.24.1 的 [简体中文](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_CN/Company.php)、[繁体中文](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Company.php)、[日本](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ja_JP/Company.php)及[韩国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ko_KR/Company.php)模板。保留重复模板和重复词条的抽样权重、原始标点及名称连接方式。公司前缀、台湾名称修饰词和英文后缀借用静态数据并支持 `unique`，未经接入的地区返回错误。日本继续使用现有日文姓氏生成器，词表与上游仍有差异。台湾 `companyEn()` 实测在固定上游报 `Unknown format "lastNameEn"`，本批只接入可用的 `companyEnSuffix()`。v0.x 未找到等价实现，Goravel 式编译装配沿用既有 Faker。

隔离基线 `191390b2`。PHP 8.5 验证 2,048 个固定上游名称和同等数量的后缀。Rust 1.94.0 格式与全目标严格 Clippy、16 项 Faker 定向回归和 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回读四个地区的八条公司名称与前缀样本。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过，站点保留既有分块提示。日常数据库验收只运行 MySQL。

临时 Seeder 已恢复，清理本批 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对其他地区公司模板。

## Faker 埃及、约旦与沙特公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按固定 FakerPHP v1.24.1 的[埃及](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_EG/Company.php)、[约旦](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_JO/Company.php)和[沙特](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_SA/Company.php)模板生成公司名称。保留地区姓氏、埃及城市、公共前缀、地区后缀与原始空格。Rust 复用公司格式器与去重基础设施，约旦和沙特共享相同的模板和后缀。v0.x 未发现等价入口，Goravel 式装配沿用既有 Faker。

隔离基线 `e2c571f8`。PHP 8.5 对照 1,536 组固定上游名称、后缀和前缀。Rust 1.94.0 格式、全目标严格 Clippy、17 项 Faker 定向回归和 MySQL 8.4.13 工厂回归通过。实际二进制 `main artisan db:seed --database=mysql --force` 写入并回读三地共九条公司名称、前缀和后缀。Artisan inspire、三语示例编译、VitePress 与根路径及 `/docs/` 下六项浏览器检查通过。本批数据库验收只运行 MySQL，站点保留既有分块大小提示。

临时 Seeder 已恢复，清理本批 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对欧洲其余公司模板。

## Faker 德国、奥地利、瑞士和捷克公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[德国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_DE/Company.php)、[奥地利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_AT/Company.php)、[瑞士](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_CH/Company.php)和[捷克](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/cs_CZ/Company.php)实现。保留地区姓氏表、法定后缀和重复条目的抽样权重。德语地区模板允许仅含姓氏，捷克模板保留四种组合且总是带后缀。Rust 复用现有 `company()`、`companySuffix()` 及去重机制，姓名生成器的其他公开方法保持原契约。v0.x 未发现等价实现，Goravel 式编译装配沿用现有入口。

隔离基线 `a498f983`。PHP 8.5 核对 2,048 组上游公司名称与后缀，逐项核对原始姓氏和后缀表。Rust 1.94.0 格式、全目标严格 Clippy、18 项 Faker 回归和 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force`，写入并回查四个地区共八条记录。Artisan inspire、三语示例编译、VitePress 及六项浏览器检查通过。站点保留既有分块大小提示，本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 与 39 MiB 文档产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对北欧公司模板。

## Faker 北欧公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。接入 FakerPHP v1.24.1 的丹麦、冰岛、芬兰、瑞典和挪威 Company 模板及对应 Person 词表。保留重复模板的权重、地区连接词与后缀，冰岛姓氏由固定上游男性名字按相同词干规则生成。Rust 复用 `company()`、`companySuffix()` 与各自的 `unique` 历史，北欧词表随实现保存原许可。v0.x 未找到等价接口，编译式装配沿用现有路径。 [FakerPHP v1.24.1 地区实现](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)

隔离基线 `48be76cc`。PHP 8.5 核对 2,560 组固定上游名称及后缀，并逐项比较词表。Rust 1.94.0 格式、全目标严格 Clippy、19 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force`，写入并回查五个地区共十条记录。Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过。站点保留既有分块大小提示，本批数据库验收只运行 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下批核对西语和葡语地区模板。

## Faker 西语与葡语地区公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。接入 FakerPHP v1.24.1 的阿根廷、西班牙、秘鲁、委内瑞拉、巴西和葡萄牙 Company 模板及 Person 姓氏表。保留当地拼写、模板权重和连接词，西班牙与委内瑞拉提供 `companyPrefix()`。Rust 使用静态模板片段表达 PHP 占位符，保留现有 `company()`、`companySuffix()` 与 `unique` 契约。v0.x 未发现等价入口，编译式装配沿用现有入口。 [FakerPHP v1.24.1 地区实现](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)

隔离基线 `537aa9d6`。PHP 8.5 核对 3,072 组上游名称及后缀，逐项比较六地模板、姓氏、前缀和后缀。Rust 1.94.0 格式、全目标严格 Clippy、20 项 Faker 回归和 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force` 写入并回查六地共十二条名称和后缀。Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过。站点保留既有分块大小提示，本批数据库验收只运行 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对荷兰语地区公司模板与名称规则。

## Faker 荷兰与比利时荷兰语公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。接入 FakerPHP v1.24.1 的[荷兰 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_NL/Company.php)和[比利时荷兰语 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_BE/Company.php)规则。荷兰保留三种店名组合、后缀概率、姓氏分组权重及词表大小写。比利时模板保留仅姓氏及姓氏加当地后缀。Rust 复用现有公开生成器与去重机制，保留重复项权重。v0.x 未发现等价实现，Goravel 式装配保持现有编译入口。

隔离基线 `51c8fd39`。通过 PHP 8.5 反射逐项提取并核对上游词表，验证 1,024 组公司名称与后缀。Rust 1.94.0 格式、全目标严格 Clippy、21 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force`，写入并回查两地四条名称与后缀。Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过，站点保留既有分块大小提示。日常数据库验收只运行 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 49%，磁盘 82 GiB。下一批核对其他欧洲公司模板。

## Faker 克罗地亚、斯洛伐克与匈牙利公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[克罗地亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hr_HR/Company.php)、[斯洛伐克](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sk_SK/Company.php)和[匈牙利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hu_HU/Company.php)模板。保留地区姓名、后缀、克罗地亚行业前缀、斯洛伐克模板原有的 `and` 和匈牙利纯姓氏模板。Rust 沿用现有公开生成器与 `unique`，将地区词表和生成逻辑放在独立模块。v0.x 未发现等价入口，Goravel 式编译装配沿用现有路径。

隔离基线 `3b9c989d`。PHP 8.5 反射逐项核对姓名和公司词表，并验证 1,536 组固定上游名称和后缀。Rust 1.94.0 格式、全目标严格 Clippy、22 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查六条名称和后缀。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过。站点仍有既有分块大小提示，本批数据库验收只运行 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 48%，磁盘 82 GiB。下一批核对希腊、黑山等地区剩余公司格式。

## Faker 希腊与黑山公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[希腊](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_GR/Company.php)与[黑山](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/me_ME/Company.php) Company。希腊使用实际生效的继承模板，组合地区姓氏与法定后缀。黑山保留公司名称、法定类别、重复条目、尾随空格和继承的 `Ltd` 后缀。新增 `companyName()`、`companyType()` 及去重入口。Rust 直接借用静态名称和类别词表。v0.x 未发现等价入口，现有编译式工厂装配沿用原路径。

隔离基线 `6c7f92ba`。PHP 8.5 反射逐项核对词表并验证 1,024 组上游名称与后缀。Rust 1.94.0 格式、全目标严格 Clippy、23 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查六条公司数据。Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过，保留已有分块大小提示。本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 48%，磁盘 82 GiB。

### 公司格式后续验收

- 尚未逐项对照的 Company 地区有 `bn_BD`、`el_CY`、`en_GB`、`en_ZA`、`fa_IR`、`fr_BE`、`fr_CA`、`fr_CH`、`he_IL`、`hy_AM`、`id_ID`、`it_CH`、`ka_GE`、`kk_KZ`、`lt_LT`、`ms_MY`、`ru_RU`、`sl_SI`、`th_TH`、`tr_TR`、`uk_UA`。需核对固定上游实际模板、姓名词表、前后缀与抽样权重，含仅继承通用模板的地区。
- 美国、法国、意大利及日本的公司模板仍复用现有姓氏生成器。Person 姓名词表和 Internet 用户名模板的地区行为需要分别核对。
- 台湾 `companyEn()` 的上游失败已复现。继续核对固定版本的默认 provider 和可扩展 formatter 行为，再以实际运行用例确认等价入口。
