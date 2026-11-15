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

### 工厂预构造与延迟创建恢复验收

在隔离基线 `4545dda1` 复验 `1a233c5e` 和 `ebfa2a31`。固定 Laravel 的 `makeOne` 保留工厂数量，`makeMany` 取用并清空默认数量。OpenRust 的 `lazy` 返回异步回调，等待回调返回的 Future 才写入。三语示例加入逐条属性覆盖及单模型延迟创建。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L342)

Rust 1.94.0 通过工作区全目标严格 Clippy。MySQL 8.4.13 和 SQLite 通过 49 项 Seeder 及 2 项工厂回归。MySQL fresh 使用一次性容器内的验收库，并显式提供清库 URL。实际二进制执行 `artisan db:seed --database=mysql --force`，验证单条及批量预构造、逐条属性覆盖和零条预构造均未写入目标表，批次数量由两条重置为一条。单条、批量 Future 在等待前均未写入，依次等待两次批量和两次单条调用后按主键回读六条记录。随后执行三语共用示例，新增五条记录。

`./artisan inspire`、三语示例编译、VitePress 构建及根路径和 `/docs/` 下六项浏览器检查通过。VitePress 仍有既有分块大小提示。工作区格式检查在隔离副本格式化基线已有的 Sanctum 测试后通过。临时 Seeder 已恢复。清理本轮独占 Cargo 产物 2.3 GiB 和站点产物约 40 MiB，磁盘可用 80 GiB。05-04 其余 Faker 差异继续跟踪。

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

- 已核对本清单各地区的公司名称与后缀模板，以及 `jobTitle()` 的地区词表与默认回退。
- 美国、法国、意大利及日本的公司模板已通过 Person 入口复用固定版姓氏。其余地区 Person 姓名与 Internet 用户名仍逐项核对。
- 台湾 `companyEn()` 的默认错误及三种自定义姓氏模板均已通过实际 Seeder 验证。

## Faker 英国、南非与塞浦路斯公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 FakerPHP v1.24.1 [英国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_GB/Company.php)、[南非](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_ZA/Company.php)和[塞浦路斯](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_CY/Company.php)实现。英国与南非沿用有效的基类模板和 `Ltd` 后缀，分别取当地姓氏。塞浦路斯等权抽样法定后缀模板和双姓连字符模板。Rust 沿用现有公开接口和 `unique` 历史。v0.x 未发现等价入口，编译式工厂装配沿用现有路径。

隔离基线 `cad92153`。PHP 8.5 反射逐项核对词表，并验证 1,536 组固定上游名称和后缀。Rust 1.94.0 格式、全目标严格 Clippy、24 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查六条名称与后缀。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过。站点保留既有分块大小提示，本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 与 39 MiB 站点产物。周额度剩余 48%，磁盘 82 GiB。下一批核对法语地区与意大利语瑞士模板。

## Faker 比利时、加拿大和瑞士法语及瑞士意大利语公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 FakerPHP v1.24.1 [比利时法语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_BE/Company.php)、[加拿大法语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_CA/Company.php)、[瑞士法语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_CH/Company.php)和[瑞士意大利语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/it_CH/Company.php)实现。保留地区姓氏、模板重复权重及法定后缀。加拿大法语复用法国后缀表，瑞士两种语言分别保留 `Sàrl` 与 `Sarl`。Rust 沿用公开生成器和去重契约。v0.x 未发现等价入口，编译式工厂装配沿用现有路径。

隔离基线 `2f0f2725`。PHP 8.5 反射逐项核对词表，验证 2,048 组上游名称和后缀。Rust 1.94.0 格式、全目标严格 Clippy、25 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force` 写入并回查八条数据。Artisan inspire、三语示例编译、VitePress 和六项浏览器检查通过。站点保留既有分块大小提示，本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 48%，磁盘 82 GiB。下一批核对立陶宛、斯洛文尼亚和土耳其公司模板。

## Faker 立陶宛、斯洛文尼亚与土耳其公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 [立陶宛](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lt_LT/Company.php)、[斯洛文尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sl_SI/Company.php)和[土耳其](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/tr_TR/Company.php)模板。保留立陶宛法定类别前置和引号、斯洛文尼亚的个体经营分支，以及土耳其重复模板的抽样权重和 49 项经营领域。Rust 使用静态词表并新增 `companyField()` 及去重入口。v0.x 未发现等价实现，编译式工厂装配沿用现有路径。

隔离基线 `83c4cfc2`。PHP 8.5 反射提取固定词表并核对 1,536 组上游名称与后缀。Rust 1.94.0 格式、全目标严格 Clippy、26 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查七条公司数据。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过。站点仍有既有分块大小提示，本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 和 39 MiB 站点产物。提交前周额度剩余 48%，磁盘 82 GiB。下一批核对俄语、乌克兰语和哈萨克语公司格式。

## Faker 俄语、乌克兰语与哈萨克语公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[俄语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ru_RU/Company.php)、[乌克兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/uk_UA/Company.php)和[哈萨克语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/kk_KZ/Company.php) Company。保留法定前缀、词素拼接、引号、连字符和重复项的抽样权重。Rust 新增 `companyNameElement()`、`companyNameSuffix()`，并扩展 `companyName()`，返回静态词表借用并支持 `unique`。俄语和哈萨克语仍从基类继承 `companySuffix()` 的 `Ltd`。v0.x 未发现等价实现，编译式装配沿用现有路径。乌克兰 `companyUrl()` 随地区 Internet 格式继续跟踪。

隔离基线 `7082d269`。PHP 8.5 反射提取固定词表并核对 1,536 组上游名称与后缀。Rust 1.94.0 格式、全目标严格 Clippy、27 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force` 写入并回查九条样本。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过。站点保留已有分块大小提示，本批数据库验收使用 MySQL。

临时 Seeder 已恢复，清理本批独占 1.7 GiB Cargo 与 39 MiB 站点产物。提交前周额度剩余 48%，磁盘 82 GiB。下一批核对西亚公司模板。


## makeOne、makeMany 与 lazy 恢复复验

隔离基线 `b815020d`。严格 Clippy、Rust 格式、完整 Seeder 49 项和 MySQL 8.4.13／SQLite 工厂回归两项通过。使用实际应用二进制执行 MySQL `artisan migrate --database=mysql --force` 和 `artisan db:seed --database=mysql --force`。单条预构造、默认批量、逐条覆盖及零条批量不写入目标表。默认批量消耗计数后回到一条。取得 `lazy` 的 Future 仍保持零行，两次批量和两次单条调用结束后回查六行。核对 Laravel 的 [Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L342) 后，三语文档补充单条回调和重复执行的行为。

整套 Seeder 首轮发现早期 VAT 用例仍将 `zh-Hant` 视为不支持，与已提交的台湾 VAT 实现 `3d68ad4d` 冲突。将该断言更新为八位数字格式，使用 `en-US` 检查不支持的分支，随后完整重跑通过。实际 `./artisan inspire`、三语工厂示例编译、VitePress 构建和六项浏览器检查通过。

## Faker 伊朗、以色列、亚美尼亚和格鲁吉亚公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照固定 FakerPHP v1.24.1 的[伊朗](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fa_IR/Company.php)、[以色列](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/he_IL/Company.php)、[亚美尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hy_AM/Company.php)及[格鲁吉亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ka_GE/Company.php)模板。保留当地姓名、原始标点、词素拼接与模板权重。扩展伊朗 `companyField()`、新增 `contract()`，接通格鲁吉亚的前缀、名称词素和名称尾部。各入口沿用 `unique`，伊朗与格鲁吉亚的独立 `companySuffix()` 保留继承的 `Ltd`。v0.x 未发现等价实现，Goravel 式编译装配沿用现有入口。

隔离基线 `b815020d`。PHP 8.5 反射提取固定词表，核对 2,048 组上游公司名称和后缀。Rust 1.94.0 格式、全目标严格 Clippy、28 项 Faker 回归及 MySQL 8.4.13 工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force` 写入并回查十一条样本。Artisan inspire、三语示例编译、VitePress 与六项浏览器检查通过，保留已有分块大小提示。本批公司样本写入验收使用 MySQL。

隔离副本的原始 Seeder 已恢复。已清理本批独占的 2.2 GiB Cargo 构建产物及 39 MiB 站点产物。提交前可用额度窗口剩余 48%，项目卷剩余 81 GiB。后续逐项核对孟加拉国、印度尼西亚、马来西亚和泰国公司模板，Person、Internet 和台湾 companyEn 仍在验收清单跟踪。

## Faker 孟加拉国、印度尼西亚、马来西亚和泰国公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 [孟加拉国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/bn_BD/Company.php)、[印度尼西亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/id_ID/Company.php)、[马来西亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Company.php)与[泰国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/th_TH/Company.php)。接入四地模板、名称词表和继承的后缀。扩展 `companyName()` 和 `companyType()` 的地区覆盖，接通 `companyPrefix()`、马来西亚 `industry()` 和泰国 `slogan()`，沿用 `unique`。Rust 静态词表保留原顺序和重复项，模板用等权分支，保持马来西亚后缀权重和姓名拼接。v0.x 未发现等价入口，现有编译式工厂装配保持原有接线。

隔离基线 `9d5dccd6`。PHP 8.5 反射对照 3,429 条词表数据及继承后缀，2,048 组上游名称和后缀通过模板验证。Rust 1.94.0 格式、全目标严格 Clippy、29 项 Faker 回归、49 项 Seeder 与 MySQL 8.4.13／SQLite 两项工厂回归通过。实际应用二进制 `main artisan db:seed --database=mysql --force` 写入并回查十四条公司样本。另通过 `./artisan inspire`、三语示例编译、VitePress 构建和六项浏览器检查。站点保留既有分块大小提示。

隔离副本原始 Seeder 已恢复。已清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 47%，项目卷剩余 81 GiB。

## Faker 职业名称与 Company provider 回退

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 FakerPHP v1.24.1 的 [Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Company.php) 和 [Factory](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Factory.php) 实现 `jobTitle()`。十个独立职业词表保留顺序、重复与拼写。法国词表由比利时、加拿大及瑞士的法语 Company provider 继承。拥有 Company provider 且继承基础模板的 41 个地区使用固定 Lorem 单词词表。缺少 Company provider 的地区回退到美国词表。Rust 用静态分派实现三条路径，使用现有 `unique` 历史及耗尽错误。v0.x 未发现等价实现，编译式工厂接线沿用现有路径。

隔离基线 `1df60f3b`。PHP 8.5 逐项核对 4,917 条职业数据和 249 个通用单词，并执行 4,864 次上游取样验证。Rust 1.94.0 格式与全目标严格 Clippy、30 项 Faker 回归、49 项 Seeder 和 MySQL 8.4.13／SQLite 两项工厂回归通过。实际应用二进制执行 `main artisan db:seed --database=mysql --force`，从 12 个地区分别生成并回查两条不重复的职业名称，覆盖三条加载路径。实际 `./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。站点保留已有分块大小提示。

下一批前置核对同时复现台湾 `companyEn()` 在默认 provider 下报 `Unknown format "lastNameEn"`。在同一固定生成器新增自定义姓氏 provider 后，128 次调用覆盖“姓氏和后缀、连字符双姓、逗号及 and 三姓”三种模板。Rust 入口和 formatter 扩展仍待实现。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%，项目卷剩余 81 GiB。

## Faker 台湾英文公司名称

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[台湾 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Company.php)和[Generator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php)。PHP 8.5 默认调用返回缺少 `lastNameEn` 的错误，注册英文姓氏 formatter 后 128 次取样覆盖三种等权模板。Rust 的 `companyEn()` 保留默认错误，`companyEnWithLastName()` 用调用时传入的可失败闭包提供姓氏。每个占位符分别调用，沿用六个后缀与 `unique` 历史。v0.x 未找到等价实现，编译式工厂装配保持原有接线。

隔离基线 `c492aece`。Rust 1.94.0 格式、全目标严格 Clippy、31 项 Faker 回归、49 项 Seeder 与 MySQL 8.4.13／SQLite 两项工厂回归通过。实际 MySQL `main artisan db:seed --database=mysql --force` 验证默认调用报缺少 formatter 并以 1 退出，提供姓氏后写入并回读固定姓氏可生成的全部八种结果。实际 `./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。站点保留已有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%，项目卷剩余 81 GiB。下一批先核对地区 Person 姓名生成，随后验证依赖姓名的 Internet 用户名模板。

## Faker 美国、法国、意大利和日本姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [美国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Person.php)、[法国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_FR/Person.php)、[意大利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/it_IT/Person.php)与[日本](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ja_JP/Person.php) Person。接入 1,025 条固定姓氏，保留原顺序、重复项和拼写。复用已核对的名字、称谓与全名模板，将这四地无参 `name()`、`firstName()` 接到与显式性别入口相同的采样路径。公司名称继续通过 `lastName()` 取样。Rust 保留无参方法和显式性别方法为两个签名，并共用对应的去重历史。v0.x 未找到等价实现，沿用原有编译式工厂接线。

隔离基线 `86452513`。PHP 8.5 比对 1,025 条姓氏及已有全名模板，并运行 2,048 组上游姓名、名字、姓氏及称谓样本。Rust 1.94.0 格式、全目标严格 Clippy、32 项 Faker 测试、49 项 Seeder 和 MySQL 8.4.13／SQLite 两项工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读四地共二十条 Person 和 Company 样本。实际 `./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。站点保留已有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%，项目卷剩余 81 GiB。下一批核对其余已接入 Person 地区，含荷兰姓氏分类权重，然后核对 Internet 用户名。

