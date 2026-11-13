# 05-01 Eloquent 模型读写和作用域

## 契约与实现

- 对照固定 Laravel 的 Model、Builder、SoftDeletes、ModelMakeCommand 及软删除集成测试，参考固定 Goravel 的 ORM Provider 和模型装配。复用旧版 `6d1a8192` 的模型显式元数据、默认全保护、fillable 优先、原始属性快照和原主键持久化规则。SQL 通过现有 Query Builder 执行。 [Laravel Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 软删除测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentSoftDeletesIntegrationTest.php) · [Goravel ORM Model](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/model.go)
- 接通模型生成、静态表名与连接、默认和自定义主键、批量赋值、原始属性与脏字段、UTC 时间戳、按原键更新、硬删除与软删除、恢复、受影响行数。已读取的模型保留来源连接。全局约束使用独立的 AND 分组，避免外层 OR 绕过约束。构造器的 `forceDelete` 遵循 Laravel 直接在基础查询执行删除的行为。
- Rust 用 `ModelMetadata` 和 `ModelState` 显式表示模型，用 `Scope<M>` 实现局部与全局约束，用异步 Result 表示读写失败。生成器使用 `app/models`，Rust 模块声明由应用添加。此批生成器覆盖模型文件和 `--force`，工厂、迁移等组合生成选项仍待对应功能接入。赋值按静态列规则过滤，未查询动态 schema。关系、丰富集合、属性转换及序列化由后续单元完成，生命周期监听器和自动唯一 ID 尚未接入。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 434 项测试。使用专用 schema 和 `OPENRUST_REQUIRE_ELOQUENT_MYSQL=1` 运行真实 MySQL 8.4.13，追加字符串主键用例后复跑双库专项与严格 Clippy，均通过。
- SQLite 和 MySQL 验证模型映射、自动与字符串主键、guarded 和 fillable、变更主键时使用原键、失败后修正重试、作用域及 OR 分组、软删除与恢复、构造器受影响行数、实例硬删除。
- 实际 `./artisan make:model Flight` 及嵌套模型生成、重复拒绝、穿越拒绝和 `--force` 覆盖通过，生成的模型以 Rust 1.94.0 编译通过。`./artisan inspire` 正常退出。
- 三语模型声明与作用域示例均编译，经真实 HTTP 在 SQLite 和 MySQL 各执行一次，六次返回 204，并断言应用作用域内的静态查询。VitePress 构建、12 项通用浏览器组合及 6 项 Eloquent 正文和导航检查通过。

下一批 05-02 Eloquent 关系、加载与集合，以本单元为前置。

## 独立补交复验

以本地 `24bb4bbd` 为基线，拆出关系、转换和序列化差异。Rust 1.94.0 格式、433 项工作区测试和全目标严格 Clippy 通过。模型读写与作用域在 SQLite 和真实 MySQL 复验通过。Artisan 实际验证 `inspire`、模型生成、同名拒绝及 `--force` 覆盖。

## 模型生命周期派发

