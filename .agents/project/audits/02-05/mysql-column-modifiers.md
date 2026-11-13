# 02-05 MySQL 列修饰符

对照固定 Laravel 的 MySQL Schema Grammar 和 Column Modifiers。复核旧版 `6d1a8192` 的 `database/schema.rs`、`database/schema/grammar.rs` 和 MySQL Schema 测试。旧版没有这四个修饰符，本次沿用当前 Blueprint、标识符引用和目录读取。 [Laravel Column Modifiers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md#column-modifiers) · [Laravel MySqlGrammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)

- `invisible()` 使用原生 `INVISIBLE`。`useCurrentOnUpdate()` 在 timestamp 和 datetime 生成原生 `ON UPDATE CURRENT_TIMESTAMP`，精度与列声明一致。`useCurrent()` 可独立指定插入默认值。
- `first()` 和 `after()` 在添加或修改列时使用原生位置子句。建表按声明顺序放置列，建表时指定位置返回 `InvalidSchema`。其他列类型的自动更新时间返回 `InvalidSchema`。SQLite 上使用这批 MySQL 修饰符返回 `UnsupportedSchema`。
- macOS arm64、Rust 1.94.0 运行 `cargo +1.94.0 test --locked -p openrust-framework --test database_schema -- --test-threads=1`，6 项通过。MySQL 8.4.13 分支强制启用。验证新增及修改列顺序、通配选择省略隐藏列且仍可按名读取、两种日期时间类型的插入默认值和更新行为、非法修饰符在执行 DDL 前被拒绝。SQLite 驱动回归与 Artisan Schema 门面测试通过。临时库和受限用户已删除。
- 同步三语迁移文档。本批覆盖这四个修饰符，跨数据库的完整验收仍按发布计划进行。
