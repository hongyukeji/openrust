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
