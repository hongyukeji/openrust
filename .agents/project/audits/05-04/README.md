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
