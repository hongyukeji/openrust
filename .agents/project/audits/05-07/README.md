# 05-07 API Resources

- 对照固定 Laravel 的资源概览、集合、分页和条件属性，核对 `ResourceResponse`、`PaginatedResourceResponse` 和条件关系处理。参考固定 Goravel 的 HTTP JSON 响应契约。旧版 `v0.x` 的 `ApiResponse` 与 `PageMeta` 已由现有业务响应提供。本单元复用 Eloquent 序列化、聚合别名和分页器元数据。 [Laravel API Resources](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md) · [Laravel ResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/ResourceResponse.php) · [Laravel PaginatedResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/PaginatedResourceResponse.php) · [Goravel HTTP](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go)
- `JsonResource` 按当前请求转换，路由可直接返回资源，支持默认和显式包装、附加元数据、递归合并以及响应钩子。 `ModelResource` 保留隐藏字段与新建模型 201。三种分页集合沿用原分页链接和元数据，支持保留查询及分页信息钩子。条件字段由 Option 区分缺失和值为 null，已加载关系使用显式类型。
- Rust 的包装选择作用于资源或单次响应，集合输入保留向量顺序。PHP 静态全局包装、自动资源类推断、关联键保留以及基于中间表名的条件帮助方法仍未移植。
- 以 `0e1e89b8` 为隔离基线。Rust 1.94.0 格式、455 项锁定离线测试、全目标严格 Clippy、`./artisan inspire` 通过。实际 SQLite 与 MySQL HTTP 验证单资源、向量集合、三种分页、空集合和空页、包装规则、附加元数据合并、并发条件字段、已加载关系与计数、404、400 和新建 201。
- 三语示例从正文抽取后在 SQLite 与 MySQL 上通过实际 HTTP。VitePress 构建、12 项通用组合及资源正文和导航在根路径与子路径共 6 项通过。

## 条件字段和中间表

- 对照固定 Laravel 的可选追加字段、合并和 Pivot 资源用例。Rust 在资源中显式区分尚未加载和已加载为空的单值关系，按已追加属性及显式 null 执行投影。关联对象的条件合并保留此前写入的键。已水合的普通及自定义 Pivot 都可按实际表和访问名检测，自定义 Pivot 另有 Rust 类型匹配入口。检查仅访问已有关系结果。 [Laravel 条件接口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/ConditionallyLoadsAttributes.php) · [Laravel HTTP 用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/ResourceTest.php)
- SQLite／MySQL 8.4.13 实际 HTTP 验证追加属性、合并键冲突、缺失字段、已加载空关系和两类 Pivot。错误表、访问名和类型均跳过相应回调。10 项资源、Eloquent 关系及 JSON 数据库回归、严格 Clippy、实际 `./artisan inspire`、三语样例编译、VitePress 及 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。
- 剩余范围包括 Resource 集合键保留、模型及分页集合的资源推断，以及其他条件值便捷入口。

## 集合键保留

- 对照固定 Laravel 的 `preserveKeys`、数字键重排、混合键及游标索引测试。Rust 资源集合可以从 Support、Eloquent 和关联集合接收有序键，转换元素时保留这些键。默认重排全部为数字的键，出现命名键时原样输出。`JsonResource::preservesKeys` 声明资源类型的策略，`preserveKeys(bool)` 可在单次响应覆盖。 [Laravel 集合键文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md#preserving-collection-keys) · [Laravel 资源测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/ResourceTest.php)
- SQLite 和 MySQL 8.4.13 实际 HTTP 验证稀疏及数字字符串键、混合键和顺序、类型策略及单次覆盖、关联集合、空集合、普通／简易分页后续链接和游标双向翻页。游标沿用固定上游的连续索引。8 项资源及分页回归、严格 Clippy、格式、实际 `./artisan inspire`、三语示例执行、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。
- 后续复核模型、集合及分页器的默认资源声明与推断，以及剩余条件值入口。