## Faker 九地姓名及荷兰姓氏权重

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [Person provider](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 及[荷兰 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_NL/Person.php)。接入沙特、德国、伊朗、荷兰、巴西、葡萄牙、土耳其、简体中文和繁体中文地区的无参 `name()`、`firstName()`、`lastName()`，沿用已核对的名字、全名模板和称谓。荷兰复用 Company 现有四个词表和 1:10:5:10 分类权重。台湾名字保留一或两个字符的等权抽样。Rust 保留显式性别入口及共享去重历史。v0.x 未发现等价实现，启动与编译式工厂接线保持原有路径。

隔离基线 `e4b549b2`。PHP 8.5 比对 3,432 条姓氏及现有姓名模板，验证 4,608 组上游样本。Rust 1.94.0 格式、全目标严格 Clippy、33 项 Faker、49 项 Seeder 和 MySQL 8.4.13／SQLite 两项工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读九地 45 条姓名及公司名称。实际 `./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。站点保留已有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%，项目卷剩余 81 GiB。现有十三个地区 Person 词表已接入公开姓名入口。后续核对 Internet 用户名，以及尚未接入公开姓名入口的其他 Person 地区。

## Faker Internet 地区用户名与邮箱本地部分

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php) 与地区 provider。沙特、约旦、埃及、伊朗、韩国和日本使用固定罗马字姓名词表，沙特和约旦共用相同词表。立陶宛保留六个等权模板以及同一性别分组的姓名组合。简体中文复用固定 Lorem 词表并保留下划线模板。保留邮件入口共享的用户名生成器和各 formatter 的去重历史。对照 PHP 无 intl 的繁体中文回退时，复现空音译抛错。Rust 沿用现有音译与字母回退，以保持 `userName()`、`safeEmail()` 和 `freeEmail()` 的无错误签名。其他通用地区仍使用现有 Person 入口及 Rust 音译。v0.x 未发现等价实现，工厂装配沿用现有编译路径。

隔离基线 `36bf953d`。PHP 8.5 核对 5,120 次调用，其中 4,867 次成功及 253 次繁体中文无 intl 错误，逐项比对地区词表。Rust 1.94.0 格式、全目标严格 Clippy、34 项 Faker 回归、49 项 Seeder 和 MySQL 8.4.13／SQLite 两项工厂回归通过。应用二进制 `main artisan db:seed --database=mysql --force` 写入并回读九地共 36 条用户名和邮箱。实际 `./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。站点保留已有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%、项目卷剩余 81 GiB。后续核对其余地区 Person、Internet 域名和邮件 provider 差异。

## Faker Internet 域名后缀与地区邮箱

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 FakerPHP v1.24.1 [Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php) 核对 51 个地区 provider 与默认 provider 的 TLD 和免费邮件域名，保留词表顺序和重复权重。沙特、约旦、埃及、伊朗、韩国和日本的 `domainName()` 复用用户名模块的罗马字姓氏词表。`domainWord()` 继续使用 Person 姓氏与 Rust 音译。一般邮箱采用上游的两种等权来源，简体中文沿用免费邮箱模板，尼泊尔采用六个模板位，比例为 2:1:3，其中三位给公司域名追加 `.np`。v0.x 未发现等价实现，工厂装配保持现有编译路径。

隔离基线 `e5aba484`。PHP 8.5 逐项核对 52 对词表，运行 6,656 组上游样本。无 intl 环境下记录到 384 次域名与 185 次邮箱调用错误，Rust 通用路径使用现有音译。Rust 1.94.0 格式、全目标严格 Clippy、35 项 Faker、49 项 Seeder 和 MySQL 8.4.13／SQLite 两项工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读九地 45 条域名及邮箱。实际 `./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。站点保留已有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 47%、项目卷剩余 81 GiB。后续核对尼泊尔 URL 模板、乌克兰 companyUrl 与其余地区 Person。

## Faker 尼泊尔 URL 与乌克兰公司标签

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。比对 FakerPHP v1.24.1 的[尼泊尔 Internet](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ne_NP/Internet.php)和[乌克兰 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/uk_UA/Company.php)。尼泊尔 `url()` 保留 11 个模板位和重复权重，按模板组合 HTTP、www、.np、路径和 .html。Rust 复用现有域名和 slug 入口。乌克兰 `companyUrl()` 原样返回西里尔字母公司名或附加连字符与后缀的标签。两个入口支持 `unique`，其他地区调用 `companyUrl()` 返回不支持地区的错误。v0.x 未发现等价实现，工厂装配沿用编译式入口。

隔离基线 `8c2c1d63`。PHP 8.5 核对 1,024 组样本，覆盖尼泊尔八种输出形态和乌克兰两种标签模板。Rust 1.94.0 格式、全目标严格 Clippy、36 项 Faker 测试、49 项 Seeder 与 MySQL 8.4.13／SQLite 两项工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读 24 条 URL 和公司标签。实际 `./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。站点保留既有分块大小提示。

隔离副本原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 与站点产物 39 MiB，提交前额度剩余 47%、项目卷剩余 81 GiB。后续核对其余地区 Person 姓名及工厂单元的剩余差异。

## Faker 北欧 Person 姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 [Person](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)，接入丹麦、冰岛、芬兰、瑞典和挪威书面语姓名。丹麦保留中间名及重复模板权重。冰岛保留独立父名词干和性别后缀。芬兰保留可选称谓。瑞典和挪威书面语使用实际生效的基类姓名格式。复用已核对的 Company 词表及冰岛词干函数。Rust 沿用无参与显式性别名字入口，地区专用方法返回 Result 并支持 unique。v0.x 未发现等价实现，工厂装配沿用现有编译路径。

隔离基线 `f2969fc1`。PHP 8.5 验证 2,560 组姓名、名字、姓氏和称谓样本。Rust 1.94.0 格式、全目标严格 Clippy、37 项 Faker、49 项 Seeder、MySQL 8.4.13／SQLite 两项工厂回归通过。实际应用 `main artisan db:seed --database=mysql --force` 写入并回读五地 25 条姓名和公司名称。实际 `./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。

丹麦回归最初只按空白统计姓名成分，现按空白与连字符判断，重跑通过。隔离副本原始 Seeder 已恢复，清理本批独占 Cargo 产物 2.2 GiB 与站点产物 39 MiB。提交前额度剩余 46%、项目卷剩余 81 GiB。下一批核对波兰与立陶宛的性别姓氏规则及其余 Person 地区。

## Faker 波兰和立陶宛姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。比对 FakerPHP v1.24.1 [波兰 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/Person.php)和[立陶宛 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lt_LT/Person.php)。保留姓名模板、称谓和重复权重，复用 Company 的姓氏词表。立陶宛无参 lastName 沿用固定上游从名字抽样的模板，显式性别和全名使用对应姓氏。Rust 通过 lastNameWithGender 表达可选性别并与 lastName 共用去重历史。v0.x 未发现等价实现，沿用现有编译式工厂装配。

隔离基线 `0c46e28a`。使用 PHP 8.5 核对 1,024 组姓名、姓氏和称谓样本。Rust 1.94.0 格式、全目标严格 Clippy、38 项 Faker、49 项 Seeder 与 MySQL 8.4.13／SQLite 工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读 14 条样本。实际 `./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。

隔离副本修正 UniqueFaker 方法所属实现及 Urbonas 同时属于立陶宛名字和姓氏词表的测试边界，重跑通过。原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 46%，项目卷剩余 81 GiB。其余 Person 姓名仍待核对。

## Faker 英国、南非、塞浦路斯及法语和意大利语地区姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。按 [FakerPHP v1.24.1 Person Provider](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 接入英国、南非、塞浦路斯、比利时法语、加拿大法语、瑞士法语和瑞士意大利语。复用公司格式已核对的七套姓氏表，保留固定上游的地区名字、称谓和有效全名模板，包括瑞士两地继承的模板。Rust 沿用无参与显式性别方法及已有去重行为。v0.x 未发现等价实现，工厂装配沿用当前编译路径。

隔离基线 `35f1e1a6`。PHP 8.5 比对 2,570 条名字及模板项目和七套姓氏表，验证 3,584 组上游样本。Rust 1.94.0 格式、全目标严格 Clippy、39 项 Faker、49 项 Seeder 以及 MySQL 8.4.13／SQLite 工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读七地 42 条姓名、称谓和公司名称。实际 `./artisan inspire`、三语示例编译、VitePress 与六项浏览器检查通过。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 46%，项目卷剩余 81 GiB。下一批继续核对西语 Person 姓名与公司词表的复用。

## Faker 西语地区 Person 姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 [阿根廷](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_AR/Person.php)、[西班牙](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_ES/Person.php)、[秘鲁](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_PE/Person.php)与[委内瑞拉](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_VE/Person.php) Person。四地保留称谓、姓名后缀和模板重复权重，含委内瑞拉的多名和复姓模板。公开姓名入口复用 Company 已核对的姓氏词表和 Rust 现有姓名渲染及去重。v0.x 未发现等价实现，沿用当前编译式工厂装配。

隔离基线 `478f75a0`。PHP 8.5 逐项核对 878 条名字、后缀和模板以及四套姓氏表，验证 2,048 组样本。Rust 1.94.0 格式、全目标严格 Clippy、40 项 Faker、49 项 Seeder 和 MySQL 8.4.13／SQLite 工厂回归通过。实际 `main artisan db:seed --database=mysql --force` 写入并回读四地 24 条姓名、称谓和公司名称。实际 `./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB，提交前额度剩余 46%，项目卷剩余 81 GiB。下一批核对中欧 Person 和性别姓氏规则。

## Faker 中欧六地 Person 姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 [FakerPHP v1.24.1 Person](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 的奥地利德语、瑞士德语、捷克、斯洛伐克、匈牙利和克罗地亚 provider。Person 与 Company 共用姓氏，克罗地亚也共用名字词表。保留捷克和斯洛伐克的性别姓氏与通用称谓，匈牙利的姓氏在前、婚后姓名及连字符模板，瑞士德语继承的姓名模板。Rust 沿用无参方法、显式性别入口和去重，婚后姓名专用入口对其他地区返回错误。v0.x 未发现等价实现，沿用当前编译式工厂装配。

隔离基线 `a33b1f84`。PHP 8.5 核对 3,072 组姓名、姓氏及称谓样本，并逐项核对复用词表。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读六地 38 条数据，覆盖两个匈牙利婚后姓名入口。Rust 1.94.0 格式、全目标严格 Clippy、41 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译与六项浏览器检查通过。旧丹麦测试将词表内连字符解析为额外姓名成分，按上游模板的空格分组修正后复验通过。

原始 Seeder 已恢复。清理独占 Cargo 构建文件约 2.2 GiB 和站点产物约 39 MiB。提交前可用额度 46%，项目卷剩余 81 GiB。下一批继续核对希腊、保加利亚和斯洛文尼亚 Person 的性别姓氏模板。

## Faker 希腊、保加利亚与斯洛文尼亚姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[希腊](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_GR/Person.php)、[保加利亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/bg_BG/Person.php)及[斯洛文尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sl_SI/Person.php) Person。希腊和保加利亚保留性别姓氏及全名模板，斯洛文尼亚共用姓氏词表，且总称谓入口与继承的两个专用称谓入口保持原有差异。希腊与斯洛文尼亚复用 Company 姓氏表。Rust 沿用显式性别方法和去重历史。v0.x 未发现等价实现，沿用现有编译式工厂装配。

隔离基线 `d209766c`。PHP 8.5 核对 1,536 组姓名、姓氏与称谓样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读三地 27 条样本。Rust 1.94.0 格式、全目标严格 Clippy、42 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译及六项浏览器检查通过。

原始 Seeder 已恢复，已清理本批独占 Cargo 产物约 2.2 GiB 和站点产物约 39 MiB。项目卷剩余 81 GiB。下一批核对埃及、约旦、以色列、亚美尼亚和格鲁吉亚的姓名模板。

## Faker 埃及与西亚五地姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。核对 FakerPHP v1.24.1 [Person](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 的埃及、约旦、以色列、亚美尼亚和格鲁吉亚 provider，保留各地称谓、埃及缩写前缀及全名模板的重复权重。前四地与 Company 共用姓氏词表，格鲁吉亚使用固定 Person 姓氏表。Rust 复用现有姓名渲染与去重，v0.x 未发现等价实现。工厂装配继续使用现有编译式入口。

隔离基线 `95afa490`。PHP 8.5 核对 2,560 组姓名、姓氏与称谓样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读五地 30 条。Rust 1.94.0 格式、全目标严格 Clippy、43 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译及六项浏览器检查通过。

原始 Seeder 已恢复。清理本批独占 Cargo 产物约 2.2 GiB 和站点产物约 39 MiB，项目卷剩余 81 GiB。下一批核对俄语、乌克兰语及哈萨克语姓名和父名。

## Faker 俄语、乌克兰语和哈萨克语姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[俄语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ru_RU/Person.php)、[乌克兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/uk_UA/Person.php)与[哈萨克语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/kk_KZ/Person.php) Person。保留俄语、乌克兰语父名与全名模板，俄语姓氏保留固定女性词尾和无参等权抽样。Rust 通过 middleNameWithGender 表达可选性别，与 middleName 共用去重历史。哈萨克语使用 Rust 统一字符串选择器映射上游的数字性别常量。v0.x 未发现等价实现，工厂装配沿用当前编译路径。

隔离基线 `bb4b856b`。PHP 8.5 按各 provider 的性别常量核对 1,536 组姓名、姓氏及称谓样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读三地 22 条样本。Rust 1.94.0 格式、全目标严格 Clippy、44 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例编译及六项浏览器检查通过。

原始 Seeder 已恢复。清理本批独占 Cargo 产物约 2.2 GiB 和站点产物约 39 MiB。下一批继续核对孟加拉国、印度尼西亚、马来西亚、韩国、泰国和越南的姓名规则。

## makeOne、makeMany 与 lazy 停点复验

隔离基线 `e3797639`。对照固定 Laravel Factory 的 `lazy`、`makeOne` 和 `makeMany`。实际应用二进制在独立 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`。`makeOne` 和 `makeMany` 返回未保存模型，默认批量依次返回两条和一条，逐条属性覆盖按顺序生效，零条返回空集合。预构造结束后目标表零行。单条与批量 `lazy` 取得 Future 后仍为零行，两种回调分别执行两次，按主键顺序回查四条批量和两条单条记录。Rust 批量接口继续返回集合，异步写入从等待 Future 时开始。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L357)

Rust 1.94.0 工作区全目标严格 Clippy 通过。基线的 `cargo fmt --all --check` 发现一处既有 Sanctum 测试排版差异，仅在隔离副本格式化后复验通过。Seeder 49 项、MySQL／SQLite 工厂回归两项、`./artisan inspire`、三语示例编译、VitePress 构建及根路径和 `/docs/` 下六项浏览器检查通过。三语示例补充未保存状态、批次数量和零条结果的断言。

临时 Seeder 已恢复并核对基线内容。清理隔离副本独占 Cargo 产物 2.2 GiB、站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。其余 Faker 地区和 formatter 差异仍由 05-04 跟踪。

## Faker 南亚、东南亚和韩国姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 [FakerPHP v1.24.1 Person](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider) 的孟加拉国、印度尼西亚、马来西亚、韩国、泰国和越南 provider。复用 Company 的七组对应词表，保留印度尼西亚性别姓氏和后缀、韩国无间隔全名、泰国称谓和后缀、越南中间名分组及马来西亚各姓名模板的权重和空格。马来西亚显式性别名字沿用继承的 John／Jane，无参入口按八组地区模板抽样。Rust 通过现有类型化姓名接口和去重入口调用；语言各自使用随机数生成器。马来西亚地区专用格式化器的独立公开入口及其他 provider 方法继续跟踪。v0.x 未发现等价姓名入口，Goravel 式编译装配沿用当前工厂入口。

隔离基线 `f5d06cc4`。PHP 8.5 核对 3,072 组固定上游样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并按主键回查 768 个姓名，逐组按相同上游词表和模板验证全名、名字、姓氏、称谓，另核对 128 组越南中间名。Rust 1.94.0 全目标严格 Clippy、45 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、`./artisan inspire`、三语示例、VitePress 和六项浏览器检查通过。既有 Sanctum 测试的格式差异仅在隔离副本处理，格式复验通过。

原始 Seeder 已恢复。清理本批独占 Cargo 产物约 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。下一批核对尚未接入的 Person 地区及专用格式化器。

## Faker 马来西亚 Person 专用格式化器

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)的假数据入口。按 [FakerPHP v1.24.1 马来西亚 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Person.php) 暴露 17 个姓名成分和 `myKadNumber`。直接调用复用全名模板词表，保留空成分、尾部空格和华人名字的双成分。MyKad 保留独立月日抽样、出生地范围及排除值、性别末位规则。Rust 采用布尔值与可选分隔符表达 PHP 的 bool/string 参数，采样日期沿用 Faker 时区设置。所有入口支持独立去重，非马来西亚地区返回错误。v0.x 未发现等价入口，沿用当前编译式应用装配。

隔离基线 `4b08f0fd`。PHP 8.5 核对 512 组上游样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 核对 128 组、共 2,560 个值，逐条回读并验证词表、分隔符及数字范围。首轮验收任务重叠时，临时 Seeder 替换影响默认 Seeder 测试，且临时 MySQL 容器名冲突。恢复种子文件并串行复验后，49 项 Seeder、MySQL／SQLite 工厂回归和实际 Seeder 均通过。Rust 1.94.0 全目标严格 Clippy、46 项 Faker、`./artisan inspire`、三语示例编译、实际运行文档示例、VitePress 和六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本格式化。

原始 Seeder 已恢复并核对。清理本批独占 Cargo 产物 2.2 GiB、站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。05-04 后续核对罗马尼亚、摩尔多瓦、塞尔维亚等尚未接入的地区姓名。

## Faker 罗马尼亚语与塞尔维亚语姓名及 CNP

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的罗马尼亚、摩尔多瓦和三个塞尔维亚 Person provider。罗马尼亚和摩尔多瓦复用相同的姓名及称谓词表。塞尔维亚保留无显式文字、西里尔和拉丁三套固定目录，沿用 BCP 47 的文字标记。罗马尼亚 `cnp` 支持三种出生日期精度、县代码、居住身份、序列与校验位及去重。Rust 按公历日期抽样，保留指定月份与抽样日冲突时的错误，以 Result 表达。v0.x 未找到对应工厂入口，Goravel 式编译装配无需改动。

隔离基线 `372997f4`。PHP 8.5 校验 2,560 组固定上游姓名样本和 128 组 CNP。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读 640 个姓名和 128 个 CNP，验证姓名成分词表、模板、日期、县代码、性别与居住身份前缀、序列和校验位。48 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、全目标严格 Clippy、`./artisan inspire`、三语示例编译及实际运行、VitePress 与六项浏览器检查通过。首轮发现旧马来西亚测试将当地词表允许的 John／Jane 一律判为错误，修正断言后复验通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。下一批核对爱沙尼亚、拉脱维亚和黑山 Person。

## Faker 爱沙尼亚、拉脱维亚和黑山 Person

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[爱沙尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/et_EE/Person.php)、[拉脱维亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lv_LV/Person.php)和[黑山](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/me_ME/Person.php)词表及继承模板。接通三地姓名入口，拉脱维亚新增 `driverLicence()` 和去重，复验已有护照及个人身份号码。Rust 沿用显式 locale 和 Result，既有公历日期入口表达拉脱维亚出生日期。v0.x 未找到对应实现，沿用现有编译式工厂接线。

隔离基线 `beed80a3`。PHP 8.5 核对 1,536 组上游姓名以及 128 组拉脱维亚证件样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 384 个姓名及 384 个拉脱维亚证件值，校验词表、模板、掩码和身份校验位。50 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、全目标严格 Clippy、`./artisan inspire`、三语示例编译及实际运行、VitePress 与六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物约 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。下一批核对印度和尼泊尔 Person 模板及中间名。

## Faker 印度英语和尼泊尔语姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的[印度英语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_IN/Person.php)和[尼泊尔语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ne_NP/Person.php)。保留名字、姓氏、全名模板的重复权重及中间名词表。印度英语只提供男性中间名，两地均无通用无参中间名。Rust 可用现有显式性别方法或直接调用，支持去重及词表耗尽错误。v0.x 未找到等价实现，编译式工厂装配无需调整。

隔离基线 `f56162ea`。PHP 8.5 核对 1,024 组上游姓名和 384 个中间名。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回读 256 个全名及 384 个中间名，按固定词表和模板验证，另执行文档中的女性中间名示例。51 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、Rust 1.94 全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 45%，项目卷可用 81 GiB。下一批核对尼日利亚和乌干达英语姓名模板。

## Faker 尼日利亚和乌干达英语姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的[尼日利亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_NG/Person.php)和[乌干达](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_UG/Person.php) Person。保留尼日利亚双名字模板，乌干达两种顺序及通用、男性和女性姓氏词表。Rust 沿用显式性别接口与去重，旧版无等价 formatter，编译式工厂装配无需调整。

隔离基线 `05eaba56`。PHP 8.5 核对 1,024 组固定上游姓名，MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回查 768 个全名、名字和姓氏值，同步核对称谓与姓名词表及模板。52 项 Faker 包含乌干达三套词表的去重耗尽，49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译和实际运行、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复，清理独占 Cargo 产物约 2.2 GiB、站点产物 39 MiB。提交前额度剩余 45%，清理后项目卷可用 81 GiB。下一批核对蒙古语姓名和专用接口。

## Faker 蒙古语姓名、首字母和证件样本

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 [FakerPHP 蒙古语 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/mn_MN/Person.php)。接通两个名字词表与句点连接模板。独立姓氏延续基础 provider 的 Doe。新增 `namePrefix()`，与证件样本的 `alphabet()` 分别使用各自词表。Rust 以 char 表达单个字符，两者各自支持去重。v0.x 未找到对应逻辑，沿用现有编译式工厂装配。

隔离基线 `0a5d0286`。PHP 8.5 核对 512 组上游姓名和 128 组首字母、证件样本。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并按顺序逐条回读 768 个值，核对词表、模板、证件掩码和字母范围，实际执行文档示例。53 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、全目标严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 45%，清理后项目卷可用 81 GiB。下一批核对新加坡英语 Person 与身份样本。

## Faker 新加坡英语姓名及 NRIC／FIN

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)和 [FakerPHP v1.24.1 新加坡 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_SG/Person.php)。保留继承的基础姓名及 `nric`、`fin`、`singaporeId` 的 1968／2000 年边界、两套校验字母与去重。Rust 以可选公历日期表达参数，省略时沿用现有日期及应用时区配置，非 `en-SG` 返回不支持错误。v0.x 无对应接口，编译式工厂装配无需改动。

隔离基线 `a9963589`。PHP 8.5 校验 512 组姓名及 128 组身份号码。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回读 640 个值，核对姓名、年份、证件前缀及校验字母，并运行文档示例。54 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、Rust 1.94 全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批核对比利时荷兰语姓名与 RRN。

## Faker 比利时荷兰语姓名及 RRN

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)和 [FakerPHP v1.24.1 比利时荷兰语 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_BE/Person.php)。接通地区名字和基础全名模板，逐项核对并复用 Company 的 218 个姓氏。RRN 保留日期、序号奇偶、女性序号可到 998、世纪偏移和模 97 校验位。Rust 返回字符串保留前导零，沿用 Faker 公历日期及时区实现、区域错误和去重。v0.x 无对应入口，编译式装配无需调整。

