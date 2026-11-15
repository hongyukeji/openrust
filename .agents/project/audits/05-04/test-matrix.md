# 05-04 固定 Laravel 工厂与 Seeder 用例对照

基线为 Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc`。下表按源码行号覆盖 Factory 测试文件中 70 个命名用例及 Seeder 文件中的五个命名用例。Rust 证据来自[工厂回归](../../../../vendor/openrust/framework/tests/eloquent_factories.rs)、[Seeder 与工厂集成测试](../../../../tests/seeding.rs)、[生成器回归](../../../../tests/startup.rs)和[实际请求与命令记录](README.md)。状态描述 Rust 已实测的范围，待补的组合单独列出。

## Factory

| 固定用例起点 | 用例数 | OpenRust 对应及验收边界 |
| --- | ---: | --- |
| [基础单条与多条创建](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L112) | 1 | 双库工厂回归与 `factory_many_operations_consume_the_configured_count` 涵盖默认值、调用覆盖、单条与多条数量。 |
| [闭包属性及返回工厂的闭包](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L167) | 2 | `factory_deferred_attributes_resolve_in_order_on_mysql` 用 `attribute` 和 `parentAttributeWith` 验证依赖顺序、晚覆盖、创建上级和错误中止。PHP 属性输入可混合类型，Rust 通过类型化入口分别声明。 |
| [未保存模型、原始属性、懒创建与零数量](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L193) | 7 | 双库工厂回归、`factory_raw_overrides_are_temporary_and_resolve_parents_after_state`、`factory_many_operations_consume_the_configured_count` 及实际 Seeder 探针覆盖。Rust 单模型和计数工厂使用不同的静态结果类型。 |
| [工厂回调和移除回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L280) | 5 | 双库回归验证批量阶段与失败前未写入，`PolymorphicFactories` 验证移除回调、重新注册及静默创建。具名工厂的 `configure` 回调已通过单条和批量实测，分别检查移除一阶段后的落库值、另一阶段的返回值以及新构建器的行为。 |
| [子关系、已有父模型、多态和中间表](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L345) | 12 | 双库工厂回归和 `artisan_seeder_persists_polymorphic_factory_relationships_on_mysql` 覆盖生成与关联。 `factory_model_keys_and_relation_cache_follow_lifecycle_on_mysql` 与 `factory_named_custom_pivot_reloads_touched_members_on_mysql` 覆盖加载缓存、已有模型数组与触碰。关系名使用显式描述符或 `*Named`、`*Inferred` 变体。 |
| [JSON Pivot 和逐条 Pivot 属性](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L568) | 2 | MySQL 自定义 Pivot 回归及实际 Artisan 三语示例验证共享 JSON、逐条 JSON 和 SQL NULL。无显式数量时按映射条数生成模型，回读检查自定义 Pivot 转换和数据库中的值。 |
| [序列与组合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L590) | 4 | `factory_sequences_continue_across_operations_and_attachments`、双库工厂及 MySQL Seeder 验证常规序列、数量、关系子序列和交叉组合。 同一批双库回归和实际 Seeder 验证序列闭包读取前序状态和类型化父模型、跨操作推进索引，以及调用属性后覆盖。 |
| [工厂发现与绑定](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L713) | 4 | 本批用生成器、嵌套模块以及显式 `--model` 生成和实编译。Rust 模型先绑定到 `FactoryDefinition::Model` 和 `HasFactory`，运行时入口按绑定返回工厂。Laravel 的可变命名空间解析在这里对应编译前路径选择及类型绑定。 |
| [动态方法与多组属性](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L771) | 2 | 名称推断和具名关系已有实际数据库证据。双库回归和 MySQL Artisan 验证将子工厂的 `forEachSequence` 传给 `hasInferred`，两个父模型分别按输入顺序生成三条子记录。 |
| [动态扩展](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L807) | 1 | Rust 用应用扩展 trait 提供可链式调用的方法。本批实际编译并运行三语示例。PHP 宏以运行时字符串注册，Rust 扩展方法由类型和导入确定。 |
| [条件链](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L817) | 1 | `factory_conditionals_select_only_one_branch_and_keep_batch_count` 验证真、假与默认分支及计数。 |
| [软删除状态](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L834) | 3 | `factory_trashed_state_respects_scopes_on_sqlite_and_mysql` 验证状态、已有时间值、过滤范围及模型能力错误。 |
| [模型键与回收边界](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L863) | 7 | `factory_model_keys_and_relation_cache_follow_lifecycle_on_mysql` 覆盖已有模型键。MySQL Seeder 与 `factory_parent_cache_survives_calls_and_respects_connections` 覆盖回收与连接边界。双库新增回归组合了空池和不同类型的多次追加，验证按类型查询、无候选时创建父模型，以及新建子模型时保留池中旧子模型的父键。 |
| [禁止自动创建父模型](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L996) | 5 | `factory_relationship_expansion_defaults_are_snapshotted`、`PolymorphicFactories` 和延迟属性回归覆盖全局默认及按属性或类型选择。 |
| [模型名与工厂解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L1048) | 2 | `named_factory_shortcuts_preserve_configuration_and_model_construction` 及本批显式绑定探针验证返回类型。全局字符串解析改用编译期关联类型。 |
| [四类关系的默认属性及覆盖](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L1067) | 8 | 双库回归验证四类关系的配置默认值与工厂状态。新增 HasOne 和 MorphMany 分别实测默认值独立写入、状态可读取并覆盖默认值、父键及多态标识正确落库。BelongsToMany 使用 `hasAttached` 表达。 |
| [批量插入、零数量、隐藏字段与转换](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentFactoryTest.php#L1123) | 4 | `factory_insert_projects_attributes_before_writing_on_mysql` 和真实 MySQL Seeder 验证无回调写入、空批次、隐藏属性投影和 JSON 转换。 |

## Seeder

| 固定用例起点 | 用例数 | OpenRust 对应及验收边界 |
| --- | ---: | --- |
| [解析子 Seeder 并调用](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php#L31) | 1 | 应用的 `SeederRegistry` 通过签名选择编译注册的 Seeder。双库 Artisan 回归检查命名调用顺序、找不到时退出失败。 |
| [注入容器与命令上下文](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php#L50) | 2 | `SeederContext` 提供同一应用容器和数据库连接，Artisan 负责输出。双库 Artisan、默认 Seeder、事件抑制和连接恢复已有回归。PHP 可调用 setter 更换依赖，Rust 在执行时借用作用域。 |
| [运行入口依赖注入及透传参数](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseSeederTest.php#L64) | 2 | 子 Seeder 通过 `context.application()` 访问容器，通过 `parameters()` 读取调用参数。共享参数、仅一次、静默和失败重试已有双库实测及三语示例。PHP 通过反射按参数签名绑定，Rust 依赖和参数在 `run` 中显式读取。 |
