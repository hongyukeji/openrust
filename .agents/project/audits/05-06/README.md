# 05-06 数据库分页

- 对照固定 Laravel 的 Query Builder、Eloquent、游标方向、链接窗口和 JSON 字段。参考固定 Goravel 的 ORM 分页。复用 `v0.x` Query Builder 的请求页码解析、分组计数和简易分页多读一行策略。 [Laravel 分页](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md) · [Laravel BuildsQueries](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Concerns/BuildsQueries.php) · [Laravel UrlWindow](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/UrlWindow.php) · [Goravel ORM](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/database/orm)
- 普通分页支持分组、去重和联合查询计数。简易分页多读一行。游标分页按多列键执行正向、反向比较，保留调用方原条件分组和查询别名。Eloquent 沿用作用域、属性转换和关系加载，只装配实际返回的模型。分页链接从应用隔离的请求上下文取得路径和查询参数。
- Rust 用 `PageOptions`、`CursorOptions` 表示可选命名参数，分页器按 Serde 序列化，链接采用请求相对路径。游标支持已选出的非空标量列，分组、联合和原始排序使用偏移分页。链接窗口输出 JSON，命名路由链接按计划在 06-01 补验。Blade 分页视图和本地化导航标签仍未接入。
- 以 `f3f6bd8a` 为隔离基线。Rust 1.94.0 格式、453 项锁定离线工作区测试、全目标严格 Clippy、`./artisan inspire` 通过。实际 SQLite 和 MySQL HTTP 验证自定义页名、并行请求、JSON 链接、筛选参数、空页、游标往返和非法输入。MySQL 在同一事务连接测得普通、简易、游标分别执行 2、1、1 次 SELECT。
- 三语文档同一示例分别在 SQLite 和 MySQL 经 HTTP 执行。VitePress 构建和 12 项通用浏览器组合通过，分页正文和链接在根路径及子路径共 6 项通过。

## 05-02 依赖复核：直接一对多关系分页

固定 Laravel [Relation](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) 将普通一对多关系的分页调用转发到受约束的 Eloquent Builder，多态一对多附加类型条件。Rust 显式借用父模型，由 HasMany 和 MorphMany 提供三类分页及其选项入口，继续使用模型查询的作用域和水合。父键为空时使用空匹配。父键和多态类型保留在基础谓词内，调用者追加的 OR 条件仍由现有模型查询分组。旧版关系没有分页入口，沿用当前已从 v0.x 核对的查询分页策略。固定 [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go) 作为查询层分页参考。

以 `56f3d215` 为隔离基线。MySQL 8.4.13 与 SQLite 的真实 HTTP 请求验证 HasMany 的总数、定制页名、筛选参数、普通回翻和父键筛选，MorphMany 的类型隔离与游标来回翻页。数据库回归另验证简易分页有下一页，以及无键父模型不会读到外键为 NULL 的行。关系套件 6 项、分页套件 2 项通过。Rust 1.94.0 格式、工作区全目标严格 Clippy 和 `./artisan inspire` 通过。既有 Sanctum 格式差异仅在隔离副本调整。三语示例独立编译，VitePress 和根路径／子路径 18 项浏览器检查通过。

临时数据库和容器已回收。清理隔离 Cargo 产物 1.8 GiB 及独占站点产物。提交前周额度剩余 28%，项目卷可用 53 GiB。下一批核对穿透关系和多对多 Pagination，其中多对多要保留当前页的 Pivot 数据。Blade 链接和本地化导航标签继续在 05-06 跟踪。

## 穿透关系分页与中间模型筛选

固定 Laravel [HasOneOrManyThrough](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/HasOneOrManyThrough.php#L572) 在加入穿透关系选列后调用 Eloquent 分页。Rust 为 HasManyThrough 和 HasOneThrough 提供同样三类分页，显式接收父模型。复用既有穿透联接、软删除范围、多态条件和单选关系选择。默认选列限定为关联表，显式选列可避免同名字段冲突。单父模型的分页不需要向模型公开用于批量匹配的内部键。旧版关系未提供对应入口，沿用已核对的 v0.x 底层查询分页。固定 [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go) 继续作为查询分页参考。

隔离基线 `b85a5b51`。MySQL 8.4.13 与 SQLite 的真实 HTTP 验证穿透分页总数、选列、母键隔离、筛选参数与上一页链接。双库直接验证游标跨页、无键父模型返回空页、`withTrashedParents()` 重新纳入软删除中间模型，以及 `latestOfMany` 分页只返回所选模型。6 项关系和 2 项分页回归通过。Rust 1.94.0 格式、全工作区全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 构建与 18 项浏览器检查通过。既有 Sanctum 格式只在隔离副本调整。

临时容器已回收，清理隔离 Cargo 产物 1.8 GiB 与独占站点产物。提交前周额度剩余 28%，项目卷可用 53 GiB。下一批补多对多分页的 Pivot 水合，Blade 分页链接和多语言导航仍待接入。

## 多对多分页与 Pivot

固定 [Laravel BelongsToMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/BelongsToMany.php) 调用 Eloquent 分页后为当前页模型装配 Pivot。Rust 的 BelongsToMany 与 MorphToMany 提供三种分页及其选项入口，显式借用父模型。复用现有多态限制、Pivot 谓词、自定义模型、访问器及逆向快照。为游标选出的排序字段通过 SQL 别名保留给分页器，Pivot 仍按实际选列装配。旧版关系没有分页入口，沿用本单元已核对的 v0.x 查询分页。固定 [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/database/orm/orm.go) 用作查询层分页参考。

以 `6fe44db2` 为隔离基线。在真实 MySQL 8.4.13 和 SQLite 上分别经 HTTP 验证父键和多态类型隔离、普通分页总数和回翻、自定义页名和筛选参数、游标来回翻页、自定义 Pivot 主键和访问器，以及内部投影从模型属性及其原始快照移除。双库直接验证简易分页的下一页和无键父模型空页。6 项关系、2 项分页回归、Rust 1.94.0 格式、工作区全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整。VitePress 构建有既有的 bundle 体积提示。

临时 MySQL 容器已回收，清理本批独占 Cargo 产物 1.8 GiB 和独占文档构建产物及依赖。提交前周额度剩余 28%，项目卷可用 53 GiB。接下来核对 Blade 分页链接和本地化导航标签。
