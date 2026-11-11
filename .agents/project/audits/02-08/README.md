# 02-08 通用集合

核对固定 Laravel 13.x 的[创建集合](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/collections.md#creating-collections)、[可用方法](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/collections.md#available-methods)、[Collection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Collection.php) 和[集合回归](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。参考固定 [Goravel 类型化集合](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/support/collect/collection.go)。从旧版 `6d1a8192` 的 `support/collection.rs` 和 `support/finite.rs` 提取规范整数键转换、嵌套非有限浮点检查及单次序列化回归。

- 实现有序的 `Collection<T>` 和 `ArrayKey`，接入 `Enumerable` 共有契约。映射、过滤、投影、分组、排序、截取、合并及聚合遵循本批核对的键保留和重排行为。底层使用锁定依赖中已有的 IndexMap 版本维护顺序与键索引。
- `toJson` 和 Serde 序列化共用实现。按迭代顺序从零连续编号时生成 JSON 数组，稀疏键或字符串键生成对象。直接反序列化可保留输入成员顺序。统一业务响应继续使用此前从旧版提取的 `ApiResponse`，本批验证集合可直接作为响应数据。
- Rust API 使用类型化回调和字段投影。排序按投影类型比较，混合键按整数和字符串的类型顺序比较。平均值和中位数返回 `Option<f64>`，整数求和沿用 Rust 对应类型的求和语义。PHP 的宽松混合类型比较、高阶代理和动态属性路径尚未移植。本批覆盖计划列出的实体集合行为，Laravel 的完整扩展方法表、LazyCollection 和 Eloquent 集合仍有后续范围。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-02-08.hUPiWF` 以前一单元的已审计源码为底。工作区和副本的集合源码、测试及文档逐文件比对，副本的模块出口排除了惰性集合骨架。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告共 363 项通过，含 6 项集合专项。此批沿用 SQLite 回归，未配置真实 MySQL 测试连接。
- 使用 PHP 8.5.12 运行固定 Laravel 集合实现的 14 组对照用例，覆盖过滤、键重排、分组、稳定排序、聚合、截取、分块、合并和并集。对照环境为该版本源码引用的 `SortDirection` 提供了仅用于验证的枚举兼容定义。
- 统一响应测试使用本地 TCP 发送 11 次 HTTP 请求，其中 3 次新增请求验证稀疏键对象、重排数组和非有限值的 500 脱敏响应。`./artisan inspire` 实际退出码为 0。
- 三语示例逐个编译执行。`npm run build`、12 项常规浏览器组合与 6 次集合页面正文和侧栏核对通过。工作区锁定依赖解析和 `git diff --check` 通过。

下一批为 02-09 惰性集合，以本批的键契约和 `Enumerable` 为前置。