隔离基线 `654b5415`。PHP 8.5 校验 512 组姓名、128 组 RRN。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 512 个值，核对姓名、出生日期范围、两个世纪的校验位、序号和文档示例。55 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、Rust 1.94 全目标严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批对账固定 Person provider 的地区专用接口及默认英语边界。

## Faker Person 姓名前缀、后缀及默认英语复核

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)和 [FakerPHP v1.24.1 Person](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。核对 24 个地区／formatter 组合，包括瑞士德语、法语和意大利语继承。公开 `prefix()`、`suffix()` 复用现有全名词表。意大利语后缀的 null 表达为 None，亚美尼亚语、台湾华语的空字符串表达为 Some，去重按不同值编码。默认 en-US 的名字、姓氏及全名模板已有实现，实际验证配置作用域。v0.x 无对应 formatter，编译式装配无需调整。

隔离基线 `d4bf0405`。PHP 8.5 校验 512 组默认英语姓名和 768 个前后缀样本。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回读 1,152 个值，其中包含 768 个前后缀值与 384 个默认姓名值，核对模板、目录及文档示例。56 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物约 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。后续补齐日语 Kana 姓名及仍缺的地区证件 formatter。

## Faker 日语假名姓名

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 [FakerPHP v1.24.1 日语 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ja_JP/Person.php)。补齐 `kanaName`、`firstKanaName`、`firstKanaNameMale`、`firstKanaNameFemale` 和 `lastKanaName`。保留姓氏在前、单空格、性别词表与重复条目的采样权重。Rust 以可选性别、借用词表结果和 Result 表达接口，五个入口均接入去重。v0.x 未找到等价入口，编译式工厂装配无需调整。

隔离基线 `e656e9c6`。PHP 8.5 验证 128 组上游数据。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 640 个值，核对词表、性别、全名顺序及文档示例。57 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 和六项浏览器检查通过。隔离副本首轮编译发现去重方法落在错误的 impl 块，纠正后复验通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批继续核对地区证件 formatter。

## Faker 瑞士 AHV13 和 AVS13

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 [瑞士法语 Person](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_CH/Person.php)。瑞士德语、法语和意大利语提供 `avs13()`，瑞士德语另提供 `ahv13()`。复用框架 EAN 校验位计算，保留 756 前缀、点分段和前导零。两个方法按名称维护各自的去重历史，在不支持的地区返回错误。v0.x 无对应实现，编译式工厂装配无需调整。

隔离基线 `deced248`。PHP 8.5 验证 512 个上游样本。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回读四个地区／方法组合的 512 个样本，核对格式和 EAN 校验位，实际运行文档示例。58 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理，VitePress 保留已有分块大小提示。

原始 Seeder 已恢复。清理独占 Cargo 产物约 2.2 GiB、站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批核对捷克出生号码和丹麦 CPR。

## Faker 捷克出生号码和丹麦 CPR

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 的[捷克](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/cs_CZ/Person.php)和[丹麦](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/da_DK/Person.php) Person。捷克保留年份边界、月份偏移、校验位、斜杠概率及性别快捷入口。固定 PHP 快捷方法的静态调用声明会触发实例方法错误，Rust 复用同一实例的 `birthNumber` 实现。年龄使用无符号参数并拒绝倒置范围。丹麦复用近百年日期采样及非零首位序号。两地方法支持去重。v0.x 未找到等价入口，编译式工厂装配沿用现有实现。

隔离基线 `d16a9da3`。PHP 8.5 校验 128 对上游号码，MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 写入并逐条回读 256 个值，核对日期、1954／2004 年边界、偏移、校验位和序号，实际运行文档示例。上游样本核对使用 UTC，修正了首次脚本将宿主本地日期用于对照的问题。60 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物约 2.2 GiB、站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批核对法国 NIR 和意大利税号接口。

## Faker 法国 NIR 与意大利税号样本

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 的[法国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_FR/Person.php)和[意大利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/it_IT/Person.php) Person。保留瑞士相应语言继承、法国 NIR 的紧凑与分组格式、科西嘉校验转换及海外省长度规则。意大利复用 bothify 生成大写税号掩码。Rust 沿用可选性别、类型化错误及去重记录。v0.x 未找到等价入口，编译式注册无需调整。

隔离基线 `0df87b34`。PHP 8.5 核对 1,024 个上游样本，实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 1,024 个值。双方都覆盖 2A、2B 及全部五个三位海外省代码，验证性别首位、月份、分组、校验位、税号掩码及文档示例。61 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 和六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物约 2.2 GiB、站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用 81 GiB。下一批核对伊朗国民号码和印度尼西亚 NIK。

## Faker 伊朗国民号码与印度尼西亚 NIK

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 的[伊朗](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fa_IR/Person.php)和[印度尼西亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/id_ID/Person.php) Person。伊朗保留非零地区码、前导零及模 11 校验，地区码直接在 1–999 均匀抽样。印度尼西亚保留 510 个出生地代码、女性日期偏移和默认近 30 年的日期范围。Rust 使用可选公历日期及现有 Faker 时区逻辑。两地接口均接入去重并检查地区。v0.x 无对应 formatter，编译式工厂装配无需调整。

隔离基线 `0b16846d`。PHP 8.5 校验 256 对上游号码。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回查 512 个值，覆盖地区词表、默认日期、显式闰日、性别位移、校验位和文档示例。62 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 44%，清理后项目卷可用约 80 GiB。下一批核对哈萨克斯坦及委内瑞拉证件接口。

## Faker 哈萨克斯坦 IIN 与委内瑞拉身份证号样本

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 的[哈萨克斯坦](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/kk_KZ/Person.php)和[委内瑞拉](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_VE/Person.php) Person。IIN 保留世纪和性别位、两套校验权重、序号范围与双 10 重抽。Rust 的公开校验方法要求 11 位 ASCII 数字，日期用可选公历日期，性别沿用字符串选择。委内瑞拉身份证号按固定前缀抽样，保留各自序号范围及调用方分隔符。两项生成器支持去重。v0.x 无等价接口，编译式工厂装配无需调整。

隔离基线 `ff19c2d4`。PHP 8.5 核对 192 对上游号码，实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 384 个值，覆盖三个世纪、性别位、两种前缀、分隔符和校验值，执行了文档示例。63 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 43%，清理后项目卷可用 80 GiB。下一批核对葡萄牙校验辅助及台湾华语姓名专用接口。

## Faker 葡萄牙 NIF 与台湾华语名字词表

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 的[葡萄牙](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_PT/Person.php)和[台湾华语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Person.php) Person。葡萄牙 `taxpayerIdentificationNumber()` 复用公开 `Faker::dvCalcMod11`，保留前缀权重、右向左循环校验及前导零。校验入口拒绝非 ASCII 数字。台湾华语 `randomName` 以字符串切片和非负抽样数表达输入，保留重复抽样及空词表结果，现有性别名字生成复用同一实现。两个生成器支持去重。v0.x 未找到对应接口，沿用编译式装配。

隔离基线 `c5b91b19`。PHP 8.5 核对 192 对上游值和跨权重周期的校验值。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回查 384 个值，覆盖两元素词表、单元素重复抽样、零抽样及空词表，验证 NIF 前缀和校验位并执行文档示例。64 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 43%，清理后项目卷可用 80 GiB。下一批复核 Person 继承接口与同名地区分派，再对账 Person 的剩余差异。

## Faker Person 同名驾照与护照地区分派

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 [立陶宛](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lt_LT/Person.php)和[拉脱维亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lv_LV/Person.php) Person，补齐立陶宛 `driverLicence()` 八位数字的分派，复用现有掩码与同名去重入口。核对两个地区的护照及驾照格式。v0.x 未找到对应 formatter，编译式工厂装配无需调整。

隔离基线 `2c05cf39`。PHP 8.5 核对 512 个上游样本，实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回查 512 个值，覆盖两个地区和两种证件，并执行文档示例。65 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、Rust 1.94 全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 43%，清理后项目卷可用 80 GiB。下一批继续复核 Person 同名方法和显式地区回退。

## Faker Person 地区回退

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 FakerPHP v1.24.1 [Factory::getProviderClassname](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Factory.php#L36)。Person 查不到地区实现时统一使用 en-US Provider 的姓名、后缀及 SSN，已有地区 Person 保留继承和不支持接口的行为。Rust 复用现有目录和结构化错误，继续接受规范化 BCP 47 地区标识。v0.x 未找到对应分派。编译式工厂装配及其他 provider 无需调整。

隔离基线 `0bc168b0`。PHP 8.5 通过固定实现解析三个回退地区并核对 192 组上游结果。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回读 960 个值，覆盖 en-CA、cy-GB 和 en，验证姓名模板、词表、后缀及 SSN 并执行文档示例。验收同时覆盖 en-SG、en-GB 和 zh-CN 自有 Person 的不支持接口。66 项 Faker、49 项 Seeder、MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 和六项浏览器检查通过。基线 Sanctum 排版差异仅在隔离副本处理。

原始 Seeder 已恢复。清理独占 Cargo 产物 2.2 GiB 和站点产物 39 MiB。提交前额度剩余 43%，清理后项目卷可用约 80 GiB。下一批对账 Person 模板、词表及地区分派的剩余行为。

## Faker Person 词表复核与委内瑞拉 RIF

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 69 个地区 Person 声明，复核姓名模板、名字、姓氏、称谓及特定词表，保留重复项与抽样权重。核对同名 formatter 时发现 [委内瑞拉 Company](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_VE/Company.php#L38) 的 `taxpayerIdentificationNumber` 分派缺失。补上六种前缀、八位主体、随机尾数及可选分隔符。Rust 通过 `taxpayerIdentificationNumberWithSeparator` 表达带参数调用，和无参调用共用去重记录。v0.x 无对应 formatter，编译式工厂装配无需调整。

隔离基线 `74c975e1`。PHP 8.5 核对 576 个固定上游样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并逐条回查 577 个值，覆盖全部前缀、无分隔符、自定义多字符分隔符和三语共用示例。67 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。格式检查仅在隔离副本处理基线 Sanctum 排版差异。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.3 GiB 和站点产物约 40 MiB。提交前周额度剩余 42%，项目卷可用约 80 GiB。后续核对 Company 的 `catchPhrase`、`bs` 及地区词组接口，05-04 保持未闭合。

## Faker Company 标语与商业短语

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)及 FakerPHP v1.24.1 [Company provider 目录](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。补齐 `catchPhrase`、`bs`、`catchPhraseNoun`、`catchPhraseVerb`、`catchPhraseAttribute` 及去重入口。按 Company provider 解析地区与缺失接口。保留固定词表、模板权重、法语去重词根、捷克语模板及 PHP 首字母转换规则，并覆盖台湾无空格拼接和三个地区的空字符串。Rust 使用显式 Result 表达格式化器不可用。v0.x 无对应入口，编译式工厂装配无需改动。

隔离基线 `84cb4b6b`。固定 PHP 8.5 核对 20 个地区及回退场景的 2,256 个样本。使用实际二进制 `main artisan db:seed --database=mysql --force` 写入 MySQL 8.4.13，并逐条回读 2,258 个值，包含三语共用示例的两个结果。两套结果按同一份上游词表、模板和格式规则验证。68 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、全目标严格 Clippy、Artisan、三语示例编译、VitePress 和六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批隔离 Cargo 和站点产物。05-04 继续核对地区电话号码入口。

## Faker 地区手机、座机与分机

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)及 [FakerPHP v1.24.1 PhoneNumber](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。补齐 21 个地区 `mobileNumber`、三地 `fixedLineNumber`、巴西与香港的座机、巴西和韩国的手机专用入口、希腊兼容名及美国和加拿大分机。复用美国区号、交换码和数字掩码。加拿大全量继承美国 E.164，缺少 PhoneNumber provider 的地区也走美国回退。有独立 provider 的地区保留该 provider 的接口边界。以 Rust 显式参数变体表达马来西亚前缀及格式开关，以 Result 表达不受支持的地区。v0.x 未找到相应 formatter，编译式工厂装配无需调整。

隔离基线 `646a5c11`。PHP 8.5 的 1,024 个固定版样本和真实 `main artisan db:seed --database=mysql --force` 在 MySQL 8.4.13 回读的 1,026 个值共用格式校验，覆盖固定模板、地区前缀、法国和新加坡号段、希腊空格、北美区号与分机以及三语示例。69 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 产物 2.3 GiB 和站点产物约 40 MiB。05-04 继续跟踪地区免费电话、服务号、VoIP 和常规展示号码。

## Faker 免费电话、服务号和 VoIP

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)及 [FakerPHP v1.24.1 PhoneNumber](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。补齐免费电话、付费及分摊费用号码、希腊个人号、法国服务号和新加坡、马来西亚 VoIP。按固定 provider 区分同义名称与地区可用性，保留美国／加拿大继承、希腊前导空格规则和法国 08 分支。马来西亚以显式 Rust 参数变体表达前缀与格式开关。支持去重，缺失入口返回 Result 错误。v0.x 无等价 formatter，编译式工厂装配保持现有流程。

隔离基线 `7281b93c`。固定版 PHP 8.5 的 576 个样本和实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 逐条写入回读的 578 个值通过同一套模板校验，其中两条来自三语共用示例。70 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 与六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 和站点产物。常规展示号码与剩余地区辅助接口继续由 05-04 跟踪。

## Faker 常规展示号码

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)及 [FakerPHP v1.24.1 PhoneNumber](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。`phoneNumber` 使用 66 个地区 Provider 的模板和美国回退。保留重复模板权重、芬兰嵌套格式、孟加拉数字、越南区号与用户号码长度关联，以及罗马尼亚和巴西的类别抽样。复用既有地区手机号、座机、VoIP、服务号码、区号和交换码逻辑。巴西另提供支持去重的 `phoneNumberCleared`，其地区边界用 Result 表达。v0.x 无等价实现，编译式工厂装配无需调整。

隔离基线 `26d8ca55`。PHP 8.5 的 2,144 个固定版样本和实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读的 4,290 个值通过格式核对，覆盖全部地区、英语回退和三语示例。71 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复。清理本批独占 Cargo 和站点产物。05-04 继续核对 PhoneNumber 尚未公开的地区辅助 formatter。

## Faker 地区区号和号码组成

对应固定 Laravel [模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)及 [FakerPHP v1.24.1 PhoneNumber](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。公开地区区号、交换码、前缀、用户号码、法国号码片段、香港传真和韩国地区电话。复用整号模板及地区词表，字符串保留前导零，参数使用 Rust 的布尔或可选布尔类型。芬兰和亚美尼亚 `numberFormat` 返回原始模板。对照全部 PhoneNumber 源文件，56 个公开名称均有 Rust 入口。v0.x 未找到对应实现，编译式工厂装配无需调整。

隔离基线 `fea168a5`。固定 PHP 8.5 校验 1,504 个组成样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 逐条回读 1,506 个值，包含三语共用示例。72 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。回归覆盖前导零、去重耗尽和不支持的地区。既有 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复，清理本批独占 Cargo 和站点产物。下一批核对剩余 Faker formatter 及 05-04 整体验收边界。

## 工厂构造暂停关系自动加载

固定 Laravel 的 [Factory::make](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L400) 在构造和 afterMaking 期间暂停关系自动加载，并在返回或失败后恢复。计数批次继续通过模型集合钩子创建。Rust 以任务局部作用域表达暂停，避免影响并发操作，并保留自定义集合键。与固定 Laravel 的直接集合构造一致，`makeMany` 和 `createMany` 的外层集合默认关闭自动加载。返回集合可显式启用。v0.x 无等价实现，编译式工厂装配无需调整。

隔离基线 `f19ffb8d`。实际 MySQL 8.4.13 应用二进制执行 `artisan db:seed --database=mysql --force`，验证计数批次只加载当前模型的关系，`createMany` 和 `makeMany` 保持该行为，随后普通查询恢复按模型设置批量加载，显式启用后批次加载生效。49 项 Seeder、MySQL／SQLite 两项工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。基线 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复，清理本批独占 Cargo 与站点产物。05-04 继续核对固定工厂测试中的公开扩展机制及剩余 Faker 行为。

## Faker Internet 罗马字姓名入口

工厂公开扩展复核沿用已验收的[编译期扩展 trait](#嵌套工厂模型发现与扩展)。固定 Laravel 对应[模型工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [Internet Provider](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)，公开六个地区的 `firstNameAscii` 和 `lastNameAscii`。直接复用用户名及域名所用词表，逐项核对顺序、原始拼写和重复值。Rust 使用借用字符串和 Result 表达静态词表及地区可用性，两个名称各有去重历史。v0.x 无对应入口，编译式工厂装配无需调整。

隔离基线 `c2e61b7a`。固定 PHP 8.5 校验 768 个样本，实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并回读 770 个值，包含三语共用示例。73 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy、Artisan、三语示例编译、VitePress 及六项浏览器检查通过。回归核对埃及名字词表的 14 个唯一值及不可用地区错误。既有 Sanctum 格式差异仅在隔离副本处理。

原始 Seeder 已恢复，清理本批独占 Cargo 与站点产物。05-04 继续核对固定 DateTime 地区入口和其余未对账 Provider。

## 恢复后复验 makeOne、makeMany 与 lazy

隔离基线 `fd1e9623`。复核固定 Laravel 的 [Factory::lazy、makeOne 与 makeMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L357)。v0.x 未提供对应的同名接口。本批沿用现有实现，Rust 的批量接口返回集合，`lazy` 返回可重复调用的异步回调，写入从等待 Future 时开始。

实际 MySQL 8.4.13 使用 `main artisan db:seed --database=mysql --force`。`makeOne` 返回未保存的模型并保留工厂数量，随后两次默认 `makeMany` 依次返回两条和一条。显式属性按输入顺序生效，零条返回空集合，预构造后目标表为零行。调用单条与批量 `lazy` 取得 Future 后目标表仍为零行，等待两种回调各两次后依序回读四条批量记录与两条单条记录。

Rust 1.94.0 全工作区全目标严格 Clippy、49 项 Seeder 回归、MySQL／SQLite 两项工厂回归及 `./artisan inspire` 通过。MySQL 的清库回归使用独立测试库。原基线的 Sanctum 测试排版差异仅在隔离副本处理，随后格式检查通过。三语示例增加批量属性覆盖后的未保存断言和单条回调重复调用，均通过编译。VitePress 构建及根路径与 `/docs/` 下六项浏览器验收通过。

临时 Seeder 已恢复，MySQL 验收容器已移除。清理本批独占 Cargo 产物 2.3 GiB 和站点产物约 40 MiB。提交前周额度剩余 41%，项目卷可用 78 GiB。05-04 其余 Faker 地区和 formatter 继续跟踪。

## Faker DateTime 地区格式

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [地区 DateTime Provider](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。捷克语、土耳其语、简体中文、繁体中文和格鲁吉亚语使用各自的星期和月份名称。土耳其语与两种中文使用地区化的上午和下午。捷克语日期数字不补零，增加 `monthNameGenitive` 和 `formattedDate`。后者按上游顺序独立抽取日、月、年，因此结果可能超出有效日历日期。Rust 沿用已配置的时区和类型化时间参数，对不支持的地区通过 `Result` 返回错误。两个新方法支持 `unique`。v0.x 未发现等价的捷克语入口，Goravel 式应用装配无需调整。

隔离运行以 `caea64ea` 的源码和文档为基线。固定 PHP 8.5 的 24 组成对时区与地区结果逐值匹配，另对照 PHP 词表及 256 条捷克语显示日期样本。MySQL 8.4.13 上用实际应用二进制运行 `artisan db:seed --database=mysql --force`，顺序回读 473 行，包含 24 条纪元时区样本、384 条地区随机样本及 65 条捷克语显示日期，其中一条来自三语共用示例。验证了中文 Seeder 的时段、星期和月份，也覆盖英语回退、捷克语时区跨月和唯一值耗尽。

74 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理，随后格式检查通过。临时 Seeder 已恢复，容器已移除，清理本批独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 40%，项目卷可用 80 GiB。后续继续逐项对账 DateTime 和其他尚未覆盖的 formatter。

## Faker DateTime 的 PHP 输出格式

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) 与 [FakerPHP v1.24.1 的 date、time](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php#L100)。复用 v0.x `44632383` 的日期输出格式化器，按 [PHP 8.5.0 的日期输出实现](https://github.com/php/php-src/blob/php-8.5.0/ext/date/php_date.c#L653) 校准转义、负年份、复合格式、UTC 标识及数值时区缩写。现有 `date` 和 `time` 接受 Chrono 格式，迁移 PHP 格式时使用显式的 `dateWithPhpFormat` 和 `timeWithPhpFormat`。两个入口沿用类型化日期上界和有效时区，各自与对应的 Chrono 方法共享 `unique` 历史。Goravel 式应用装配无需调整。

隔离基线 `5bda15eb`。MySQL 8.4.13 实际运行 `main artisan db:seed --database=mysql --force`，顺序回读 477 行，并与 PHP 8.5.12 逐值比较。涵盖七个时区、29 个固定纪元结果和 448 个携带采样时间戳的结果。固定值按 FakerPHP 入口对照，随机值按 PHP DateTime 对照。包含空格式、尾部反斜线、Unicode 字面量、三语示例、半小时与 45 分钟偏移。日历回归另覆盖巴黎历史秒级偏移和扩展年份。

75 项 Faker、49 项 Seeder、两项日历回归、MySQL／SQLite 两项工厂回归、全工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理，随后格式检查通过。原 Seeder 已恢复，MySQL 容器已移除，清理本批独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 40%，项目卷可用 80 GiB。05-04 继续对账 DateTime 的显式时区返回形式、PHP 表达式及其他 formatter。

## Faker DateTime 显式输出时区

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories) 与 [FakerPHP v1.24.1 DateTime](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php)。给公元日期、类型化和表达式区间以及四个日历范围补上 `InTimezone` 变体。输出时区按显式参数、生成器覆盖和应用时区取值。区间仍使用原始采样逻辑和表达式解析时区。Rust 用 `DateTime<Tz>` 保留时区名，原入口返回 `DateTime<Utc>`。按 FakerPHP 的序列化去重行为，同一方法对 UTC 版本共用历史，命名时区按当地日期时间与时区名区分。夏令时回拨的重复本地时间在同一时区识别为重复。v0.x 提供日历时区解析部件，未提供对应的 Faker 变体。Goravel 式装配无需调整。

隔离基线 `385a6a61`。MySQL 8.4.13 实际运行 `main artisan db:seed --database=mysql --force`，顺序回读 51 行。用 PHP 8.5.12 和固定 FakerPHP 对照五个输出时区的纪元、公元下界、固定区间、四种日历范围上下界及三语共用示例。另对照默认的 419 个时区和全部 676 个大写两字母筛选值，247 组非空列表共 418 项与内置列表一致。回归覆盖显式覆盖、默认值、错误区间、同偏移别名、跨表达式去重及夏令时回拨。

76 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、全工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理，随后格式检查通过。临时 Seeder 已恢复，容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 40%，项目卷可用 80 GiB。后续继续对照 DateTime 的 PHP 表达式语义及其余 formatter。

## Faker 日期表达式的时区词法

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)、[FakerPHP DateTime](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/DateTime.php) 和 [PHP 8.5.0 timelib 的时区词法](https://github.com/php/php-src/blob/php-8.5.0/ext/date/lib/parse_date.re#L978)。复用从 v0.x 迁入的扫描器。保留输入的原始大小写以校验命名时区首段，并继续使用现有时区查找。修复扫描器遇到没有字母的分隔符时吞掉后一个标记的问题。例如 `Etc/GMT-5` 在表达式中按时区名和数值标记处理，行为与直接构造该 IANA 时区不同。通过 Rust 的错误类型保留严格解析接口，Faker 的区间端点按上游将失败解析回退为 Unix 纪元。Goravel 式初始化无需修改。

隔离基线 `a99f1ddd`。PHP 8.5.12 对照 2,340 组日期和区间，以及固定时区数据库 598 个名称的原拼写、全小写和全大写变体（1,794 组），4,134 组全部一致。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并按顺序回读 21 行，包含三语共用示例。每行对照 `strtotime`、FakerPHP 区间端点及其相对区间的结果，覆盖成功、严格错误和 Unix 纪元回退。

76 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、三项日历回归、全工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理，随后格式检查通过。临时 Seeder 已恢复，MySQL 容器已移除。清理独占 Cargo 产物 2.3 GiB 和约 106 MiB 差分程序产物，以及约 40 MiB 站点产物。提交前周额度剩余 40%，项目卷可用 80 GiB。日期语法的样本外覆盖和其余 formatter 继续跟踪。

## 设置工厂数量后的链式配置

对应固定 Laravel 的[序列与工厂关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#sequences)及 [Factory 构建器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L669)。Laravel 在设置数量后继续提供关系、序列、回调和状态配置。Rust 的 `FactoryBatch` 委托同一个 `Factory` 实现，保留类型化集合结果及显式关系描述符。再次调用 `count` 可替换数量，`forEachSequence` 按序列长度设置数量，普通序列、交叉序列、回调、关系和延迟属性保留数量。固定 Goravel 的[工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)同样保留链式配置。v0.x 的 factory 为查询工厂，无对应 Eloquent 批量功能。

隔离基线 `632c6d0e`。实际 MySQL 8.4.13 应用二进制执行 `artisan db:seed --database=mysql --force`，三语共用示例写入两个父模型及每个父模型的一条子记录。第二组使用 `forEachSequence` 把六条改为两条，并验证两条记录共用生成的上级、创建前修改名称、创建后状态和读回顺序。构造回调失败后父表仍为三行。MySQL／SQLite 工厂回归同时覆盖回调顺序、替换数量、交叉序列、延迟属性和 Pivot 关联。交叉序列回归使用独立的派生字段，保留显式属性优先的既有契约。

76 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、全工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。原 Seeder 已恢复，MySQL 容器已移除，清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 40%，项目卷可用 80 GiB。整单元继续核对关系参数，例如无显式计数的子工厂与按名称关联既有父模型，并对账其余 Faker formatter。

## 无显式数量的子工厂与按名称关联既有父模型

对应固定 Laravel 的[工厂关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#factory-relationships)，对照 [Relationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Relationship.php)、[BelongsToRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/BelongsToRelationship.php) 和固定框架测试。直接向 `has` 或 `hasNamed` 传入工厂时，每个父模型执行单条子模型路径，保留构造、保存、嵌套关系和创建回调。显式批次继续使用模型集合钩子及自己的数量，支持零条。Rust 的 `FactoryChildren` 保留这两种类型的语义。新增 `forExistingNamed`，在构造时用已注册的名称和 Rust 关系描述符解析已有父模型，沿用 `forExisting` 的外键和多态目标验证。固定 Goravel 的类型化工厂约定无需改动，v0.x 的查询工厂无对应模型关系实现。

隔离基线 `dda7fb61`。实际 MySQL 8.4.13 应用二进制执行 `artisan db:seed --database=mysql --force`，运行三语共用示例并核对最终四条父记录、五条子记录。覆盖父批次逐个生成单条子记录、显式零数量、按名称复用父键，以及无效关系名失败且无额外子记录。MySQL／SQLite 两项工厂回归验证单条路径跳过批量集合钩子，显式数量仍走批量路径。

76 项 Faker、49 项 Seeder、两项工厂回归、全工作区全目标严格 Clippy、`./artisan inspire`、六段三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，MySQL 容器已移除，清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 40%，项目卷可用 80 GiB。工厂与 Seeder 整单元继续复核默认关系推断和未核定的 Faker formatter。

## 工厂默认关系名推断

对应固定 Laravel 的[工厂关系](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#factory-relationships)和 [Factory::guessRelationship](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L714)。`hasInferred` 按相关模型名生成小驼峰复数名，模型注册该名称时优先选用，否则回退到单数。`hasAttachedInferred` 采用复数，`forInferred` 与 `forExistingInferred` 采用父模型的小驼峰名。Rust 复用当前英文复数器与关系注册表，保留显式描述符类型，并通过现有命名解析处理描述符不匹配和自定义关系。固定 Goravel 的类型化工厂装配无需改动，v0.x 无 Eloquent 关系名推断实现。

隔离基线 `810e10e4`。MySQL 8.4.13 的真实应用二进制运行 `artisan db:seed --database=mysql --force`，回读九条父模型、十条子模型和一条 Pivot 记录。覆盖三语共用示例、复数子关系、单数回退、新建和复用父模型、推断多对多关系。MySQL／SQLite 工厂回归额外验证模型同时注册单数和复数时优先复数。真实 Seeder 复验按数据库外键查询比较键值，涵盖有符号和无符号字段返回类型。

76 项 Faker、49 项 Seeder、两项工厂回归、全工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。原 Seeder 已恢复，容器已移除，清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 仍需对账剩余 Faker formatter，尤其是使用 fake-rs 词表的城市与街道等 Address 项。

## 美国英语与简繁中文地址词表

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [美国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Address.php)、[简体中文](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_CN/Address.php)与[繁体中文](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Address.php) Address。美国城市和街道按上游等权模板组合已移植的 Person 名字及地址词表。简中接通城市、省级名称、简称、国家和 `area()`，并采用基础 Address 的街道和楼号模板。台湾县市与区乡镇分级取样，接通街道词干 `street()`、路街、可选段、邮编和固定省名及简称。词表保留原顺序和重复项。Rust 以静态切片表达词表，以 `Result` 表达两个地区专用入口，并沿用各 formatter 的去重历史。v0.x 未发现对应地址生成器，Goravel 式装配无需改动。

隔离基线 `6081682f`。PHP 反射提取的 15 个词表逐项一致，并用 PHP 8.5.12 核对三地共 384 个地址样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 385 行，包含三语共用示例，覆盖词表成员资格、美国换行及邮编、简中城市和区拼接、台湾县市归属和街道段巷弄楼格式。Rust 回归验证错误地区及两项路街唯一值耗尽。

77 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版问题仅在隔离副本处理。恢复原 Seeder 并移除容器，清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。后续继续核对台湾数值辅助入口、法语 Address 和其他地区剩余 fake-rs 回退。

## 法国地址与台湾地址数字

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [法国 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_FR/Address.php) 与 [台湾 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Address.php)。法国城市保留姓氏、后缀及双姓三种模板的权重，街道采用三个等权模板，邮编回到上游的五位数字。接通街道前缀、法国大区及行政区格式化器。Rust 用字符串对表示编号及名称，统一保留 `01`、`2A` 等编号。台湾补齐一至九汉字、1–100 和 1–1000 三个地区数值入口。地区专用入口都支持 `unique`，并对范围之外的地区返回错误。v0.x 未发现等价地址生成器，编译式工厂装配无需调整。

隔离基线 `a30c0b0b`。PHP 反射核对五个法国词表，包含 101 条编号与名称的对应关系。PHP 8.5.12 抽验 256 组地址与数字结果。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 257 行并按上游词表和模板核对，包含三语共用行政区示例。Rust 回归遍历行政区和汉字数字的唯一值并核对耗尽、编号及错误地区。

78 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。原始 Seeder 已恢复，测试容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。其他法语地区的继承数据和覆盖模板以及其他地址 provider 继续对账。

## 比利时、瑞士与加拿大法语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [比利时](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_BE/Address.php)、[瑞士](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_CH/Address.php)与[加拿大](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fr_CA/Address.php)的法语 Address。三地沿用法国的国家、大区和行政区目录，仅按地区覆盖城市、街道、楼号、邮编及地址行模板。比利时保留邮编行开头的空格并暴露 `province`。瑞士保留三种街道模板、带字母楼号和本地街道前缀，`canton` 返回代码与名称的字符串对。加拿大保留地名后缀权重、可选附加地址和两种大写邮编形式。Rust 沿用已有 Person 词表和 formatter 去重，地区专用方法通过 `Result` 传播错误。v0.x 无对应生成器，编译式工厂装配无需调整。

隔离基线 `2e5df3c5`。逐项比对 11 份地区词表，保留原有顺序和重复项，并用 PHP 8.5.12 核对 384 个运行样本。真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 385 行，覆盖三地模板和继承词表，包含三语共用的瑞士地址示例。Rust 回归另外遍历 26 条州记录，验证去重耗尽和地区错误。

79 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。原始 Seeder 已恢复，测试容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 继续对账其他地区 Address 的继承、词表和格式。

## 加拿大、澳大利亚和新西兰英语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [加拿大](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_CA/Address.php)、[澳大利亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_AU/Address.php)和[新西兰](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_NZ/Address.php)。三地复用上游继承的美国词表和姓名组合模板。加拿大保留省份、受限邮编字母、三种分隔形式和地址行的双空格。澳大利亚保留州、市名覆盖、州邮编范围和三种楼号排列。新西兰保留本地地区、城市后缀和四位邮编。Rust 以 `Result` 表达地区专用入口，对应唯一值历史独立。v0.x 无对应生成器，编译式工厂装配无需调整。

隔离基线 `971b5cad`。按 PHP 8.5.12 反射核对 13 份地区词表的顺序和重复项，执行固定 FakerPHP 抽验 384 个样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 385 行，核对地址格式、继承词表和三语共用样例。Rust 回归验证地区错误和四个澳大利亚楼号字母的唯一值耗尽。

80 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 40 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 继续核对英国等其他英语地区的独立 Address provider。

## 英国英语地址与郡名

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [英国 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_GB/Address.php)。英国邮编按固定 provider 直接抽取 497 条完整字符串。保留楼号街道与独立 Flat／Studio 行的 2:1 权重、英国城市和后缀词表、地址换行及 `county()`。英文城市前缀复用与英国完全相同的美国词表。Rust 的地区专用 `county` 使用 `Result` 并接入 `unique`。v0.x 无对应地址生成器，编译式工厂装配无需调整。

隔离基线 `eda080b2`。PHP 8.5.12 反射核对 7 份词表的顺序与重复项，并用固定 FakerPHP 抽验 256 个样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 257 行，覆盖三行与四行地址、词表组合及三语共用示例。Rust 回归遍历 497 个邮编和 89 个郡名并验证唯一值耗尽。

81 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。原始 Seeder 已恢复，测试容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 41 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 继续核对其他英语及地区 Address provider。

## 南非、乌干达和尼日利亚英语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [南非](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_ZA/Address.php)、[乌干达](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_UG/Address.php)及[尼日利亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_NG/Address.php) Address。南非覆盖城市后缀、省份、简称及四位邮编。乌干达覆盖城市、地区、行政区和一至五位邮编，完整地址沿用上游继承的美国州简称。尼日利亚从基础 Address 继承城市、街道和楼号模板，覆盖国家、州郡、地区与两种五位邮编。Rust 的地区方法通过 `Result` 表达不支持的地区并支持 `unique`。v0.x 未发现对应 Faker 地址实现，编译式工厂装配无需改动。

隔离基线 `4d64bda8`。PHP 8.5.12 反射核对 11 份词表的顺序与重复项，其中两个南非词表与美国完全一致并复用。固定 FakerPHP 抽验 384 个样本，真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 回读 385 行，覆盖三地模板、继承字段及三语共用示例。Rust 回归验证乌干达地区的唯一值耗尽和无效地区调用。

82 项 Faker、49 项 Seeder、MySQL／SQLite 两项工厂回归、工作区全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 和六项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已移除。清理独占 Cargo 产物 2.4 GiB 和站点产物约 41 MiB。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 继续核对其他英语及地区 Address provider。

## 新加坡英语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，复核 FakerPHP v1.24.1 [新加坡 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_SG/Address.php) 及继承的基础 Address。街道名由姓氏与地区后缀组合，门牌与楼栋号使用数字模板，六位邮编保留前导零。两行和三行地址均覆盖。楼层与单元号保留上游直接返回的模板字符。Rust 对地区专用方法返回 Result，并接入 unique，保留原有工厂装配与编译式启动。v0.x 无可复用 Faker 地址生成器。

隔离基线 `5c585555`。PHP 反射核对五份词表及模板，执行固定 FakerPHP 抽验 128 个样本。实际 MySQL 8.4.13 使用 `main artisan db:seed --database=mysql --force` 插入并按序回读 129 行，包含三语共用示例，并逐行核对两种地址格式及词表。回归覆盖无效地区及唯一楼层模板耗尽。

Rust 1.94.0 格式、全工作区全目标严格 Clippy、83 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 均通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查及三个工厂页面示例检查均通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 39%，项目卷可用 80 GiB。05-04 待继续核对其他地区 Address provider。

## 印度英语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，复核 FakerPHP v1.24.1 [印度 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_IN/Address.php) 与继承的基础 Address。地区地址覆盖城市、邦名与简称、街区、小区及 1–5 起首的六位邮编。保留两种街道模板的逗号、住宅小区布局和词表前导空格。地区专用方法返回 Result，并接入 unique。v0.x 无对应 Faker 地址生成器，Goravel 式编译装配不受影响。

隔离基线 `ad4aae59`。PHP 反射逐项核对八份词表，印度与尼日利亚使用相同国家名词表，Rust 复用该常量。固定 FakerPHP 抽验 128 个样本。实际 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入、顺序回读并核对 129 行，包括三语共用示例。回归覆盖两种街道结构、无效地区和唯一街区后缀耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、84 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查与六项工厂页面示例检查通过。既有 Sanctum 排版差异只在隔离副本处理。原始 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 38%，项目卷可用 80 GiB。下一批核对菲律宾与香港英语 Address。

## 菲律宾和香港英语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，复核 FakerPHP v1.24.1 的 [菲律宾 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_PH/Address.php) 与 [香港 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_HK/Address.php)。菲律宾支持描笼涯加城市或市镇、四位邮编、四种楼号格式及继承的美国州名、简称与公寓编号。香港支持街道、村落与屋苑的三行地址、地区及加权命名模板。独立 postcode 沿用基础 provider 的五位模板。Rust 的地区专用方法使用 Result，并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 参考中的编译式装配继续沿用。

隔离基线 `c0f216f3`。PHP 反射核对 16 份词表和保留重复项的模板，固定 FakerPHP 实测两地共 320 个样本。实际 MySQL 8.4.13 使用 `main artisan db:seed --database=mysql --force` 写入、顺序回读并验证 322 行，包含三语共用示例，覆盖菲律宾两种与香港三种地址结构。Rust 回归覆盖不适用地区与唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、86 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查与六项工厂页面示例检查通过。既有 Sanctum 排版差异只在隔离副本处理。原始 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 38%，项目卷可用 80 GiB。05-04 继续核对日语及其他地区 Address。

## 日语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [日语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ja_JP/Address.php) 与 Person 姓氏。地址保留三位加四位邮编、两格间距、47 个都道府县、五种区、由姓氏组成的市町、三个独立番地及可选楼栋。楼栋号为 101–110。Rust 地区专用方法使用 Result，数字组件使用 i64，支持 unique。v0.x 无对应 Faker 地址实现，Goravel 式编译装配继续沿用。

隔离基线 `6b84495d`。PHP 反射对照国家、都道府县、区和后缀，另用固定 Person 姓氏词表和范围验证两种完整地址及所有中间字段。160 个固定 PHP 样本通过。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并回读 161 行，包括三语共用示例，按同一规则核对。Rust 回归覆盖两种地址、数值范围、无效地区与五个区唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、87 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查及六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 38%，磁盘可用 80 GiB。05-04 下一批核对韩语和其他亚洲地址。

## 韩语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [韩语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ko_KR/Address.php) 和基础 Address。完整地址保留特别市或广域市加区、道加市两种组合，门牌号在街道后。继承的 streetAddress 把门牌号放在前面，postcode 独立生成五位。Rust 地区方法使用 Result 并支持 unique。v0.x 无对应地址生成器，固定 Goravel 的编译式应用装配沿用现有路径。

隔离基线 `6fdea23c`。PHP 反射逐项核对八份词表和模板，保留区名重复项。160 个固定 PHP 样本通过。实际 MySQL 8.4.13 使用 `main artisan db:seed --database=mysql --force` 写入、顺序回读并核对 161 行，包括三语共同示例。Rust 回归覆盖两种行政组合、街道地址顺序、门牌号与邮编格式、无效地区及七个城市唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、88 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查及六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 38%，磁盘可用 80 GiB。05-04 下一批核对越南语及其他亚洲 Address。

## 越南语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [越南语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/vi_VN/Address.php) 和既有 Person 词表。完整地址保留三种城市、省及村组结构。两种坊名模板保留村组前缀，复用既有无参 middleName 的名字抽样规则，数字占位符在模板展开后填充。独立邮编保留三个模板。国家词表与美国完全相同，复用该常量。Rust 地区方法返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 参考下的编译式装配沿用现有路径。

隔离基线 `a38dce57`。PHP 反射核对 12 份词表和带重复项的模板，按上游名字、前缀、数字和行政区词表审查 160 个实际 PHP 样本。MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入、顺序回读并核对 161 行，包含三语共用示例及三种地址。Rust 回归覆盖村组前缀、无效地区和唯一县前缀耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、89 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查及六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 38%，磁盘可用 79 GiB。05-04 下一批核对泰语及其他地区 Address。

## 泰语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [泰语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/th_TH/Address.php) 和 Person 词表。完整地址保留两行结构、五种门牌号模板、道路词表、四种城市名模板和五位邮编。省名可单独生成。Rust 沿用 Result 表达地区可用性，已有 unique 接口覆盖泰语道路前缀和省名。v0.x 无对应地址模块，编译式装配沿用现有路径。

隔离基线 `b6d30616`。PHP 反射核对七份词表和门牌模板，对照固定 Person 词表验证 160 个实际 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入、顺序回读并验证 161 行，包含三语共用示例。回归覆盖道路前缀唯一值耗尽、斜杠门牌及两行分隔。

Rust 1.94.0 格式、工作区全目标严格 Clippy、90 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径和 /docs/ 的 12 项浏览器检查及六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 38%，磁盘可用 79 GiB。05-04 下一批核对印尼语及马来语 Address。

## 印尼语和马来语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [印尼语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/id_ID/Address.php) 与 [马来语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Address.php)。印尼语覆盖当地街道、城市、州简称及非零起首五位邮编。马来语覆盖加权门牌和社区格式、地区别名及城镇和邮编关联。Rust 保留无参 postcode，增加 postcodeWithState 接收上游州键并对无效键返回错误。两个入口共用 unique 的邮编历史。地区专用方法使用 Result。v0.x 无对应 Faker 地址模块，编译式装配沿用现有路径。

隔离基线 `fd542d2a`。PHP 反射核对 16 份独立词表或格式，以及马来西亚全部 16 组城镇、州名和邮编范围。固定 FakerPHP 共 320 个样本通过。实际 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入并按序回读 322 行，包含三语共用示例，核对 Perlis 前导零、Pahang 分段范围和同地区组合。Rust 回归覆盖无效州键、不适用地区和按州邮编唯一抽样。

Rust 1.94.0 格式、工作区全目标严格 Clippy、92 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 及独占文档依赖。提交前周额度剩余 38%，磁盘可用 79 GiB。05-04 下一批核对孟加拉语及尼泊尔语 Address。

## 孟加拉语和尼泊尔语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [孟加拉语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/bn_BD/Address.php) 和 [尼泊尔语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ne_NP/Address.php)。孟加拉语保留 1–100 的本地数字街号和独立 ASCII 邮编。尼泊尔语保留一位或两位字符串选区编号、地名与街名的拼接及五位邮编。国家词表与美国完全一致，复用常量。Rust 地区专用方法返回 Result 并接入 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `66edac05`。PHP 反射核对 12 份词表与模板，按同一规则核对 320 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 322 行，包括三语共用示例。Rust 回归覆盖本地街号、两种选区编号长度、不适用地区和孟加拉语街名唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、93 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 38%，磁盘可用 79 GiB。05-04 下一批核对阿拉伯语地区 Address。

## 埃及、约旦和沙特阿拉伯语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [埃及](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_EG/Address.php)、[约旦](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_JO/Address.php) 和 [沙特](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ar_SA/Address.php) Address。三地保留两行结构和可选房屋信息，约旦市名可带方位前缀，独立州名及简称仍按上游美国州词表。埃及行政区编号复用现有身份格式化入口及唯一值历史，统一使用地址行政区词表。沙特独立 cityPrefix 上游缺少静态属性并报错，Rust 保留现有回退，完整地址生成只使用上游城市名。地区方法使用 Result 并支持 unique。v0.x 无对应 Faker 地址模块，编译式装配沿用现有路径。

隔离基线 `b2ab5337`。PHP 反射核对 25 份词表和模板，按同一规则验收三地共 384 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 387 行，包含三语共用示例。Rust 回归覆盖可选房屋格式、不适用地区、埃及两位行政区编号及唯一值耗尽。身份号码既有回归继续通过。

Rust 1.94.0 格式、工作区全目标严格 Clippy、94 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 38%，磁盘可用 79 GiB。05-04 下一批核对波斯语与希伯来语 Address。

## 波斯语和希伯来语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [波斯语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fa_IR/Address.php) 和 [希伯来语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/he_IL/Address.php)。波斯语保留可选城市、楼栋编号与邮编前缀及十位邮编。希伯来语保留两行格式、五位或七位邮编、可选公寓或楼层。省略公寓或楼层时街道行保留尾空格。地区专用方法返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `624ca94c`。PHP 反射核对 14 份词表与模板，执行并核对 320 个固定 PHP 样本。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 写入并按序回读 322 行，包含三语共用示例。回归验证前缀唯一值耗尽、无效地区、两种可选分支及希伯来语尾部空格。

Rust 1.94.0 格式、工作区全目标严格 Clippy、95 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对土耳其语及其他地区 Address。

## 土耳其语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 [土耳其语 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/tr_TR/Address.php)。街道名保留名字与姓氏 2:1 权重，门牌号在街道名后，第二行依次为五位邮编和城市。cityName 采样 81 个本地城市，Rust 复用既有 Result 和 unique 入口。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `36820b0a`。PHP 反射核对六份词表和模板，利用上游 Person 词表、标点和顺序验证 160 个固定 PHP 样本。实际 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入并按序回读 161 行，含三语共用示例。回归检查 81 个城市唯一抽样耗尽及邮编、门牌顺序。

Rust 1.94.0 格式、工作区全目标严格 Clippy、96 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对俄语、乌克兰语及哈萨克语地区 Address。

## 俄语、乌克兰语和哈萨克语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [俄语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ru_RU/Address.php)、[乌克兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/uk_UA/Address.php) 和 [哈萨克语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/kk_KZ/Address.php) Address。乌克兰语保留 24 组城市与州配对，独立调用 city、region 时各自抽样。俄语与哈萨克语沿用独立地区抽样、当地街道顺序和六位邮编，哈萨克语邮编首位为零。地区方法返回 Result，并支持 unique。v0.x 无对应 Faker 地址模块，Goravel 式编译装配沿用现有路径。

隔离基线 `3a0a38ea`。PHP 反射核对 13 份独立词表与 24 组城市和州的配对，逐项核对 384 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 387 行，含三语共用示例。回归覆盖地区格式、无效地区和 24 组配对唯一抽样耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、97 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对波兰语、捷克语及斯洛伐克语 Address。

## 波兰语、捷克语和斯洛伐克语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [波兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/Address.php)、[捷克语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/cs_CZ/Address.php) 和 [斯洛伐克语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sk_SK/Address.php) Address。波兰语保留连字符邮编、门牌字母和省名。捷克语保留三种加权地址布局、可选门牌与两种邮编。斯洛伐克语保留字面反斜杠和 n、空格邮编及与美国相同的街道后缀词表。地区方法沿用 Result 和 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 参考下的编译式装配沿用现有路径。

隔离基线 `b1fcddfb`。PHP 反射核对 15 份词表与模板，按同一规则核对 384 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 387 行，含三语共用示例。Rust 回归检查连字符邮编、字面分隔符、捷克地址三种布局和 14 个地区唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、98 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对德语地区 Address。

## 德国、奥地利和瑞士德语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [德国](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_DE/Address.php)、[奥地利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_AT/Address.php) 和 [瑞士德语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/de_CH/Address.php) Address。保留三地门牌模板、短长街道后缀、州与邮编。奥地利从 223 个完整邮编抽样，瑞士德语另支持城市词干及 26 组州简称和州名。Rust 直接抽取门牌字母类中的 a、b、c，州和后缀继续使用 Result，瑞士州沿用元组和 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `7e8c4574`。PHP 反射核对 19 份词表和门牌模板及 26 组州数据，三地共 384 个固定 PHP 样本通过。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 387 行，包含三语共用示例。回归覆盖地区门牌、瑞士德语州名、唯一值耗尽和无效地区。

Rust 1.94.0 格式、工作区全目标严格 Clippy、99 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对荷兰语地区 Address。

## 荷兰和比利时荷兰语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [荷兰](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_NL/Address.php) 和 [比利时荷兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nl_BE/Address.php) Address。保留完整邮编词表、当地城市和省、门牌模板以及比利时第二行的前导空格。两地复用逐项相同的街道后缀，Rust 沿用 Result 和 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `490c438e`。PHP 反射核对 10 份独立词表和门牌模板，验证 256 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 258 行，含三语共用示例。回归覆盖不同邮编格式、荷兰字母门牌、比利时前导空格和荷兰省名唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、100 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对北欧地区 Address。

## 挪威语和瑞典语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [挪威语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/nb_NO/Address.php) 和 [瑞典语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sv_SE/Address.php) Address。两地共用有权重的街名结构和门牌模板，独立采样当地词表，门牌字母转为大写。挪威保留四位邮编及前导零模板，瑞典保留五位和带空格模板。streetSuffixWord 返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `556a3f3c`。PHP 反射核对 13 份词表、门牌与邮编模板及共同街名权重，验证 256 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 258 行，含三语共用示例。回归覆盖大写字母门牌、各邮编格式和瑞典后缀唯一值耗尽。

Rust 1.94.0 格式、工作区全目标严格 Clippy、101 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对丹麦语、芬兰语及冰岛语 Address。

## 丹麦语、芬兰语和冰岛语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [丹麦语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/da_DK/Address.php)、[芬兰语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/fi_FI/Address.php) 和 [冰岛语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/is_IS/Address.php) Address。丹麦语保留可选楼层和方位以及 kommune。芬兰语保留连字符城市前缀、三位公寓编号与可选地区。冰岛语保留街名权重、三位邮编和地区。丹麦与冰岛共享逐项相同的门牌模板。新增地区专用方法返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，Goravel 式编译装配沿用现有路径。

隔离基线 `00797e7a`。PHP 反射核对 25 份词表和模板，按同一规则验证 384 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 387 行，含三语共用示例。回归覆盖丹麦三种布局、方位唯一值耗尽、芬兰可选地区、冰岛三位邮编和非法地区。

Rust 1.94.0 格式、工作区全目标严格 Clippy、102 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例可编译，VitePress 构建、根路径及 /docs/ 的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收，清理独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与独占文档依赖。提交前周额度剩余 37%，磁盘可用 79 GiB。05-04 下一批核对西班牙语地区 Address。

## 西班牙、阿根廷、秘鲁和委内瑞拉地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [西班牙](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_ES/Address.php)、[阿根廷](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_AR/Address.php)、[秘鲁](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_PE/Address.php) 和 [委内瑞拉](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/es_VE/Address.php) Address。保留四地的城市、街名、地址布局和补充地址，包含阿根廷省级缩写、秘鲁字面门牌符号及尾随空格、委内瑞拉可省略邮编。国家和自治区按上游继承关系共用词表。Rust 用固定标记展开现有字段方法，无需运行 PHP，community 返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `752513dc`。PHP 反射逐项核对 47 份有序词表及模板。按同一规则匹配 512 个固定 PHP 样本及真实 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读的 516 行，含三语共用示例。回归覆盖地区专用布局、自治区唯一值耗尽和无效地区。对照固定 Laravel Factory 及其测试复验 `makeOne`、`makeMany` 和 `lazy`。工厂预构造保持零写入，默认批次数量取用后重置，延迟创建随异步执行写入。 [固定 Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L365)

Rust 1.94.0 格式、工作区全目标严格 Clippy、103 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、根路径及 /docs/ 下的 12 项浏览器检查和六项工厂页面示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 和文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对葡萄牙语 Address。

## 巴西和葡萄牙地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [巴西](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_BR/Address.php) 和 [葡萄牙](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_PT/Address.php) Address。巴西保留地址首行邮编、第二行州缩写、五大区及其缩写。葡萄牙保留完整城市词表、两种邮编长度和三种街道布局。Rust 复用固定模板展开逻辑，按原序抽样，regionAbbr 返回 Result 并支持 unique。v0.x 无对应 Faker 地址模块，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `61fb010f`。PHP 反射核对 26 份有序词表和模板，256 个固定 PHP 样本均符合预期。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖葡萄牙两种邮编、三种街道布局及巴西大区缩写唯一值耗尽。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、104 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对意大利和瑞士意大利语 Address。

## 意大利和瑞士意大利语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [意大利](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/it_IT/Address.php) 和 [瑞士意大利语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/it_CH/Address.php) Address。意大利保留五位邮编和可选补充地址，省名与缩写按上游独立采样。瑞士意大利语保留四位邮编、固定城市词表和带小写字母的门牌，独立街道前缀继续通过 streetPrefix 获取。canton 沿用 Rust 元组接口并支持 unique。v0.x 无等价 Faker 地址代码，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `366ff450`。按 PHP 反射逐项核对 19 份词表和模板及 26 组州缩写和州名。256 个固定 PHP 样本通过格式核验。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖意大利两种街道布局、瑞士字母门牌、州数据和唯一值耗尽。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、105 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对希腊和塞浦路斯希腊语 Address。

## 希腊和塞浦路斯希腊语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [希腊](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_GR/Address.php) 和 [塞浦路斯希腊语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/el_CY/Address.php) Address。希腊保留两种邮编、连字符门牌、地区词表与男女姓氏街道模板。塞浦路斯希腊语地址使用四位邮编和四个城市，保留当地姓氏模板。Rust 共享现有姓氏词表，沿用返回 Result 的 prefecture 和 UniqueFaker。塞浦路斯希腊语的空街道后缀以空字符串表示。v0.x 无等价 Faker 地址代码，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `d3af419e`。按 PHP 反射逐项核对 17 份地址词表和模板及三份复用的姓氏词表。256 个固定 PHP 样本通过格式与字段核验。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖两种希腊邮编、连字符门牌、地区字段、塞浦路斯城市和空街道后缀。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、106 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对罗马尼亚和摩尔多瓦罗马尼亚语 Address。

## 罗马尼亚和摩尔多瓦罗马尼亚语地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [罗马尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ro_RO/Address.php) 和 [摩尔多瓦罗马尼亚语](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ro_MD/Address.php) Address。保留六项加权模板中的四种街道布局，地区词表、县名和邮编按各提供器取值。Rust 用静态数字词表保留 apartmentNumber 的借用返回类型及前导零，先等概率选择一位或两位，再抽数字。楼栋、楼梯单元、楼层及无前缀街名支持 UniqueFaker。v0.x 无等价 Faker 地址代码，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `db6a6fc5`。按 PHP 反射逐项核对 20 份共用及独立词表和模板，并核对数字池覆盖一位和两位模板。256 个固定 PHP 样本通过格式与字段核验。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖四种布局、六位和四位邮编、县字段适用范围、楼梯单元唯一值耗尽及新加坡既有公寓号入口。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、107 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对克罗地亚和斯洛文尼亚 Address。

## 克罗地亚和斯洛文尼亚地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [克罗地亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hr_HR/Address.php) 和 [斯洛文尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sl_SI/Address.php) Address。两地按原序保留街道、城市和国家词表。克罗地亚使用五位邮编及街道查询入口。斯洛文尼亚保留尾数为零的四位邮编、加权门牌模板及完整地址里的两处字面 `\n`。Rust 直接采样固定名称，复用已有地址模板和 UniqueFaker。v0.x 无等价 Faker 地址代码，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `24206956`。按 PHP 反射逐项核对 15 份词表和模板，验证 256 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖当地邮编、斯洛文尼亚字面分隔符、克罗地亚 street 入口及两地 cityName。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、108 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对立陶宛和拉脱维亚 Address。

## 立陶宛和拉脱维亚地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [立陶宛](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lt_LT/Address.php) 和 [拉脱维亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/lv_LV/Address.php) Address。保留立陶宛三种布局和 LT- 邮编、拉脱维亚带 LV- 邮编和地区／城市后缀的布局，以及各地街道、城市和地区词表。Rust 复用模板和现有 Result 型 regionSuffix、street 与 municipality，继续支持菲律宾的 municipality。v0.x 无等价 Faker 地址实现，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `d4944b8b`。按 PHP 反射逐项核对 23 份词表和模板，验证 256 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖立陶宛三种地址布局、拉脱维亚格式、地区后缀唯一值耗尽、市镇的地区边界及菲律宾既有调用。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、109 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对亚美尼亚和格鲁吉亚 Address。

## 亚美尼亚和格鲁吉亚地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [亚美尼亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hy_AM/Address.php) 和 [格鲁吉亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ka_GE/Address.php) Address。保留亚美尼亚以地区开头、以 00 开头的邮编结束的布局，以及格鲁吉亚八种布局、独立城市名和地区属格。Rust 复用地址模板、现有地区及街道方法，regionGenitiveForm 返回 Result 并支持 UniqueFaker。v0.x 无等价 Faker 地址实现，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `443571a8`。按 PHP 反射逐项核对 26 份词表和模板，验证 256 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 258 行，含三语共用示例。回归覆盖亚美尼亚邮编、格鲁吉亚八种布局、地区属格唯一值耗尽及地区边界。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、110 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对塞尔维亚和黑山 Address。

## 塞尔维亚和黑山地址

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [塞尔维亚默认](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sr_RS/Address.php)、[西里尔](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sr_Cyrl_RS/Address.php)、[拉丁](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/sr_Latn_RS/Address.php) 和[黑山](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/me_ME/Address.php) Address。逐项相同的词表和模板共享，保留两种塞尔维亚文字、黑山本地词表与带空前缀模板的前导空格。黑山本地坐标沿用已实现的 Coordinates 结构和范围。v0.x 无等价 Faker 地址实现，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `e0962e0b`。按 PHP 反射逐项核对 19 份词表和模板，验证四个地区标识的 512 个固定 PHP 样本。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 516 行，含三语共用示例。回归覆盖默认和显式西里尔、显式拉丁、五位邮编、黑山前导空格及本地坐标范围。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、111 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 36%，磁盘可用 79 GiB。05-04 下一批核对匈牙利 Address，并继续核对地区回退行为。

## 匈牙利地址和地区回退

对应固定 Laravel 的 [Eloquent 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)，对照 FakerPHP v1.24.1 的 [匈牙利 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/hu_HU/Address.php) 与 [Factory 的提供器回退](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Factory.php#L36)。匈牙利保留城市权重 3:2:1、四位邮编、门牌尾随句点、补充地址和本地坐标。capital、bigCity、smallerCity 返回 Result 并支持 UniqueFaker。Rust 在构造 Faker 时核对固定 Address 地区列表。缺少本地 Address 时核心地址字段与布局回退 en_US，模板调用人名时仍按原地区选择 Person。v0.x 无等价 Faker 地址实现，固定 Goravel 的编译式装配沿用现有路径。

隔离基线 `367c0fa2`。PHP 反射核对 14 份匈牙利词表和模板，Address 提供器列表与固定版本目录逐项一致。匈牙利及保加利亚、威尔士、无提供器测试地区共 512 个固定 PHP 样本通过核验。实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force` 写入并按序回读 516 行，含三语共用示例。回归覆盖城市分级、本地补充地址、回退的两种邮编、当地人名及 UniqueFaker 耗尽。Seeder 测试复验 `makeOne`、`makeMany` 和 `lazy`。

Rust 1.94.0 格式、工作区全目标严格 Clippy、113 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归及 `./artisan inspire` 通过。三语示例编译、VitePress 构建、站点根路径和 /docs/ 的 12 项浏览器检查及六项工厂示例检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 与文档依赖。提交前周额度剩余 35%，磁盘可用 79 GiB。下一批核对全部 Address 提供器的公开字段和行为，汇总 05-04 剩余差异。

## Address 提供器公开接口与格式总验收

以 `41df93cb` 为隔离基线，复核固定 [Laravel 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)、[FakerPHP v1.24.1 Address 源码](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)及其 [Factory 回退](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Factory.php#L36)。v0.x 没有等价 Faker 提供器。固定 Goravel 的编译式装配仍由现有 Artisan Seeder 入口覆盖。

- PHP 反射枚举 71 个本地 Address 提供器的 104 种公开方法，去重前共 1,184 个地区与方法组合。Rust 逐项调用均成功。埃及 governorateId 已由身份格式化模块提供，复用地址词表。沙特独立 cityPrefix 在 PHP 因未声明静态属性而报错，OpenRust 保留已记录的回退；完整地址不依赖该入口。
- 对 472 组可从 PHP 的随机单值词表直接提取的字段各抽样 256 次。所有 Rust 值均在对应词表范围内，空词表的 Rust 字符串约定仍适用。
- 格式检查覆盖 446 个地区与核心字段组合。每组先用 64 条固定 PHP 样本验证匹配规则，再检查 128 条 Rust 样本。其中 443 组展开可解析的内部字段，马来西亚的 streetName、streetAddress、address 三组以外层格式核对，内部城镇、州及邮编关联沿用独立验收。随机流及样本权重的全量统计不在本次核对范围内。
- 05-04 后续先补 PhoneNumber 仍缺少的地区辅助方法，再复查 Payment 和其他提供器。公开方法名清单只提供后续核对线索，方法已存在的地区行为继续以契约和运行证据验证。

真实 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`，写入并按序回读 572 行。覆盖 71 个本地提供器各八行及四个文档示例，其中三条验证回退路径。113 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、Rust 1.94.0 全目标严格 Clippy 和 `./artisan inspire` 通过。三语示例编译、VitePress 构建及两个站点路径共 18 项浏览器检查通过。工作区格式检查在隔离副本补齐既有 Sanctum 排版后通过。临时 Seeder 已恢复，测试容器已回收。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 及文档依赖。提交前周额度剩余 35%，磁盘可用 79 GiB。

## PhoneNumber 公开接口复验与马来西亚 JPJ 车牌

对应固定 [Laravel 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。重新按 PHP 反射及 Rust 编译后的入口核对 [FakerPHP v1.24.1 电话提供器](https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider)。此前按字面搜索遗漏了宏生成的方法。66 个本地 PhoneNumber 提供器的 56 种方法共 296 个地区与方法组合全部可调用。每组 256 个 Rust 样本的数字与非数字排列均出现在固定 PHP 样本的对应组中。此项复验覆盖默认参数与格式排列，前导零和参数变体继续由已有独立验收覆盖。

对照固定 [马来西亚 Miscellaneous](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Miscellaneous.php)，补齐 JPJ 号码和六个组成方法。保留加权模板及前缀、混合大小写、空字母和尾随空格。Rust 使用 Result 表达地区边界，numberSequence 返回 i64，其余组成字段借用静态词表。完整号牌和组成方法均支持 UniqueFaker。v0.x 无等价实现，固定 Goravel 的编译式装配沿用现有入口。

隔离基线 `64adf162`。逐项核对六份 JPJ 词表和模板，验证 256 个固定 PHP 样本。真实 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入并按序回读 257 行，包含一条三语共用示例。回归覆盖号牌格式、空字母和尾随空格、大小写、唯一值耗尽及地区错误。114 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、工作区全目标严格 Clippy 和 `./artisan inspire` 通过。三语示例编译、VitePress 构建及 18 项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。下一批补齐波兰 LicensePlate 的地区及特殊号牌筛选，之后复核 Payment。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 及文档依赖。提交前周额度剩余 35%，磁盘可用 79 GiB。

## 波兰车牌及地区筛选

对应固定 [Laravel 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)与 [FakerPHP v1.24.1 波兰 LicensePlate](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/LicensePlate.php)。保留 16 个常规省级区域、两类特殊号牌、390 条县及机构条目、重复代码权重和两组尾号正则。先筛选省级区域，再在选定区域筛县。筛选为空或没有匹配时从该级可用项抽取。默认排除特殊号牌，两个 Rust 入口共享 unique 历史。Rust 通过专用 WithOptions 方法及借用切片表达 PHP 可选参数，缓存固定尾号正则。PHP Fixture 显式注册 LicensePlate，Rust 由 pl-PL 选择已编译的提供器。v0.x 无相同实现，固定 Goravel 式应用装配由现有 Artisan 入口验收。

隔离基线 `869b3b01`。按 PHP 反射核对全部地区、代码、县名和尾号模板的顺序及重复项。448 个固定 PHP 样本验证七组筛选。实际 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入并按序回读 449 行，包含三语共用示例。覆盖普通号牌、两种县码长度、军用、警务、禁用特殊号牌、空筛选及无匹配回退。115 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy 和 `./artisan inspire` 通过。三语示例编译、VitePress 构建和 18 项浏览器检查通过。已有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。05-04 下一批复核 Payment 的实际地区入口与行为。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 及文档依赖。提交前周额度剩余 34%，磁盘可用 79 GiB。

## Payment 地区回退与奥地利旧别名

对应固定 [Laravel 工厂定义](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)、[FakerPHP v1.24.1 提供器回退](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Factory.php#L36)、[美国 Payment](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_US/Payment.php)及 [at_AT 别名](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/at_AT/Payment.php)。缺少本地 Payment 时，普通账号和路由号选择 en_US 提供器。存在本地 Payment 时按其自身能力提供入口。旧版 at_AT 从 de_AT 继承账号和 VAT，Rust 保留原地区标识，复用相同格式。缺少本地提供器的账号沿用美国无参数契约，Rust 使用显式默认参数并拒绝不适用的覆盖值。v0.x 无等价实现，固定 Goravel 的编译式装配由现有 Seeder 入口覆盖。

按 PHP 反射核对 51 个本地 Payment 提供器的 22 种公开生成方法，434 个地区和方法组合在 PHP、Rust 均可调用。Rust 另复验 en_CA、ja_JP、zz_ZZ 三个回退地区共 27 个组合。美国路由校验助手继续由已提交的 Seeder 回归单独核对。该矩阵验证默认参数的可调用性，信用卡和机构词表等沿用此前独立验收。

隔离基线 `431fcf73`。固定 PHP 8.5 样本 384 组通过，涵盖美国、三个回退地区和两个奥地利标识。实际 MySQL 8.4.13 用 `main artisan db:seed --database=mysql --force` 写入并按序回读 385 行，包含三语共用示例。验证账号长度、美国路由校验、奥地利 IBAN 模 97、VAT 格式、显式参数错误和无路由号的本地提供器。115 项 Faker、49 项 Seeder、两项 MySQL／SQLite 工厂回归、严格 Clippy 和 `./artisan inspire` 通过。三语示例编译、VitePress 构建和 18 项浏览器检查通过。已有 Sanctum 排版差异仅在隔离副本处理。临时 Seeder 已恢复，测试容器已回收。下一批按固定工厂测试、Seeder 和三语交付清单复核 05-04 尚未闭合的契约。

清理本批独占 Cargo 产物 2.3 GiB、站点产物 41 MiB 及文档依赖。提交前周额度剩余 34%，磁盘可用 79 GiB。

## Seeder 批量赋值保护

固定 Laravel 的 [Seeder 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md)及 [SeedCommand::handle](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Console/Seeds/SeedCommand.php#L88)要求填充期间停用模型批量赋值保护。此前 OpenRust 在普通 Seeder 中执行 Eloquent 写入时仍会拒绝受保护的属性。本批在共用 Seeder 入口进入任务局部作用域，子 Seeder 继承该作用域，属性修改器和类型转换照常运行。命令返回后保护恢复，独立启动的 Tokio 任务保留自身状态。核对旧版 `6d1a8192` 的 Seeder 和迁移用例，编译注册与顺序执行沿用现有接线，并参考固定 [Goravel Seeder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/seeder/seeder.go)。

隔离基线 `707c293c`。Rust 1.94.0 格式和全工作区全目标严格 Clippy 通过。49 项 Seeder 回归和两项 MySQL／SQLite 工厂回归通过，覆盖批量生成、关系、状态、回调、连接选择、子 Seeder、命令成功和失败后的保护恢复，以及独立任务仍拒绝受保护属性。实际 `main artisan db:seed --database=mysql --class=GuardedSeeder --force` 在 MySQL 8.4.13 写入子 Seeder 的受保护字段，回读 `GUARDED` 验证修改器生效。重复执行退出 1，原有记录仍为一条。实际编译并运行了三语共用 CatalogSeeder 示例，连续两次执行后回读一条预期记录。`./artisan inspire`、三语 VitePress 构建和根路径及 `/docs/` 的 18 项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。宿主默认 Rust 1.99.1 的附加尝试遇到两条既有 lint，库内验收采用规范指定的 Rust 1.94.0。

对照固定 [工厂测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php)及 [Seeder 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php)。模型选择和依赖获取分别由 `HasFactory` 的静态绑定与 `SeederContext` 的应用容器承接。下一批补齐 Seeder 多类共享参数、静默或仅一次执行时同时传参的语义，并继续核对工厂剩余公开契约。05-04 保持在途。

临时 Seeder 已恢复，测试容器已回收。清理本批独占 Cargo 产物及站点、文档和浏览器测试依赖，磁盘可用恢复至 79 GiB。

## Seeder 共享参数与调用模式

对照固定 [Laravel Seeder 调用实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Seeder.php#L43)、[Seeder 参数测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php#L77)及[调用其他 Seeder](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md#calling-additional-seeders)。Laravel 接受单个类或类列表，并将参数传给每个类，成功后才记入已调用集合。Rust 通过 `callWithMany`、`callSilentWith` 和 `callOnceWith` 表达可选参数组合，保留原有签名。每个子上下文借用同一个只读 Record。调用历史仍属于单次命令，避免独立应用和并发命令共享状态。旧版仅注册单个编译种子函数。固定 [Goravel Seeder](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/seeder/seeder.go) 用于参考编译注册与顺序调用。

隔离基线 `96a404c8`。用 PHP 8.5 运行固定 Seeder 类的调用探针，验证共享参数、仅一次调用跳过已成功类、失败停止余项和重试。同一场景经 OpenRust Artisan 分别在 MySQL 8.4.13 和 SQLite 验证，覆盖静默输出、同类两个名称只调用一次、失败后重试、普通调用仍可再次执行及命令间历史隔离。49 项 Seeder、两项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式和工作区全目标严格 Clippy 通过。隔离副本处理已有 Sanctum 排版差异。

三语共用示例编译进实际应用二进制。MySQL 上两次运行 `main artisan db:seed --database=mysql --force` 均无子 Seeder 进度行，回读一条 `standard:Standard plan`。显式指定 `--class=CatalogSeeder` 显示进度，回读同一行更新为 `standard:Standard`。`./artisan inspire`、三语 VitePress 构建及 18 项浏览器检查通过。临时应用代码已恢复，测试容器已回收。清理本批 Cargo 编译产物、文档站和浏览器测试依赖，磁盘可用 78 GiB。

05-04 继续核对工厂操作的临时属性。固定 Laravel 的 `make`、`create`、`createQuietly` 接受映射或回调作为本次操作的末尾状态，`lazy` 接受映射并在调用时保存。OpenRust 已有 `rawWithState` 和显式父模型入口，仍需核对无父模型时的成批覆盖和普通延迟入口。

## 工厂单次属性与批量生命周期

对照固定 [Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L325)、[工厂测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L193)和[用工厂创建模型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#creating-models-using-factories)。Laravel 将调用传入的属性映射或闭包追加为最后一个状态。Rust 延续 `rawWithState` 的命名，单模型用 `FnOnce`，计数工厂用 `FnMut` 逐条计算。属性映射复用相同路径，父键及延迟属性在覆盖之后解析。计数创建沿用整批构造、逐条写模型与关系、整批触发 `afterCreating` 的顺序。`lazy` 在等待结果时解析连接并执行创建，可重复调用。旧版同名工厂只构造查询，当前 Seeder 沿用参考固定 Goravel 的编译注册接线。

隔离基线 `b69da0a4`。Rust 1.94.0 格式和工作区严格 Clippy、49 项 Seeder 与 2 项 MySQL／SQLite 工厂回归通过。复验单条及批量临时属性、父键覆盖跳过额外写入、重复使用同一工厂、批量回调顺序及 `afterMaking` 失败时未写入目标表。既有 Sanctum 排版差异仅在隔离副本处理。

三语共享示例编译进实际应用，通过 MySQL 8.4.13 上的 `main artisan db:seed --database=mysql --force` 回读八条按序记录。两条由回调创建，两次延迟调用各创建两条，两条由静默创建。普通创建在绑定取消事件时失败，静默创建后再次普通创建仍触发取消。无草稿或失败记录写入。`./artisan inspire`、三语 VitePress 构建、站点根路径及 `/docs/` 的 18 项浏览器检查通过。临时 Seeder 已恢复，容器和独占构建缓存已清理，磁盘恢复至 78 GiB。

05-04 继续核对已显式传入父模型时的末尾回调。当前 `rawWithParent` 支持直接读取父模型和累积属性，`makeWithParent` 及持久化变体直接接收属性映射。

## 带显式父模型的工厂末尾状态

对照固定 [Laravel Factory 的属性累积](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L555)、[工厂回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L193)及[用工厂创建模型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#creating-models-using-factories)。`makeWithParentState`、`createWithParentState`、`createQuietlyWithParentState` 将借用的父模型和当前累积属性交给末尾回调，之后解析延迟属性及关联键。单条使用 `FnOnce`，批量使用 `FnMut` 逐条计算，`On` 变体显式指定连接。仅提供父模型不会填写外键。映射继续用于 `insertWithParent` 和 `lazyWithParent`，对应上游这两个入口接受数组。旧版工厂用于查询构造，固定 [Goravel 工厂](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)提供类型化调用，现有 Seeder 装配可直接承接。

隔离基线 `995377df`。49 项 Seeder、2 项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式和全工作区全目标严格 Clippy 通过。覆盖单条和批量、末尾回调读到配置状态、覆盖延迟父键、无额外父记录、静默创建和模型事件恢复、保留父模型给 `afterCreating` 以及 `lazyWithParent` 重复调用。既有 Sanctum 排版差异仅在隔离副本处理。

三语共用示例编译进应用，通过实际 `main artisan db:seed --database=mysql --force` 在 MySQL 8.4.13 按序回读四行。父记录一条，计算状态生成两条子记录且外键为空，显式输出外键生成一条关联记录。预览没有写入。`./artisan inspire`、三语文档构建及根路径和 `/docs/` 的 18 项浏览器检查通过。临时 Seeder 已恢复，测试容器和独占缓存已清理，磁盘恢复至 78 GiB。

下一步按固定 Laravel 的工厂与 Seeder 测试逐项归档对应的 OpenRust 证据，重点核对动态扩展与工厂发现的 Rust 等价入口，确认 05-04 的整单元边界。

## 工厂发现、扩展和固定用例对照

[用例对照表](test-matrix.md)覆盖固定 Factory 的 70 个命名用例和 Seeder 的五个命名用例，逐组列出 OpenRust 的相应入口、回归和待补组合。以固定 [Laravel 工厂发现与动态扩展](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L713) 为行为索引，Rust 用 `FactoryDefinition::Model` 和 `HasFactory` 在编译期绑定模型，用可导入的扩展 trait 定义可链式调用的方法。旧版 factory 负责查询构造，旧 Seeder 采用编译注册。固定 [Goravel 工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)也由代码提供类型化调用。

隔离基线 `942819b2`。实编译发现 `make:factory --model=User` 生成的代码无法访问默认私有模型模块，且公开工厂会泄露 crate 内模型关联类型。将应用的 User 模块与生成工厂的可见性限定为 crate 内，使生成入口能直接使用应用默认模型。实际生成 `Fleet/Flight`、推断模型的 `Fleet/FlightFactory` 及显式绑定 User 的 `Fleet/RouteFactory`，编译后确认两种工厂模型分别绑定预期 Rust 类型。三语共用扩展示例随应用编译，用真实 `main artisan db:seed --database=mysql --force` 在 MySQL 8.4.13 回读两条 `scheduled`。

生成器定向用例、49 项 Seeder、2 项 MySQL／SQLite 工厂回归、Rust 1.94.0 格式及工作区严格 Clippy、`./artisan inspire`、三语站点构建和 18 项浏览器检查通过。既有 Sanctum 排版差异仅在隔离副本处理。生成与装配探针保留在隔离目录，临时应用接线已恢复，测试容器和独占缓存已清理。

05-04 仍待补验表中列出的组合，先用实际 MySQL 与固定上游验证 JSON Pivot 的共享和逐条映射，再核对关系默认属性及覆盖顺序。

## JSON Pivot 与子关系默认属性

对照固定 Laravel [JSON Pivot 用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L568)、[关系默认属性用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L1067)及[多对多工厂关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#many-to-many-relationships)。Rust 用 `Value::Json` 传递结构化值，`Value::Null` 表达 SQL NULL，自定义 Pivot 由 `withPivot` 选择属性并声明转换。通过 `FactoryChild` 和具名关系装配继续复用现有默认属性、显式状态和关系键的顺序。旧版 `pivot_writes.rs` 已有类型化中间表写入，工厂仍属查询构造。Goravel 的[类型化工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)作为装配参考。

隔离基线 `6e029173`。MySQL 8.4.13 回归验证共享 JSON、逐条属性推断数量、嵌套 JSON、SQL NULL、自定义 Pivot 转换和重新加载。MySQL 与 SQLite 工厂回归新增 HasOne 和 MorphMany 各自的默认值与覆盖路径，核对最终外键及多态标识。49 项 Seeder、两项双库工厂回归、Rust 1.94.0 格式及全工作区全目标严格 Clippy 通过。既有 Sanctum 格式差异仅在隔离副本处理。

实际 `main artisan db:seed --database=mysql --force` 编译并运行三语共用示例。MySQL 按关联键回读四条中间表行，第一组使用逐条映射得到 SQL NULL 和 editor，第二组两条均为 reader。自定义 Pivot 回读与 SQL 一致。`./artisan inspire`、三语站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。临时 Seeder 已恢复，容器与本批缓存已清理，磁盘可用 78 GiB。

[用例对照表](test-matrix.md)已更新。下一批核对具名工厂配置回调的移除、推断子关系配合逐条工厂状态，以及回收池不重绑已有子模型。

## 配置回调、推断关系序列及回收池

对照固定 Laravel 的[配置回调移除](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L321)、[推断关系的逐条状态](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L791)、[回收池与子模型归属](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L881)，以及[工厂关系文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#factory-relationships)。Rust 以 `FactoryDefinition::configure` 装配回调，将类型化子工厂的 `forEachSequence` 交给 `hasInferred`，并通过按类型的 `recycle` 与 `recycleMany` 维护候选池。旧版同名工厂用于查询，固定 [Goravel 工厂契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)作为编译期装配参考。

隔离基线 `73575b78`。MySQL 8.4.13／SQLite 验证具名工厂移除所选阶段的回调、新构建器仍执行原配置、关系按输入顺序写入，以及回收池按类型选择和空池回退。父工厂新建子模型后，原有子模型继续指向原父键。49 项 Seeder、两项双库工厂、Rust 1.94.0 格式及全工作区全目标严格 Clippy 通过。既有 Sanctum 排版差异仅在隔离树处理。

实际 `main artisan db:seed --database=mysql --force` 运行三语共用示例。MySQL 按键回读 4 条父记录和 8 条子记录。前两条子记录指向各自的父记录，后两个父模型各有按 alpha、beta、gamma 排列的子记录。父记录值验证移除指定阶段后的写入结果。`./artisan inspire`、三语站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。临时 Seeder 已恢复，容器和本批独占缓存已清理，磁盘可用 78 GiB。

[用例对照表](test-matrix.md)中的工厂与 Seeder 待补组合已有对应证据。05-04 仍需核对 FakerPHP 兼容边界。其中固定 [FakerPHP Generator::seed](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php) 可重置随机序列，现有 Rust Faker 没有该公开入口。下一批验证同种子可复现、不同种子及独立应用的作用域，再更新 Faker 验收状态。

## Faker 随机种子与应用隔离

对照固定 [FakerPHP Generator::seed](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php#L685)和 [Laravel 工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。固定 PHP 8.5 探针运行该方法，重复种子重放相同整数序列，更换种子产生不同序列。Rust 让每个 Faker 实例持有随机序列，以隔离不同应用、语言及独立生成器。工厂仍从所选连接所属应用获取单独绑定的 Faker。区域词表、正则及 fake-rs 回退都从该序列取样。Rust RNG 与 PHP MT 产生的具体值有差异。尚未设种子时走原有随机路径，显式设种子后只在抽样时同步访问该实例的随机状态。跨任务共享实例的结果顺序取决于调度，重放依赖相同调用顺序及输入，带相对时间的格式还依赖时钟。文件取样在设种子后按文件名排序，下载临时文件继续独立生成名称。旧版同名工厂为查询构造，Goravel 式编译装配无需改动。

隔离基线 `531f60ee`。10 个 locale 的混合 formatter 回归验证重设相同种子、不同种子、实例交错、默认熵重置入口及单独的 uniqueness 历史。两次同进程 Artisan 应用调用验证语言别名共用序列、不同语言和应用各自推进。Rust 1.94.0 格式、全工作区全目标严格 Clippy、50 项 Seeder 与两项 MySQL／SQLite 工厂回归通过。既有 Sanctum 排版差异仅在隔离副本处理。

实际 `main artisan db:seed --database=mysql --force` 在 MySQL 8.4.13 运行三语共用示例。两次独立设种子写入六条记录，三对位置对应的名称逐字节一致。`./artisan inspire`、三语站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。临时 Seeder 已恢复，容器和本批缓存已清理，磁盘可用 78 GiB。

下一批按固定 FakerPHP 的自定义 provider、formatter 重名覆盖和错误路径复核 Rust 扩展入口，完成 05-04 剩余兼容边界对照。

## 05-04 恢复复验：预构造与延迟写入

隔离基线 `185501c1`。按固定 Laravel 的 [Factory 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L357)及[使用工厂创建模型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#creating-models-using-factories)复验已提交的入口。固定 Goravel 继续作为编译装配参考。v0.x 同名代码用于构造查询。三语示例同时取得单条和批量回调的 Future，再等待其完成并检查保存状态。Laravel 在调用闭包时同步写入，Rust 在等待 Future 时开始异步写入。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 验证预构造不写入目标表，`makeOne` 保留批量数量，两次默认 `makeMany` 依次返回两条和一条，逐条覆盖保留顺序，显式零数量返回空集合。调用两种 `lazy` 回调并取得 Future 后仍为零行。两次批量与两次单条执行后按主键核对四条批量与两条单条记录。三语共享示例在同一真实 Seeder 内运行，再新增六条。

Rust 1.94.0 全工作区全目标严格 Clippy、50 项 Seeder、MySQL 和 SQLite 各一项工厂回归、`./artisan inspire`、三语示例独立编译、VitePress 构建及 18 项浏览器检查通过。基线既有 Sanctum 测试格式差异仅在隔离副本处理后通过格式检查。临时 Seeder 已恢复，临时 MySQL 容器已回收。清理隔离 Cargo 产物 4.9 GiB、站点产物及独占文档依赖。提交前周额度剩余 32%，项目卷可用 78 GiB。Faker 自定义 provider、同名格式化器覆盖及错误路径继续在 05-04 跟踪。

## Faker 自定义提供器和具名格式化

对照固定 Laravel [模型工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)和 FakerPHP v1.24.1 [Generator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php#L600)。PHP 8.5 探针验证后注册提供器优先、大小写无关、旧回调保留、生成器方法优先、参数传递、模板展开及未知名称。旧版查询工厂没有 Faker 提供器。固定 Goravel 继续作为编译式装配参考。

Rust 通过 `FakerProvider::formatter` 注册线程安全的 Rust 回调，`addProvider` 将一组回调写入当前生成器。调用在释放注册表读锁后执行，可继续调用同一生成器。具名 `format` 和 `parse` 共用解析路径，回调接收类型化 `Value` 参数，错误经 `DatabaseResult` 传播。已有类型化内置方法保留直接调用。其中台湾英语公司模板会从同一生成器查找 `lastNameEn`，无提供器时保持原有错误，回调失败或返回非文本时返回错误。

隔离基线 `fbf00650`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、52 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。固定版 Sanctum 测试格式差异仅在隔离副本处理。两次独立应用的 Artisan 测试复核同一应用的语言别名共享注册，其他应用及语言独立。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 编译并运行三语共用示例，回读一行 `account-demo`。替换为返回错误的姓氏回调后，同一二进制报告原错误并以状态 1 退出。`./artisan inspire`、三语 VitePress 和 18 项浏览器检查通过。

临时 Seeder 已恢复，测试容器回收，清理隔离 Cargo 产物 5.0 GiB 和独占站点产物及依赖。提交前周额度剩余 32%，项目卷可用 78 GiB。具名入口当前支持三语文档列出的内置项和已注册回调。其余内置 formatter 的具名参数适配、动态查找与修饰器组合及 PHP 扩展名解析仍待核对。

## Faker 具名 formatter 的去重与修饰器

对照固定 Laravel [工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)和 FakerPHP v1.24.1 的 [UniqueGenerator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/UniqueGenerator.php#L63)、[ChanceGenerator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/ChanceGenerator.php#L48)及 [ValidGenerator](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/ValidGenerator.php#L59)。PHP 8.5 探针验证了按调用拼写保存历史、值的类型差异、重置、跳过 optional 的方法解析，以及 unique 和 valid 到达上限时的取样次数。v0.x 的同名工厂属于查询构造，固定 Goravel 工厂实现继续作为编译式装配参考。

Rust 复用现有的重试循环，按调用拼写维护具名历史，并用带类型的样本键保留整数、文本、字节及 JSON 结构。参数变化和重新注册提供器继续使用同一拼写的历史。查找和生成在去重锁外执行，结果在锁内登记。类型化调用继续使用自己的历史，重置同时清空两类历史。具名调用可通过已有的闭包式 `optional` 和 `valid` 传入，回调错误直接传播。

隔离基线 `161446a7`。Rust 1.94.0 格式及全工作区全目标严格 Clippy、54 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。并行调用竞争固定 JSON 值时仅一次登记成功。基线既有 Sanctum 测试排版差异只在隔离副本处理。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 编译并执行三语共用示例，按主键回读 `demo`、`skip`、`demo`。重复样本耗尽后以状态 1 退出，保留错误。另通过 `./artisan inspire`、VitePress 构建和 18 项浏览器检查。临时 Seeder 已恢复，测试容器已回收，清理隔离 Cargo 产物 5.0 GiB 与独占文档构建产物及依赖。提交前周额度剩余 32%，项目卷可用 78 GiB。

05-04 待接具名内置函数的参数适配，以及 FakerPHP 命名空间扩展解析和修饰器调用。

## Faker 数字与模式 formatter 的具名参数

对应固定 Laravel [工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。对照 FakerPHP v1.24.1 的 [Generator 数字入口](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php#L869)、[Number](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/Number.php)、[基础模板提供器](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L437)和 Address、Miscellaneous 提供器。v0.x 的同名实现用于查询构造，固定 Goravel 工厂契约继续作为编译装配参考。

具名调度复用 Rust 的数字、坐标和替换模板实现。生成器自有数字方法优先于同名提供器，提供器默认方法仍可覆盖。`format` 和 `getFormatter` 使用同一套位置参数处理，省略值采用固定版默认值，显式空值按方法处理。Rust 校验参数类型、整数转换范围与有参数方法的最大参数数量，通过 `DatabaseResult` 返回失败。PHP 8.5 探针经固定 Generator 实际调用，核对反向区间、严格位数、非零数字、默认参数、空值、生成器优先级、格式模板与位数错误。

隔离基线 `6e2f9d08`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、55 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 执行三语共用示例并回读 `7`、`XX`。传入文本作为区间端点时报告类型错误并以状态 1 退出。`./artisan inspire`、VitePress 构建和 18 项浏览器检查通过。

临时 Seeder 已恢复，测试容器已回收，隔离 Cargo 产物 5.0 GiB 和独占文档构建产物及依赖已清理。提交前周额度剩余 32%，磁盘可用 78 GiB。05-04 下一批核对限定扩展解析、扩展修饰器以及尚未接入的其他具名内置 formatter。

## Faker 限定扩展名及修饰器组合

对照固定 Laravel 的工厂数据生成章节及固定上游 Faker 的 Generator::getFormatter、Generator::ext 和 ContainerBuilder 默认扩展映射。v0.x 查询工厂没有 Faker 扩展，编译式装配沿用固定 Goravel 工厂契约。PHP 8.5 探针运行固定 Generator 和数字扩展，验证限定名参数、方法大小写、扩展 ID 区分大小写、未知扩展和缺少方法。使用固定修饰器复验限定名称的 unique、optional 和 valid 路径。

Rust 在生成器实例内注册扩展，由 ext 返回已绑定句柄。限定名称直接查扩展，未知扩展和未知方法分别通过 DatabaseResult 返回。具名生成器方法调用对应扩展，已有类型化方法保留直接调用。已解析回调和扩展句柄保留取得时的实现，下一次按名称调用可看到新注册。Rust 的具名去重继续按完整名称保存历史，上游 PHP 代理按被调用的方法名称保存历史。三语正文统一上游称谓，保留来源链接。

固定上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Generator.php#L715 和 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Container/ContainerBuilder.php 。

隔离基线 `416f3b46`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、56 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 执行三语共用示例，回读 `demo` 和 `account-demo`。未知扩展报告原标识并以状态 1 退出。`./artisan inspire`、VitePress 构建和 18 项浏览器检查通过。基线 Sanctum 格式差异只在隔离副本处理。

临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 5.0 GiB 和独占文档构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。已接 Barcode、Blood、Color、Number、Uuid 及 File 的 MIME 和扩展名方法。DateTime、Version、File 的 filePath 及其他内置方法的具名适配仍待对账，05-04 先继续核对序列闭包的属性与父模型上下文。

## 工厂序列读取当前属性与父模型

固定 Laravel Sequence::__invoke 将当前索引和长度、状态累积属性及可选父模型传入所选闭包，成功返回后推进索引。PHP 8.5 探针运行固定 Sequence 源码，复验数组和闭包交替、当前属性、父模型、索引和序列长度。v0.x 同名工厂用于查询构造，既有编译式装配沿用固定 Goravel 工厂参考。

Rust 以 sequenceWithState 和 sequenceWithParent 延续类型化闭包接口。序列在其状态声明位置读取已合并属性，后续状态、显式调用覆盖与延迟属性仍按各自阶段处理。父模型通过现有类型化借用传入，类型不匹配或缺失时为 None。闭包无可恢复错误返回，原子递增在调用前分配索引。普通序列、属性映射序列与交叉组合继续共用原来的索引和批次数量机制。PHP 可在一个序列中混合映射和闭包，Rust 的闭包按索引选择下一组属性，映射条目的批次长度由 forEachSequence 设定。

固定源码 https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Sequence.php#L55 及 https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L518 。

隔离基线 `eb3f1ae8`。56 项 Seeder、115 项 Faker、MySQL／SQLite 各一项工厂回归和全工作区全目标严格 Clippy 通过。双库验证直接父模型、HasMany 和 BelongsToMany 上下文、前序属性、后续覆盖、连续索引及批次数量。Rust 1.94.0 格式检查通过，基线 Sanctum 格式差异仅在隔离副本处理。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 编译并执行三语共用示例，按主键回读 `owner`、`item-0-parent`、`item-1-parent`。Artisan inspire、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 5.0 GiB 与独占文档构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。05-04 继续对账剩余 Faker 扩展和工厂序列条目组合。

## 工厂混合序列条目

对照固定 Laravel Sequence::__invoke 和 Factory::forEachSequence。PHP 8.5 探针运行固定 Sequence，读取混合映射和闭包，并核对重复周期、递增索引、父模型和条目计数。v0.x 的查询工厂没有此机制，固定 Goravel 继续作为编译式装配参考。

Rust 以 FactorySequence 表达静态映射和类型化闭包。sequenceEntries 保留原批次设置，forEachSequenceEntries 按条目数设置批次。回调读取绝对索引、条目数、当前属性和相应类型的可选父模型。条目在原状态顺序点执行，计数工厂复用同一状态。空条目不追加状态，计数入口产生零条；这沿用 Rust 现有空映射序列的行为。Laravel 的序列对象由回调接收，Rust 提供只读的位置与条目数量，并在并行运行前分配索引。

固定源码 https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Sequence.php#L55 和 https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L636 。

隔离基线 `7bd5a0b0`。MySQL 与 SQLite 验证预构造、持久化和原始属性调用连续推进，静态与回调条目轮换，空列表保留或清零批次。Rust 1.94.0 格式和工作区全目标严格 Clippy、56 项 Seeder、115 项 Faker、MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 执行三语共用示例，按主键回读 `owner`、`fixed`、`1-parent`。Artisan inspire、VitePress 及 18 项浏览器检查通过。临时 Seeder 已恢复，测试容器已回收，清理隔离 Cargo 产物 5.0 GiB 和独占站点构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。05-04 下一批回到 Faker DateTime、Version、File 扩展及具名 formatter 剩余对账。

## Faker 日期、版本与临时文件扩展

对照固定 Laravel 的工厂数据生成和上游 Faker v1.24.1 的 DateTimeExtension、Core/Version 和 Core/File。PHP 8.5 探针使用实际固定类，确认纪元日期、格式标记、显式时区、版本号边界及两类元数据形状和空临时文件生命周期。v0.x 只有查询工厂，固定 Goravel 仍作为编译装配参考。

Rust 复用现有 PHP 日期格式化、时间上界解析和时区能力。新增具名日期与时间、Unix 秒、日期部件和时区调用。具名 dateTime 与 dateTimeAD 使用 RFC 3339 文本承载时间及偏移。SemVer 根据两个布尔标记选择是否生成预发行与构建信息，沿用当前生成器的 RNG。filePath 用已有 tempfile 原子创建空文件，成功后将路径交给调用方，路径保留至调用方删除，文件系统错误保留原因。具名方法只返回可表示为 UTF-8 的路径。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/DateTime.php 、https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/Version.php 和 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/File.php 。

隔离基线 `387bac4e`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、57 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。以固定种子验收版本形状和标记、纪元日期和显式时区、真实空文件及参数错误。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 编译运行三语共用示例，回读纪元日期和版本号，并验证空文件创建和删除。无效时区报错且状态为 1。`./artisan inspire`、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已移除，清理隔离 Cargo 产物 5.0 GiB 和站点构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。DateTimeExtension 的日期区间、相对周期以及其他具名 formatter 待继续对账。

## Faker DateTime 区间与相对周期

对应固定 Laravel 工厂数据生成和固定上游 Faker DateTimeExtension。PHP 8.5 探针运行固定 Core/DateTime、Number 和 Generator，验证等端点、可用时区、周一、本月首日、年初及十年和百年边界，以及反向端点报错。v0.x 的同名工厂用于查询构造，固定 Goravel 继续作为编译装配参考。

具名 DateTimeExtension 接入 dateTimeBetween、dateTimeInInterval 及本周、本月、本年、十年和百年范围。以同一参考时间解析范围端点，端点按应用时区解释。区间复用现有相对时间解析，可向过去取样。输出 RFC 3339 文本，显式时区用于渲染偏移。具名周期跟随上游扩展的日历边界，既有类型化周期继续沿用其对应的旧提供器行为。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Core/DateTime.php#L86 。

隔离基线 `4241f60f`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、58 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。

实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 执行三语共用示例，回读 `1970-01-01T08:00:00+08:00` 和 `1970-01-01T00:00:00+00:00`。反向端点报告错误并以状态 1 退出。Artisan inspire、VitePress 及 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 5.0 GiB 及站点构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。05-04 后续继续核对集合等剩余具名 formatter 与整单元账本。

## Faker 集合抽样和洗牌的具名参数

固定 Laravel 的工厂数据生成使用上游 Faker。对照固定 v1.24.1 Base 的 randomElement、randomElements、randomKey、shuffle、shuffleArray、shuffleString 和 passthrough。PHP 8.5 探针用实际 Base 验证默认和空集合、关联键、重复项、超量错误及带 mbstring 的 UTF-8 洗牌。v0.x 的工厂属于查询构造，固定 Goravel 继续作为编译装配参考。

Rust 在已有类型化生成器上接入具名 Value 参数。JSON 对象保留文本键，数组索引以整数返回，多项抽样和洗牌返回 JSON 列表。文本洗牌按 Unicode 码点工作，Rust 字符串输入为 UTF-8。具名调用不接受 PHP 枚举类名或 Traversable 对象，调用方先提供 JSON 元素。重复抽样的分配容量错误转为 DatabaseResult。整单元仍需核对尚未接入的具名方法和剩余兼容边界。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Base.php#L193 。

隔离基线 `cbb575da`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、59 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 执行三语共用示例，回读文本键 `score` 和 JSON `[9,9]`。不允许重复时超量抽样报告错误并以状态 1 退出。

Artisan inspire、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 6.4 GiB 与独占文档构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。闭环账本保留 05-04 待完成状态，下一批按上游接口与 Rust 生成器对照补齐剩余具名 formatter。

## Faker 文本、网络与 User Agent 的具名参数

对照固定 Laravel 工厂数据生成章节和上游 Faker v1.24.1 的 Lorem、Text、Internet 与 UserAgent。PHP 8.5 探针实际运行固定 Lorem 和 Internet，核对省略和空值、固定词数、列表和文本形式、密码长度、IP 格式及短文本错误。v0.x 同名工厂仅用于查询构造，固定 Goravel 继续作为编译装配参考。

| 接口组 | 本批具名接口 | 验收边界 |
| --- | --- | --- |
| Lorem 和 Text | word、words、sentence、sentences、paragraph、paragraphs、text、realText、realTextBetween | 复用类型化生成器，位置参数校验后传入。列表经 JSON 数组返回，短文本和不可分配的词数返回错误。 |
| Internet | password、slug、companyEmail、safeEmailDomain、freeEmailDomain、domainWord、domainName、tld、url、ipv4、ipv6、localIpv4、macAddress | 复用经地区验收的生成器。固定长度和名称覆盖在具名入口复验。 |
| UserAgent | userAgent、firefox、chrome、msedge、safari、opera、internetExplorer、macProcessor、linuxProcessor、windowsPlatformToken、macPlatformToken、linuxPlatformToken、iosMobileToken | 具名调度复用同一 RNG 和类型化生成逻辑。 |
| 剩余接口 | 地址与姓名的地区专用方法、支付与身份、图像和其他方法 | 既有类型化证据不能代替具名调用证据，05-04 仍待逐组对账。 |

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Lorem.php 和 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Internet.php 。

隔离基线 `963339f4`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、60 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 运行三语共用示例，回读两个单词的句子和 slug。短文本报错并以状态 1 退出。

Artisan inspire、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 6.4 GiB 及站点构建产物。提交前周额度剩余 31%，磁盘可用 78 GiB。

## Faker 地址、姓名和支付的地区具名调度

对照固定 Laravel 工厂数据生成及上游 Faker v1.24.1 的 Address、Person、Payment 和地区提供器。PHP 8.5 实跑日本都道府县、马来西亚州邮编、俄语女性姓氏、信用卡分隔符、德国 IBAN 账户前缀及英国政府 VAT。固定法国 Company 的 SIREN 默认格式保留分组空格。v0.x 同名工厂为查询构造，编译式装配仍参考固定 Goravel。

本批维护 151 项地区具名映射，其中 147 项新增，姓名、名字、姓氏和邮编四项接入已有的具名入口并识别对应参数。Rust 复用类型化地区和回退规则，复杂结果用 JSON 表达，日期对象为 RFC 3339 文本，具名到期日期字符串沿用 PHP 格式标记。只有具名调用增加参数适配，已公开的类型化方法继续使用原契约。自定义提供器仍可覆盖具名默认方法。地区方法的缺少、参数校验和原生成器错误通过 DatabaseResult 传播。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Address.php 、https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Person.php 和 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Payment.php 。

隔离基线 `17c4f1e6`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、61 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 运行三语共用示例，回读日本都道府县和德国 IBAN。不具备对应地址提供器时报告原错误并以状态 1 退出。

Artisan inspire、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 7.7 GiB 及站点构建产物。提交前周额度剩余 30%，磁盘可用 78 GiB。05-04 下一批继续图像、身份校验及地区方法参数差异的具名对账。

## Faker 具名图像与身份校验

对照固定 Laravel 的工厂数据生成和上游 Faker v1.24.1 的 Image、捷克 Person、波兰 Person 等地区提供器。PHP 8.5 运行固定提供器，核对 PESEL 闰日编码、捷克出生号分隔符、图像参数、UTF-8 URL、格式码和非法格式。v0.x 同名工厂用于查询构造，应用编译装配继续参考固定 Goravel。

具名入口接入 60 项同步映射以及异步 image 下载。身份方法复用已有地区实现和校验，使用文本表达民用日期，拒绝非法日期，保留各地区参数顺序。图像 URL 和格式目录保持同步。formatAsync 复用既有 HTTP 下载实现，保留自定义提供器覆盖。远端失败返回空值，本地 I/O 错误保留原错误。图像 URL 验收使用固定上游字符串，本地 HTTP 传输成功和中断由下载回归验证。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/Image.php 和 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/Person.php 。

隔离基线 `066f2650`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、62 项 Seeder、115 项 Faker、MySQL／SQLite 各一项工厂回归和一项本地 HTTP 图像下载回归通过。基线 Sanctum 排版差异仅在隔离副本处理。MySQL 8.4.13 实际 `main artisan db:seed --database=mysql --force` 运行三语共用示例并回读样本身份号及占位 URL。实际异步入口拒绝不支持的图像格式并以状态 1 退出。外部占位站点的实时可用性未纳入验收。

Artisan inspire、VitePress 和 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 6.7 GiB 及站点构建产物。提交前周额度剩余 30%，磁盘可用 78 GiB。05-04 继续审计地区专用入口的参数矩阵，以及非图像的文件复制等具名接口。

## Faker 具名文件复制与地区参数对账

对照固定 Laravel 模型工厂数据生成和上游 Faker v1.24.1 的 File、马来西亚 Person、罗马尼亚 Person、台湾 Company、英国 Company 和美国 Payment。PHP 8.5 运行固定类，核对文件内容、扩展名、完整路径与文件名、同目录拒绝、MyKad 字符分隔符、CNP 地区参数、台湾 VAT 返回类型及两个支付校验值。v0.x 同名工厂用于查询，继续参考固定 Goravel 的编译式装配。

具名 file 复用既有的可读文件选择与复制，省略目录时使用系统临时目录。源和目标通过规范路径判定，经 FakerIo 传播 I/O 原因。路径对外使用 UTF-8。复制后的文件名无法转换时移除该副本。保留既有的 UUID v3 名称与排他创建，固定种子重放时遇到已有目标会选择另一个文件名。新增 MyKad、CNP、电话号码国家前缀、校验值、摘要和 HTML 生成的具名入口。台湾 VAT 的具名结果与上游一样为整数。英国 modulus 97 的具名结果保留两位字符串，既有类型化方法继续返回数值。

上游参考 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/File.php#L576 及 https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Person.php#L775 。

隔离基线 `af6513da`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、63 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。基线 Sanctum 排版差异仅在隔离副本处理。真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 运行三语共用示例，比对复制的二进制内容与生成扩展名，读回保存的文件名。同目录报错并以状态 1 退出。

Artisan inspire、VitePress 及 18 项浏览器检查通过。临时 Seeder 已恢复，容器已回收，清理隔离 Cargo 产物 5.1 GiB 和站点构建产物。提交前周额度剩余 30%，磁盘可用 78 GiB。05-04 下一批复核地区联系电话和其他仍只有类型化入口的公共 formatter。

## 05-04 预构造与延迟写入复验（2026-11-15）

隔离基线 `db39007a`。沿用固定 [Laravel Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Factories/Factory.php#L357)及[使用工厂创建模型](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#creating-models-using-factories)。固定 [Goravel 工厂](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/factory.go)用于参考编译式装配。v0.x 同名工厂用于构造数据库查询。Laravel 的 `lazy` 闭包在调用时同步保存，Rust 在返回的 Future 开始轮询时运行异步创建。三语示例补上直接丢弃未轮询 Future 的用法。

实际 MySQL 8.4.13 执行 `main artisan db:seed --database=mysql --force`。验证 `makeOne` 保留配置数量，两次默认 `makeMany` 分别返回两条和一条，显式属性保留顺序且零数量返回空集合。预构造、丢弃未轮询的 Future，以及取得单条和批量 Future 后，目标表均为零行。两次批量和两次单条执行后，按主键读取四条批量与两条单条记录。三语共用示例在实际 Seeder 中再次运行并写入六条，合计十二条。

Rust 1.94.0 全工作区全目标严格 Clippy、63 项 Seeder 测试、MySQL 和 SQLite 各一项工厂回归以及 `./artisan inspire` 通过。既有 Sanctum 测试的格式差异只在隔离副本调整，调整后格式检查通过。三份示例独立编译，VitePress 构建和 18 项浏览器检查通过。临时 Seeder 已恢复，MySQL 容器已回收。清理本批隔离 Cargo 产物 2.3 GiB 和独占站点产物及依赖。提交前周额度剩余 30%，项目卷可用 66 GiB。05-04 的剩余地区电话号码及具名 Faker 接口继续在本记录跟踪。

## Faker 地区电话号码具名调度

对应固定 Laravel 的[工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。FakerPHP v1.24.1 的 PhoneNumber 提供器共有 55 个不区分大小写的公开名称。逐项与具名调度表核对，本批新增 50 个入口，既有 5 个入口仍由原位置处理。固定 Goravel 工厂继续作为编译装配参考。v0.x 同名工厂及测试面向查询构造。

具名入口复用现有地区生成器，涵盖移动电话、座机、分机、服务号码、区号及模板组件。马来西亚三类号码按固定上游顺序接收国家前缀和格式化两个布尔值，巴西五种入口接收一个格式化布尔值。显式 null 关闭选项。Rust 按地区签名验证这些参数的类型和数量，通过 `DatabaseResult` 报告错误。其他地区是否提供对应方法继续由已验收的类型化生成器决定。

PHP 8.5 实跑固定 [马来西亚](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/PhoneNumber.php)、[巴西](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pt_BR/PhoneNumber.php)、[加拿大](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_CA/PhoneNumber.php)、[新加坡](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/en_SG/PhoneNumber.php)和韩国提供器，验证默认与显式选项、加拿大继承的分机、韩国方法大小写、新加坡三种前缀形式及缺少地区入口时的错误。

隔离基线 `885f7dbf`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、64 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。既有 Sanctum 排版差异仅在隔离副本处理。实际 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 运行三语共用示例并按插入顺序回读马来西亚紧凑手机号码、加拿大带分机号码。美国地区调用法国专用入口报告原错误并以状态 1 退出。`./artisan inspire`、VitePress 构建及根路径和 `/docs/` 的 18 项浏览器检查通过。

临时 Seeder 已恢复，测试容器已回收。清理隔离 Cargo 产物 2.4 GiB 和独占站点产物及依赖。提交前周额度剩余 29%，项目卷可用 60 GiB。05-04 整单元继续核对地区地址和执照号码等其他具名接口。

## Faker 台湾地址组件与地区车牌具名调度

对应固定 Laravel [工厂数据生成](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md#defining-model-factories)。固定 Goravel 工厂继续作为编译装配参考。v0.x 同名工厂及测试用于构造数据库查询。对照 FakerPHP v1.24.1 的[台湾 Address](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/zh_TW/Address.php)、[马来西亚 Miscellaneous](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/ms_MY/Miscellaneous.php)与[波兰 LicensePlate](https://github.com/FakerPHP/Faker/blob/v1.24.1/src/Faker/Provider/pl_PL/LicensePlate.php)，补齐 13 个具名入口。

Rust 复用已有类型化生成器。台湾具名坐标只按对应地区提供器开放，保留原类型化坐标方法的行为。波兰车牌通过 locale 选择提供器，PHP 需额外注册 LicensePlate。三个位置参数依次为布尔值和两个可空的名称集合。JSON 数组或对象均按值读取，Rust 要求名称是字符串。未给筛选或匹配为空时使用该层全部可用项。马来西亚 JPJ 的主入口和六个编号或前缀等组件均可按名称调用。

PHP 8.5 用固定提供器分别验证 32 轮台湾坐标与数字范围、JPJ 生成与数字序列、波兰省县筛选、军用选项、无匹配回退和布尔参数错误。

隔离基线 `cb373e0c`。Rust 1.94.0 格式、全工作区全目标严格 Clippy、65 项 Seeder、115 项 Faker 和 MySQL／SQLite 各一项工厂回归通过。既有 Sanctum 排版差异只在隔离副本调整。真实 MySQL 8.4.13 `main artisan db:seed --database=mysql --force` 运行三语共用示例，按插入顺序回读台湾纬度、马来西亚车牌和波兰指定县车牌。非字符串筛选项报告参数错误并以状态 1 退出。`./artisan inspire`、VitePress 构建和 18 项浏览器检查通过。

临时 Seeder 已恢复，MySQL 容器已回收，清理本批隔离 Cargo 产物 2.4 GiB 及独占站点产物和依赖。提交前周额度剩余 29%，项目卷可用 54 GiB。05-04 仍需核对地区 Person、Company 等具名入口。
