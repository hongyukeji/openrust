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
