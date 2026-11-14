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