- 对照固定 Laravel 的保存、删除和恢复顺序及模型测试，参考 Goravel 的显式类型模型。沿用 `v0.x` 的预事件取消和 Tokio 调用链静默机制。本版使用应用容器内的类型化注册表，在实际操作的连接上借用可变模型。监听器执行前释放注册表锁。查询构造器批量写入保持 Laravel 的实例事件边界。 [Laravel 模型事件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#events) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php)
- SQLite 与独立 MySQL 8.4.13 验证写前修改、取消、无变更保存、查询加载、软删除、恢复、强制删除、静默写入、应用隔离、SQL 失败及写后监听器错误。写后错误时保留已执行的持久化状态，避免把重试当作首次插入。Rust 的静默范围是当前异步调用链。
- 生命周期派发已接入。映射到普通事件 Dispatcher、复制及其 `replicating` 事件和自动唯一 ID 仍待跟踪。
- Rust 1.94.0 格式、615 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语监听器示例分别在 SQLite 执行，VitePress 构建及 18 项浏览器检查通过。实际二进制运行 `make:model LifecycleArticle` 与 `inspire` 成功。

## 显式事务中的模型写入

- 对照固定 Laravel 的模型事务保存及事务隔离测试，参考 Goravel 向回调提供事务句柄的方式。复用 `v0.x` 由事务查询执行同一保存流程的做法。Rust 的五个 `*InTransaction` 写入方法借用活动事务，已绑定连接的模型校验查询来源。调用方控制提交和回滚，模型实例仍反映本次写入尝试。 [Laravel Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 事务测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/DatabaseTransactionsTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go)
- SQLite 与独立 MySQL 8.4.13 实测事务内自动键、更新、软删除、恢复、保存点回滚后保留记录、写前取消、同事务监听器审计回滚、不同应用连接拒绝和关闭事务拒绝。生命周期监听器可接收当前事务。已有 Observer 回调继续使用连接；事务后 Observer 在 06-02 接入。
- Rust 1.94.0 格式、616 项锁定离线工作区测试及全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语示例分别在 SQLite 编译运行，VitePress 构建、12 项通用和 6 项模型正文浏览器检查通过。实际二进制的 `artisan inspire` 正常退出。

## 模型事件映射

- 对照固定 Laravel 的 `HasEvents::fireModelEvent`，先经所属应用派发类型事件，再按响应决定取消、完成或回退到模型监听器。复用 `v0.x` 的派发优先级、事务传播与空响应回归，沿用固定 Goravel 参考的编译期模型装配。Rust 用 `Model::dispatchesEvents` 返回拥有数据的类型事件，现有模型监听器和 Observer 继续借用可变模型。写前布尔 `false` 取消写入，常用零值和空字符串回退。任意 Rust 对象是有效响应，应用可显式返回 `None` 表示无响应。 [Laravel HasEvents](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasEvents.php) · [Laravel 模型事件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#events) · [Goravel ORM Model](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/model.go)
- SQLite 和独立 MySQL 8.4.13 实际模型写入验证写前取消、响应优先级、回退、异常传播、静默、两应用隔离、事务提交及回滚。提交后监听器收到派发时的自有载荷，并能解析所属应用的连接。
- 三语文档示例分别编译运行，验证载荷和原模型后续修改互相独立。实际 Artisan 的 `event:list --json` 和 `make:model MappedFlight` 通过。复制和自动唯一 ID 继续跟踪。
- Rust 1.94.0 格式、618 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语站点构建及 18 项浏览器检查通过。

## 模型复制

- 对照固定 Laravel 的 `replicate`、`replicateQuietly` 及生成列排除回归。Rust 使用异步接口调用现有模型事件。副本排除主键、配置的时间戳、内部中间表键和调用者指定列，保留已加载关系。副本沿用源连接，这是为了让显式连接绑定的 Rust 模型在原库保存。固定 Goravel 作为模型装配参考，`v0.x` 未找到独立复制入口。自动唯一 ID 和生成列刷新有各自的前置实现，复制的相应自动排除随这些能力接入。 [Laravel 模型复制](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#replicating-models) · [Laravel Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentIntegrationTest.php)
- SQLite 和独立 MySQL 8.4.13 验证当前原始属性、字段排除、原始快照重置、关系保留及独立移除、事件修改副本、静默复制、插入新主键和原记录保持。三语示例编译运行，实际 Artisan 生成 `ReplicableFlight` 通过。
- Rust 1.94.0 格式、618 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。三语站点构建和 18 项浏览器检查通过。

## 自动唯一 ID

- 对照固定 Laravel `HasUniqueIds`、`HasUuids`、`HasUlids` 和 `HasUniqueStringIds`。Rust 用静态 `ModelMetadata::uniqueId` 和 `uniqueIds` 声明生成策略与列，复用 `v0.x` 的 UUIDv7、空值填充及属性写入契约。默认作用于主键，包含主键时按非自增字符串处理。保留 `newUniqueId` 和 `isValidUniqueId` 的类型化覆盖入口。Goravel 模型装配作为编译期参考。 [Laravel UUID 与 ULID 主键](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#uuid-and-ulid-keys) · [Laravel HasUniqueStringIds](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasUniqueStringIds.php)
- SQLite 与独立 MySQL 8.4.13 实际插入验证 UUIDv7、小写 ULID、空值填充、保留显式标识符、复制排除并重新生成。真实 TCP HTTP 验证 UUID 主键及 ULID 指定列绑定成功，格式错误即使存在对应数据库行也返回 404。三语示例编译运行。数据库生成列刷新仍待收尾。
- Rust 1.94.0 格式、618 项锁定离线工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。实际 Artisan 生成 `Ticket` 成功。三语站点构建及 18 项浏览器检查通过。

## 写入后刷新属性

- 对照固定 Laravel 的 `refreshSavedAttributes` 和生成列复制回归。Rust 通过 `ModelMetadata::refreshes` 声明回读列，沿用 `v0.x` 依主键及当前事务读取的思路。回读走写连接且不应用查询作用域，在 `Created`、`Updated` 和 `Saved` 之前合并属性。更新主键后用新键回读已写入行。复制排除这些列。回读失败向调用方返回错误，模型保留已写入状态。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#refreshing-attributes-after-writes) · [Laravel Model](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentIntegrationTest.php)
- SQLite 与独立 MySQL 8.4.13 实测生成列插入、显式事务更新、变更主键、写后事件、原始值与变更快照、复制重新生成、回读缺行错误。三语示例编译运行。
- 以 `7511e70d` 干净源码加本批差异验收。Rust 1.94.0 格式、618 项工作区测试、全目标严格 Clippy 通过，8 项进程夹具标记忽略。实际 Artisan 生成 `GeneratedArticle` 通过。隔离三语构建及 18 项浏览器检查通过。05-01 核心闭合。

## 单模型刷新与关系重载

按固定 Laravel 的 Refreshing Models 实现 `fresh()`、`freshWithPaths()` 和 `refresh()`。从模型的写连接按原身份无作用域回读，复用现行水合与 Retrieved 事件。普通模型使用原主键，Pivot 复用持久化的复合键和多态身份约束。原位刷新保留实例配置及 Pivot 投影，通过已登记的名称重载直接关系，成功后同步原始属性。读取失败保留旧状态，关系失败保留新属性及原始快照。参考 Goravel 的连接级 ORM，复用旧版 `fresh_with`、`refresh` 的职责划分和失败顺序。 [Laravel 模型源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentIntegrationTest.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md#refreshing-models) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)

SQLite 和独立 MySQL 8.4.13 验证原主键、全局作用域、软删除、新模型、数据库记录已删除、命名路径、缓存关系替换与复合键 Pivot 回读。锁定回读 `refreshForUpdate` 仍需接入当前事务 API 并验证锁的生命周期。

工作区 620 项通过、8 项夹具忽略。最终差异通过双库模型与关系专项，额外验证未登记关系错误时保留新属性与旧快照。Rust 1.94 格式、全目标严格 Clippy、实际 `artisan inspire`、三语示例编译、站点构建和 18 项浏览器检查通过。
