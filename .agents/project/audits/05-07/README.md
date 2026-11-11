# 05-07 API Resources

- 对照固定 Laravel 的资源概览、集合、分页和条件属性，核对 `ResourceResponse`、`PaginatedResourceResponse` 和条件关系处理。参考固定 Goravel 的 HTTP JSON 响应契约。旧版 `v0.x` 的 `ApiResponse` 与 `PageMeta` 已由现有业务响应提供。本单元复用 Eloquent 序列化、聚合别名和分页器元数据。 [Laravel API Resources](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md) · [Laravel ResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/ResourceResponse.php) · [Laravel PaginatedResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/PaginatedResourceResponse.php) · [Goravel HTTP](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go)
- `JsonResource` 按当前请求转换，路由可直接返回资源，支持默认和显式包装、附加元数据、递归合并以及响应钩子。 `ModelResource` 保留隐藏字段与新建模型 201。三种分页集合沿用原分页链接和元数据，支持保留查询及分页信息钩子。条件字段由 Option 区分缺失和值为 null，已加载关系使用显式类型。
- Rust 的包装选择作用于资源或单次响应，集合输入保留向量顺序。PHP 静态全局包装、自动资源类推断、关联键保留以及基于中间表名的条件帮助方法仍未移植。
- 以 `0e1e89b8` 为隔离基线。Rust 1.94.0 格式、455 项锁定离线测试、全目标严格 Clippy、`./artisan inspire` 通过。实际 SQLite 与 MySQL HTTP 验证单资源、向量集合、三种分页、空集合和空页、包装规则、附加元数据合并、并发条件字段、已加载关系与计数、404、400 和新建 201。
- 三语示例从正文抽取后在 SQLite 与 MySQL 上通过实际 HTTP。VitePress 构建、12 项通用组合及资源正文和导航在根路径与子路径共 6 项通过。
