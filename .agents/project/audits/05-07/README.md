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

## 模型及分页资源推断

- 对照固定 Laravel 的显式选择、默认资源关联、集合工厂与空集合退回。Rust 通过 `UseResource` 编译期关联模型和资源，缺少关联会在调用默认入口时由编译器报告。模型实现可声明自定义集合工厂，转换保留源集合和分页器。显式入口可选择另一项资源类型或传入集合工厂，对应 Laravel 的类参数。 [Laravel 模型推断](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/TransformsToResource.php) · [Laravel 集合推断](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/TransformsToResourceCollection.php)
- SQLite 和 MySQL 8.4.13 实际 HTTP 验证默认资源、显式覆盖、普通／简易／游标分页、自定义集合包装及响应头、键保留、空页元数据、空集合回退和显式空集合工厂。10 项资源、模型 HTTP 和分页回归、严格 Clippy、实际 `./artisan inspire`、三个三语示例执行、VitePress 及 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。
- 剩余条件值入口、应用级包装配置及固定文档中 JSON:API 小节的交付边界仍待复核。

## 应用资源包装与条件值

- 对照固定 Laravel 的未包装响应、已存在包装键、强制包装及分页元数据分支。Rust 将默认包装注册为请求所属应用的 `ResourceWrapping`，资源自定义和单次响应优先。关闭默认包装后，分页或附加元数据仍使用 `data`。标准资源默认保留已经包装的结果，强制包装可以增加一层。补齐 `attributes`、`merge` 与 `transform`。条件转换以 JSON 空字符串和空集合为缺失，保留零和布尔值，可用 Rust 的 Option 提供回退。 [Laravel 包装源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/ResourceResponse.php) · [Laravel 条件源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/ConditionallyLoadsAttributes.php)
- 两套应用并发实际 HTTP 验证默认关闭与改名互相隔离、单次覆盖、具名资源、已包装与强制包装、分页和元数据。SQLite／MySQL 8.4.13 复验条件值、隐藏字段及现有资源路径，共 11 项资源、模型 HTTP 与分页回归。严格 Clippy、格式、实际 `./artisan inspire`、三语注册示例执行、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。
- 固定资源章另含 JSON:API Resources。媒体类型、资源类型与标识、稀疏字段、关联包含、链接及分页属于后续验收范围。 [Laravel JSON:API Resources](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md#jsonapi-resources)

## 普通资源的 JSON 响应选项

- 对照固定 Laravel 的 `jsonOptions`、集合继承、`toJson` 和 `toPrettyJson`，复用旧版 `json_encoding.rs` 的四空格缩进及 UTF-16 转义规则。Rust 的 `JsonOptions` 以类型化方法设置可读格式和字符转义，Serde 默认保留 `3.0` 等小数写法。数字和容器形状在 `toArray` 中用 JSON 值类型表达。非空集合使用首个资源的实例选项，空集合和空页使用类型所声明的选项。 [Laravel JsonResource](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/JsonResource.php) · [Laravel 集合选项](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/CollectsResources.php)
- SQLite／MySQL 8.4.13 实际 HTTP 核对单资源、集合、推断集合、分页和空页的原始响应字节，包括小数、缩进、Unicode、斜杠、分页链接和响应钩子。11 项回归、严格 Clippy、格式、实际 `./artisan inspire`、三语示例执行、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式差异只在隔离副本处理。
- 按固定资料继续交付 JSON:API Resources，包括媒体类型、资源标识、稀疏字段、关联包含、分页、链接和元数据。Artisan 的 `make:resource` 目前登记在计划命令中，待接普通和 `--json-api` 生成器。 [Laravel JSON:API 章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md#jsonapi-resources)

## JSON:API 文档、资源标识和稀疏属性

- 对照固定 Laravel 的 `JsonApiResource`、集合和资源标识解析。Rust 的资源实现关联 Eloquent 类型。默认主键为字符串，资源类型来自资源类型名，支持 `toId` 和 `toType` 覆盖。默认属性尊重模型的可见性。支持 `fields[type]`、资源对象链接与元数据、集合与分页文档。JSON:API 的外层固定为 `data`，成功响应使用 `application/vnd.api+json`。旧版仅有 JSON:API MIME 识别，资源复用当前响应、分页和模型序列化。 [固定 Laravel JSON:API](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/JsonApiResource.php) · [JSON:API 规范](https://jsonapi.org/format/1.1/)
- SQLite／MySQL 8.4.13 实际 HTTP 验证默认与自定义标识、稀疏属性、资源链接和元数据、分页、空集合、缺少主键的 500、应用包装隔离及普通资源回归。11 项回归、格式、严格 Clippy、实际 `./artisan inspire`、三语示例执行、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式只在隔离副本处理。
- 后续继续延迟属性、关联标识及 `include`、顶层 JSON:API 信息与 Artisan 生成器。

## JSON:API 延迟属性与复合文档

- 对照固定 Laravel 的延迟属性、关系标识和点分包含。Rust 通过 `lazyAttributes` 登记属性闭包并先筛选字段；`toRelationships` 登记单值及多值关联闭包，只解析请求中的路径。复用当前 HTTP 序列化与 Eloquent 已加载关系。支持 `data: null`、数组标识和顶层 `included`，按类型与标识去重。稀疏字段可以省略关系连接，显式包含的资源仍可输出。 [固定 Laravel 元素解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/Concerns/ResolvesJsonApiElements.php) · [JSON:API 规范](https://jsonapi.org/format/1.1/)
- SQLite／MySQL 8.4.13 实际 HTTP 验证闭包跳过及失败、嵌套 include、空单值、稀疏字段和跨集合去重、主数据重复排除及路径超限 400。11 项 HTTP、分页与模型回归、格式、严格 Clippy、实际 `./artisan inspire`、三语示例执行与 15 项浏览器检查通过。既有 Sanctum 格式只在隔离副本处理。
- 当前应用显式预加载关联。后续接入按请求自动批量补载、应用级 JSON:API 元数据、关系深度配置与 Artisan 生成器。

## JSON:API 关联自动批量加载

- 对照固定 Laravel 在单资源及集合序列化前的 `loadMissing`。Rust 资源用 `includePath` 将请求路径明确映射到 `Model::namedRelations`，并通过 `modelMut` 提供模型。路由在序列化前异步补载，普通集合和三类分页器共享同一批 Eloquent 加载。手动响应使用 `toResponseAsync`。现有关系继续复用，加载失败时保留原模型。 [固定 Laravel 关系解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/Concerns/ResolvesJsonApiElements.php) · [固定 Laravel 集合批量加载](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/AnonymousResourceCollection.php)
- SQLite／MySQL 8.4.13 实际 HTTP 验证无 include、未知路径、点分别名、分页批量补载及已加载关系。MySQL 同连接计数为无 include 1 次 SELECT、未知路径 1 次、嵌套单资源 3 次、预加载同路径 3 次、两条主记录分页 4 次（含计数和主查询）。17 项资源、模型 HTTP、分页与关系回归、格式、严格 Clippy、实际 `./artisan inspire`、三语示例执行和 15 项浏览器检查通过。既有 Sanctum 格式仅在隔离副本处理。
- 后续接入应用级 JSON:API 文档元数据、关系深度和请求控制，再验收 Artisan 生成器。

## JSON:API 应用配置与请求控制

- 对照固定 Laravel 的文档信息、关系深度及请求控制。Rust 在应用容器注册类型化 `JsonApi`，将 `version`、`ext`、`profile` 和 `meta` 放入顶层 `jsonapi`，包括空集合。默认深度五层，超出的路径按固定上游截断，零和一均保留直接关联。格式错误的路径仍返回 400。旧版仅有 MIME 识别，本批复用当前应用容器、资源投影和 Eloquent 已加载关系。[Laravel JSON:API 资源](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/JsonApiResource.php) · [Laravel 请求解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/JsonApi/JsonApiRequest.php)
- Rust 用资源 trait 声明查询策略。关闭策略会按固定文档同时忽略 `fields` 和 `include`。使用已加载关系时，仅选择已声明且已加载的直接关联，稀疏字段仍可单独使用。测试验证关闭查询只执行读取主资源的一次 SELECT。
- 两应用并发真实 HTTP 验证各自的文档信息与截断深度，普通 JSON 不附加 `jsonapi`。SQLite／MySQL 8.4.13 共 11 项资源、HTTP 和分页回归通过。隔离副本格式、严格 Clippy、实际 `./artisan inspire`、三语配置示例执行、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。
- 后续验收普通及 `--json-api` 的 `make:resource`，并完成 05-07 协议对账。

## Artisan 资源生成器

- 对照固定 Laravel 的 `ResourceMakeCommand`、三种模板及生成器测试。已接通 `make:resource`、`-j/--json-api`、`-c/--collection` 和 `-f/--force`。名称后缀 `Collection` 自动选集合模板，集合优先于 JSON:API。重复文件保持原样并报告冲突，退出码为 0。旧版未提供该命令。本批复用现有 Artisan 源码生成、模板发布和原子覆盖机制。 [固定 Laravel 命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ResourceMakeCommand.php) · [固定 Laravel 生成器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Generators/ResourceMakeCommandTest.php)
- Rust 生成到 `app/http/resources`，由应用声明 `mod.rs` 后重新编译。普通和 JSON:API 模板用类型化模型实现资源接口。默认集合是 `ResourceCollection` 的类型别名，继承分页及 JSON:API 集合行为。可在 `stubs` 覆盖三种模板。
- 两项实际 Artisan 生成器测试通过，覆盖帮助、选项优先级、嵌套和应用命名空间、非法路径、模板发布及自定义、重复保护和强制覆盖。隔离副本执行三语文档命令，将生成源码编入实际应用，HTTP 验证普通 JSON、JSON:API 和集合的响应结构、媒体类型以及隐藏字段。SQLite／MySQL 8.4.13 的 11 项资源、分页及 HTTP 回归、严格 Clippy、格式、实际 `./artisan inspire`、VitePress 构建和 15 项浏览器检查通过。既有 Sanctum 格式差异只在隔离副本处理。
- 最终协议复核仍有差异。固定 Laravel 覆盖顶层与嵌套的数组后端资源，Rust 当前的 `JsonApiResource` 强制关联 `Model`。固定 `JsonApiRequest` 的非字符串 fields/include 容错也需要与现有 HTTP 状态逐项对照。05-07 保持在研。 [固定 Laravel JSON:API 集成矩阵](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/Resources/JsonApi/JsonApiResourceTest.php)

## JSON 对象数据源与查询容错

- 对照固定 Laravel 的顶层及嵌套数组后端资源用例。Rust 用 `JsonApiSource` 将数据源投影与响应装配分开。Eloquent 模型继续提供主键、序列化、创建状态和批量加载，PHP 关联数组对应 Rust 的 `serde_json::Map`，默认从字符串或整数 `id` 取标识。已有模型资源和生成模板继续使用相同的接口。本批复用框架响应和加载，旧版没有 JSON:API 资源实现。 [固定 Laravel JSON:API 集成矩阵](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/Resources/JsonApi/JsonApiResourceTest.php)
- 非字符串 `include` 按没有请求关联处理。非字符串 `fields[type]` 保留该类型的字段集记录，但投影为空。未指定该类型字段集时保留全部属性。字符串中存在空路径段仍返回 400。 [固定 Laravel 请求矩阵](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/Resources/JsonApi/JsonApiRequestTest.php)
- SQLite／MySQL 8.4.13 共 11 项回归通过。真实 HTTP 覆盖对象单资源、集合、嵌入 Eloquent 关联、保留字段、无效标识与非字符串查询。MySQL 确认非字符串 include 只执行读取主资源的一次 SELECT。合入并行完成的 05-09 基线后额外复验 3 项授权测试。Rust 格式、全目标严格 Clippy、实际 `./artisan inspire`、三语代码示例执行、VitePress 和 15 项浏览器检查通过。Sanctum 既有格式差异仅在隔离副本处理。
- 固定版还覆盖关系闭包返回资源集合并保留嵌套筛选。OpenRust 现有 `toMany` 的输入为 `Vec<R>`。该入口及可见行为纳入 05-07 最终复核。

## 05-07 整单元验收

- 固定 Laravel 的关系闭包可返回已筛选的资源集合。Rust 增加 `JsonApiRelationships::toCollection`，和向量关系共用标识、稀疏字段和复合文档投影。嵌套 include 只遍历集合选中的项，关联输出是标识数组。SQLite／MySQL 8.4.13 的实际 HTTP 验证公开项及其子项进入 included，私有项及其子项被排除，空集合返回 `data: []`，解析错误在请求该关联时传播，未请求时不执行解析器。 [固定 Laravel 集合关联用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/Resources/JsonApi/JsonApiResourceTest.php)
- 按 05-07 计划复核单资源、集合和键、普通／简易／游标分页、条件属性和关系、空结果与响应钩子，固定 JSON:API 章节的类型与标识、字段、关联、深度、链接、元数据及生成器已有分项验收。本批额外在合入数据库启动调整的 `643f4f03e` 基线上复跑 17 项资源、分页、模型、授权回归和 2 项 Artisan 生成器测试。格式、全目标严格 Clippy、真实 `./artisan inspire`、三语集合示例执行、VitePress 构建和 15 项浏览器检查通过。Sanctum 既有格式差异只在隔离副本处理。 [固定 API Resources 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md)
- Rust 使用 trait、编译期默认资源映射和应用容器，生成后显式声明模块。对象数据源采用 JSON Map，关联自动加载采用已登记路径，错误的点分 include 字符串继续返回 400。05-03 与 05-06 前置已闭合，05-07 记为闭合。

## JSON Resource 与 Responsable 响应生命周期

- 对照固定 Laravel 的 `Router::toResponse`、`JsonResource`、`ResourceCollection` 和分页响应。旧版 `openrust-v0.x` 的 Axum 响应适配已在前序单元复用，旧版未提供可复用的 Laravel 式 JSON Resource 适配。 [Laravel JsonResource](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/JsonResource.php) · [Laravel ResourceCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/ResourceCollection.php) · [Laravel API Resources](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md#concept-overview)
- 直接返回的资源与控制器共用请求感知的响应生命周期。类型化控制器通过 `intoResponsable()` 将资源交给拥有所有权的适配器，避免与通用响应类型的 Rust blanket 实现重叠。适配器保留异步准备、包装、状态、元数据、响应钩子和分页。借用的响应构建器用于已经准备好的 `Sync` 资源，保留单次包装和附加元数据。
- 基于 `1f885ae9b` 的隔离副本，真实 TCP 验证直接路由和控制器异步准备、双应用包装隔离、借用构建器元数据、普通集合和分页查询保留。PHP 8.5 执行固定 Laravel 对照资源投影、元数据、响应头与分页链接。MySQL 8.4.13 复验模型和分页，Docker 卷清单一致。
- 真实 HTTP、固定 Laravel、MySQL 8.4.13、64 项定向回归、Artisan、严格 Clippy、三语站点和 18 项浏览器检查通过。实际应用二进制的健康请求、路径校验和 SIGTERM 退出通过。清理本批独占编译和站点缓存后磁盘可用 98.31 GB（91.56 GiB），提交前周额度剩余 21%，本轮按用户授权继续。验收日志位于 `/private/tmp/openrust-resource-responder.yfdHHQNX`。
