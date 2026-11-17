# 04-05 Validator、API 422 与 FormRequest

## 契约与实现

- 对照固定 Laravel 的 Validator、ValidationException、FormRequest 和 ValidationRuleParser，参考固定 Goravel 的工厂与请求上下文传递。复用旧版 `6d1a8192:vendor/openrust/framework/src/validation/{path,runner,tests}.rs` 的路径预算、通配展开、缺失/null/空字符串规则及回归。 [Laravel Validation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md) · [Laravel Validator 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Validation/ValidationValidatorTest.php) · [Goravel Validation](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/validation/validation.go)
- `Validator::make` 支持声明顺序、基础类型和长度规则、`bail`、`sometimes`、`nullable` 及异步自定义规则。应用容器提供工厂和翻译器。验证投影保留嵌套结构，父数组与子规则共用时排除未校验的子字段。JSON 失败由异常 Handler 返回 Laravel 的 `message` 和 `errors`，保留展开后的点路径。
- Rust 通过显式 `validateResolved` 调用保留准备、授权、验证及通过后处理顺序。授权拒绝返回 403。请求继续使用现有输入解析和错误处理。基础规则之外的文件、数据库规则及自动参数注入仍待实现。Web 重定向、旧输入和命名错误包属于 04-06。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 421 项测试通过。
- 真实回环 HTTP 覆盖手动验证、FormRequest 预处理与成功钩子、授权拒绝、嵌套通配 422、null 与缺失、未校验字段排除、中英文并发消息和本地化摘要。异步自定义规则专项覆盖短路、字段标签和自定义消息。
- 三语各两个代码示例编译并分别完成实际 HTTP 200 与 422。 `./artisan inspire` 退出 0。VitePress 构建、12 项通用浏览器组合及 6 项 Validation 正文和导航检查通过。

下一批 04-06 依赖本单元与已验收的 04-04 Session。

## MySQL 数据库验证规则

- 对照固定 Laravel 的 `validateExists`、`validateUnique`、`DatabasePresenceVerifier` 和 `DatabaseRule`。旧版 `v0.x` 没有数据库存在性验证器和对应测试。本批复用当前连接解析器、参数化查询 Builder 与验证规则的路径和错误处理。 [Laravel 数据库规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php) · [Laravel DatabasePresenceVerifier](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/DatabasePresenceVerifier.php)
- 字符串与流式 `exists`、`unique` 接入应用容器的写连接。数组存在性校验按不同值计数，前置规则失败时跳过数据库查询。支持具名连接、默认列、附加查询条件、软删除筛选及排除已有记录。Laravel 动态模型类名对应 Rust 的 `fromModel`，接受已知类型的模型实例。测试覆盖显式记录标识和字段路径排除。
- 使用 `python3 -B tests/mysql_acceptance.py` 和真实 MySQL 8.4.13 验收，HTTP 与路径共 5 项通过。HTTP 夹具把读取端口指向不可用地址，验证查询只走写端。重复输入、数据库重复行、缺失引用、租户过滤、软删除、参数绑定、JSON 422 和执行异常 500 均通过。
- 三语文档示例编译并运行。站点构建、12 项通用浏览器检查及 6 项数据库规则正文／导航检查通过。基线 `409950b79` 和本批均复现 `validation_web.rs:176` 的重定向路径差异，基线及当前均返回 `/`，测试期望 `/edit?step=1`。
- Rust 1.94.0 的改动文件格式、工作区严格 Clippy 和独立 `./artisan inspire` 通过。全仓格式仍只报告基线的 `sanctum_authentication.rs:1412` 差异。已回收临时 MySQL、3.4 GiB 隔离 Cargo 产物和文档缓存，验收日志保留于 `/private/tmp/openrust-04-05-presence.GGTDZGlB`。
- 单元继续跟踪文件规则、FormRequest 自动注入、Laravel 引号 CSV 参数与 PHP 动态类型边界。额外数据库驱动按发布验收验证。核心账本维持 59/66。

## Multipart 文件验证

