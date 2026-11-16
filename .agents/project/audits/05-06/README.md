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

## 显示分页结果与翻译标签

对照固定 Laravel [Displaying Pagination Results](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md#displaying-pagination-results) 和 [PaginationServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/PaginationServiceProvider.php)。三类分页器提供 `links`、`render` 及带显式视图和数据的 Rust 入口。默认的 Tailwind 与 Simple Tailwind 模板随二进制嵌入，优先解析应用的 `resources/views/vendor/pagination`。Blade 接收结构化分页元数据，复用 JSON 页码窗口。导航标签按当前应用和请求翻译，HTML 经 Blade 转义，JSON 保留翻译原文。Goravel 固定版 ORM 只作为数据分页参考，旧版未找到分页模板或对应翻译入口。

隔离基线 `3eb00e65`。MySQL 8.4.13 与 SQLite 的真实 HTTP 分别验证普通、简易及游标导航、跟随游标链接、单页输出、页码窗口和省略号、带查询参数和片段的链接、并发英文和繁体中文、JSON 标签、模板覆盖及转义。2 项分页、5 项 View、4 项翻译回归、全目标严格 Clippy、`./artisan inspire`、三语示例编译、VitePress 与 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整。VitePress 有既有体积提示。容器已回收并清理隔离 Cargo 产物 2.4 GiB 和任务专用站点产物。提交前周额度剩余 27%，项目卷可用 53 GiB。

后续对照 Laravel 的 Customizing the Pagination View，补齐内置 Bootstrap 视图、应用级默认视图选择与发布。命名路由链接仍按计划在 06-01 补验。

## 自定义分页视图、样式和发布

对照固定 Laravel [Customizing the Pagination View](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md#customizing-the-pagination-view)、[AbstractPaginator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/AbstractPaginator.php#L555) 和[九个内置视图](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/resources/views)。OpenRust 保留应用服务提供者中的命名与配置入口，用应用容器持有默认视图。简易和游标分页共用简易视图设置。内置模板使用 Blade 结构化数据，沿用应用覆盖优先和 HTML 转义，复用发布注册表将同一个嵌入资产表发布到应用路径。固定 Goravel 的 ORM 分页仅作为查询层参考。v0.x 未找到样式选择或分页模板发布。

隔离基线 `427dda3f`。五个同时运行的 SQLite HTTP 应用覆盖 Bootstrap 3／4／5、切回 Tailwind、两个独立自定义默认名，以及显式 Semantic UI 覆盖。简易和游标共享所选模板。MySQL 8.4.13 与 SQLite 重跑真实分页 HTTP。3 项分页、5 项 View、7 项 TestApp 回归通过。实际 `./artisan vendor:publish --tag=laravel-pagination` 发布九个模板，核对与嵌入源逐字节一致，重复发布保留已有编辑，`--force` 恢复源文件。Rust 1.94.0 全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 格式差异在隔离副本处理，VitePress 仍有既有体积提示。

临时 MySQL 已回收，清理独占 Cargo 产物 1.9 GiB 和站点产物。提交前周额度剩余 27%，项目卷可用 53 GiB。继续核对手工创建分页器和实例方法。命名路由链接按计划在 06-01 验证。

## 手工分页与实例方法

对照固定 Laravel [手工创建分页器](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md#manually-creating-a-paginator)、[实例方法](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md#paginator-instance-methods) 与 [CursorPaginator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pagination/CursorPaginator.php)。三个类型都公开手工构造，普通分页接收切片及原总数，简易与游标分页消费预读记录。应用以类型化回调提供游标键，手工和查询分页共用双向边界及页号校验。Rust 的 `through` 移动分页器以允许返回异型记录，同时保留生成链接所需原始键。配置由 `PaginatorOptions` 表达，`getOptions` 借用当前生效的选项。旧版有偏移分页页号和预读策略，未提供对应手工构造。固定 Goravel ORM 作为查询分页参考。

以 `d7a28b7a` 为隔离基线。实际 MySQL 8.4.13 和 SQLite HTTP 回归同时覆盖手工总数、页码范围、简易预读、转换类型、带查询的路径、筛选和片段以及游标正反方向来回。用例验证零页容量、溢出偏移、空值及重复游标键、复合游标编解码。10 项关系与分页回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 与 18 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本调整。测试容器已回收，清理隔离 Cargo 产物 1.8 GiB 和独占站点产物。提交前周额度剩余 27%，项目卷可用 53 GiB。

下一批核对固定分页测试矩阵及 05-02 依赖的完成条件。命名路由链接按计划在 06-01 补验。

## 固定测试矩阵、URL 窗口和 JSON

基线 `444d5299`。固定 Laravel 分页目录的 11 个文件和 51 个用例按[矩阵](test-matrix.md)核对。抽出独立的 `UrlWindow`，Blade 与 JSON 使用同一份分段链接。Rust 通过有序数组表示每段，公开 `linkCollection`、`toArray`、`toJson` 和 `toPrettyJson`，都使用同一 Serde 结构。手工总数分页保留页容量零，使用 1 作末页除数，查询路径继续验证正数。已编码查询的路径由 `setPath` 原样保留。旧版的 `QueryPage/SimplePage` 与固定 Goravel 的 ORM 查询分页没有对应视图窗口服务。

MySQL 8.4.13 与 SQLite 的实际 HTTP 分页、6 项关系回归、6 项分页回归、严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 与 18 项浏览器检查通过。测试涵盖窗口分段、格式化和紧凑 JSON、非法游标及空页。既有 Sanctum 的格式差异只在隔离副本整理。容器已回收，清理隔离 Cargo 产物 1.8 GiB 及独占站点产物。提交前周额度剩余 27%，项目卷可用 52 GiB。

下一批补齐分页器向 Eloquent Collection 转发的 `loadMorph` 和 `loadMorphCount`。Resource 转换在 05-07 验收，命名路由链接在 06-01 验收。

## 分页器多态加载和计数

对照固定 Laravel 的 [PaginatorLoadMorph](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Pagination/PaginatorLoadMorphTest.php)、[CursorPaginatorLoadMorphCount](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Pagination/CursorPaginatorLoadMorphCountTest.php) 及同组四个用例。三种 Eloquent 分页器复用集合的批量加载与计数回填，处理当前页模型后返回同一个具体分页器。Rust 使用类型化目标别名及关系描述符。v0.x 未找到对应转发，固定 Goravel ORM 分页仅作为查询层参考。

以 `1327cae9` 为隔离基线。MySQL 8.4.13 与 SQLite 实际 HTTP 各验证普通、简易和游标分页的混合类型、嵌套集合、计数、空页及链式返回。错误目标类型可返回错误后重试。后续页含非法多态类型时，前两页正常；翻到该页返回 500，删除非法记录后同一游标可重试。6 项分页和 6 项关系回归、严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 排版差异只在隔离副本处理。容器已回收，清理 1.8 GiB 隔离编译产物及独占站点产物。提交前周额度剩余 27%，项目卷可用 52 GiB。

分页矩阵的四项多态转发已验收。账本继续跟踪 05-02 关系与集合依赖整体验收。六项 Resource 转换随 05-07 验收，命名路由链接随 06-01 验收。

## 模型集合工厂与页键

固定 Laravel 的偏移分页保留模型集合键，游标分页按位置重新编号。三种 Eloquent 分页已调用模型集合工厂，普通和简易分页透过 `getByKey` 和 JSON 保留键。多对多工厂在 Pivot 水合后运行。双库 HTTP 和三语验收见 [05-02 集合工厂记录](../05-02/README.md#eloquent-分页的集合工厂和键)。本批验证分页与自定义集合的接线，05-02 的整体验收仍待对账。

## 单条和反向关系分页

以 `e11ba9e2` 为隔离基线。固定 Laravel 的 Relation 将分页转发到关联查询，BelongsTo 由子模型外键约束所有者，MorphTo 先选择具体模型。Rust 为 HasOne、MorphOne、BelongsTo 和 MorphTo 提供普通、简易和游标入口及对应 With 选项。固定关系选择和作用域继续生效。缺失反向引用直接构造携带请求元数据的空页，默认模型仍由加载接口提供。MorphTo 映射为 `MorphTarget` 后保留集合键和游标。v0.x 无相应关系入口，复用当前 Builder 的分页、投影和游标规则。 [固定关系转发](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Relations/Relation.php) · [固定多态目标解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasRelationships.php)

独立 MySQL 8.4.13 实际 HTTP 核对择一结果、软删除、空引用元数据、无效容量、自定义集合键、多态目标和游标往返。分页 HTTP、关系查询计数、关系批量写入及 Query Builder 四项定向回归通过。Rust 1.94.0 全目标严格 Clippy、`./artisan inspire`、三语示例、VitePress 和 18 项浏览器检查通过。既有 Sanctum 测试格式仅在隔离副本调整，临时容器与构建缓存已清除。05-06 继续跟踪模型级默认页容量与 05-02 核心依赖。 [固定用例矩阵](test-matrix.md#关系转发与默认页容量复核)
