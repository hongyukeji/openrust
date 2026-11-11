# 05-06 数据库分页

- 对照固定 Laravel 的 Query Builder、Eloquent、游标方向、链接窗口和 JSON 字段。参考固定 Goravel 的 ORM 分页。复用 `v0.x` Query Builder 的请求页码解析、分组计数和简易分页多读一行策略。 [Laravel 分页](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md) · [Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php) · [Laravel UrlWindow](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/UrlWindow.php) · [Goravel ORM](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/database/orm)
- 普通分页支持分组、去重和联合查询计数。简易分页多读一行。游标分页按多列键执行正向、反向比较，保留调用方原条件分组和查询别名。Eloquent 沿用作用域、属性转换和关系加载，只装配实际返回的模型。分页链接从应用隔离的请求上下文取得路径和查询参数。
- Rust 用 `PageOptions`、`CursorOptions` 表示可选命名参数，分页器按 Serde 序列化，链接采用请求相对路径。游标支持已选出的非空标量列，分组、联合和原始排序使用偏移分页。链接窗口输出 JSON，命名路由链接按计划在 06-01 补验。Blade 分页视图和本地化导航标签仍未接入。
- 以 `f3f6bd8a` 为隔离基线。Rust 1.94.0 格式、453 项锁定离线工作区测试、全目标严格 Clippy、`./artisan inspire` 通过。实际 SQLite 和 MySQL HTTP 验证自定义页名、并行请求、JSON 链接、筛选参数、空页、游标往返和非法输入。MySQL 在同一事务连接测得普通、简易、游标分别执行 2、1、1 次 SELECT。
- 三语文档同一示例分别在 SQLite 和 MySQL 经 HTTP 执行。VitePress 构建和 12 项通用浏览器组合通过，分页正文和链接在根路径及子路径共 6 项通过。
