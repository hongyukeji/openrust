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
