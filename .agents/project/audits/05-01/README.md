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