- 对照固定 Laravel 的 `File`、`validateImage`、`validateMimes`、`validateMimetypes` 和 `validateExtensions`，参考固定 Goravel 的 multipart 输入数据袋。核对旧版 `http/request/{uploaded_file,mime}.rs` 的内容推断和客户端类型分离思路。当前 `UploadedFile` 已持有受请求体限制约束的 `Bytes`，继续复用现有解析器与 `infer`，用 XML 根元素识别显式允许的 SVG。 [固定 Laravel File](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Rules/File.php) · [固定 Goravel 数据袋](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/data_bag.go)
- `Data::fromRequest` 供手动验证与 FormRequest 共用，合并标量和嵌套上传。`ValidatedInput::file` 按验证路径读取已选上传，JSON 投影在文件位置保留 null。字符串和流式规则涵盖文件、图片、内容类型、原始扩展名与大小，区分上游规定的文件大小单位和流式单位后缀。
- MySQL 8.4.13 环境下 `request_input`、`validation_http` 和 `validation_paths` 共 22 项通过。实际 HTTP 包含客户端 MIME 篡改、名称扩展名不匹配、PHP 文件名拒绝、SVG 显式允许、文件大小边界、JSON 仿冒文件、验证后读取和过滤未选上传。
- 额外复验手动 multipart 路径中同一数组成员的文本和文件合并。Rust 1.94.0 的改动文件格式、工作区严格 Clippy、独立 `./artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查均通过。全仓格式仍只报告既有的 `sanctum_authentication.rs:1412` 差异。临时 MySQL、3.4 GiB 隔离 Cargo 产物及文档缓存已回收。验收日志保留于 `/private/tmp/openrust-04-05-files.3FebuwxX`。
- 单元继续跟踪图片尺寸、文件编码和不同平台 MIME 推断差异，以及 FormRequest 自动注入与数据库规则剩余边界。核心账本维持 59/66。

## 图片尺寸与文件编码

- `dimensions`、`Rule::dimensions()` 和 `File::dimensions` 对照固定 Laravel 的宽高约束、宽高比容差与 SVG 例外。框架读取 JPEG、PNG、WebP 等图片头部或 AVIF／HEIC 主图元数据。尺寸取编码记录值，贴合 Laravel 的 `getimagesize`。 [固定 Laravel 尺寸 Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Rules/Dimensions.php) · [固定 Laravel 文件尺寸读取](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/FileHelpers.php)
- `encoding` 和 `File::encoding` 核对上传字节及普通输入中的字符串，支持严格字节序列校验和 UTF-16／UTF-32 的字节序标记。复用锁定版本 `encoding_rs` 和现有图片读取链路，旧版可复用部分为上传字节访问与 MIME 区分。PHP mbstring 的完整编码名称和边界继续跟踪。 [固定 Laravel 编码与尺寸实现](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)
- PHP 8.5.12 执行固定 Laravel 的验证 trait，JPEG、WebP、AVIF 为 100 × 100，HEIC 样本编码尺寸为 112 × 112。UTF-8、ASCII、UTF-16、UTF-32、SJIS 正反例均通过。OpenRust 实际 multipart HTTP 验证同类输入、比例容差、损坏元数据、SVG、跨缓冲区 SJIS 和 422。
- 实际 MySQL 8.4.13 环境下，图片、HTTP 与验证路径专项共 21 项通过。FormRequest 自动注入、数据库规则和图片格式／编码对应的其他边界仍待收尾。核心账本维持 59/66。
- Rust 1.94.0 的改动文件格式、严格工作区 Clippy、独立 `./artisan inspire`、三语示例编译、站点构建及 18 项浏览器检查通过。全仓格式仍报告既有 `sanctum_authentication.rs:1412` 差异。已回收临时 MySQL、3.5 GiB 隔离 Cargo 产物和文档缓存。PHP 对照样本和验收日志保留于 `/private/tmp/openrust-04-05-file-metadata.SVa57vhG`。

## FormRequest 自动注入

- 固定 Laravel 的 FormRequest 解析阶段取得当前请求，在 action 之前执行 `validateResolved`。Rust 用 `ValidatedFormRequest<F>` 声明参数，普通路由、`ControllerDispatcher::withFormRequest` 和 `ResourceController::actionWithFormRequest` 共用既有生命周期。控制器可继续读取原始请求和路由参数。旧版未找到可复用的 FormRequest 自动注入。 [固定 Laravel FormRequestServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Providers/FormRequestServiceProvider.php) · [固定 Laravel 验证生命周期](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/ValidatesWhenResolvedTrait.php)
- 每次解析构造独立表单实例。默认使用 `Default`，可用当前应用容器中的瞬态绑定提供依赖。实际双应用与并发 HTTP 验证默认和异步容器构造、预处理、授权、规则、通过钩子、403／422 阻止 action、原始路由参数及资源路由。三语示例各自实测 HTTP 200 与 422。
- 基于集成提交 `1d3565b91`，真实 MySQL 8.4.13 环境的请求及验证回归 24 项通过，Web 显式重定向与命名错误包定向用例通过。严格 Clippy、独立 Artisan、变更文件格式、三语站点和 18 项浏览器检查通过。全仓格式仍有基线 `sanctum_authentication.rs:1412` 差异。已回收临时 MySQL、3.5 GiB 隔离 Cargo 产物和文档缓存。验收日志保留于 `/private/tmp/openrust-04-05-form-request.9VShGSKk`。
- 单元还需核对数据库规则的字符串参数、类型边界及文件格式和编码覆盖。核心账本维持 59/66。
