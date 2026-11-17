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

## 数据库规则参数与标量边界

- `exists` 和 `unique` 字符串参数按固定 Laravel 的 CSV 规则保留引号内的逗号、双引号与转义。字符串规则排除 ID 沿用整数正规化与反斜杠处理。文本主键 `001` 保留前导零，`+1` 正规化为 `1`。规则解析与查询继续使用现有 SQL 参数绑定。旧版未找到可复用的数据库规则字符串解析。 [固定 Laravel 参数解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/ValidationRuleParser.php) · [固定 Laravel 数据库规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)
- 数组 `exists` 按 PHP 默认字符串精度 14 计算不同标量数，查询使用实际绑定值。嵌套数组和对象作为查询值返回验证错误。动态排除使用确切字段路径，缺失、null 或复合值不会排除记录。
- PHP 8.5.12 直接执行固定 Laravel 参数解析和验证 trait，核对引号、文本和数值 ID、动态排除、科学计数与浮点去重。真实 MySQL 8.4.13 的 HTTP 和验证路径共 8 项通过。三语数据库示例在真实 HTTP 上验证命中与拒绝路径。
- 固定 Goravel 的存在性与唯一性规则也通过数据库 Builder 查询，字符串参数契约按固定 Laravel 保留。Rust 1.94.0 变更文件格式、严格工作区 Clippy、独立 Artisan、三语站点及 18 项浏览器检查通过。全仓格式仍有既有 `sanctum_authentication.rs:1412` 差异。已回收临时 MySQL、3.1 GiB 隔离 Cargo 产物和文档缓存。验收日志保留于 `/private/tmp/openrust-04-05-db-boundaries.WLNsceH7`。 [固定 Goravel 数据库规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go#L1753)
- 04-05 仍待复核文件格式和编码覆盖，额外数据库驱动归入发布验收。核心账本维持 59/66。

## 上传内容类型与常用编码对照

- PHP 8.5.12 的 Fileinfo 样本与固定 Laravel `validateEncoding` 对照覆盖 JSON 对象及数组、JSON 标量、CSV、带 BOM 的 UTF-16、空文件和 PDF，以及 SJIS、CP932、EUC-CN、UCS-2 和 UCS-4 正反例。旧版复用内容推断与文件名分离的契约，当前基于同一 `UploadedFile` 的受限字节和共享缓存，在多条验证规则及存储命名中复用结果。 [固定 Laravel 文件规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)
- JSON 容器使用语法检测，UTF-16 文本识别字节序标记，空文件匹配 `application/x-empty`。扩展名规则继续读取客户端文件名。编码规则按受支持的 Laravel 名称解析，SJIS 与 CP932 使用各自的字节范围，EUC-CN 保留双字节范围，UCS 固定宽度校验与 UTF Unicode 标量校验各自独立。固定 Goravel 的 MIME 与编码规则用作实现参考。 [固定 Goravel 验证规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go)
- 实际 MySQL 8.4.13 环境下，文件系统、上传与验证路径回归共 34 项通过。三语示例在实际 multipart HTTP 上通过 200 和 422。传输编码、部分旧式编码表及未纳入 MIME 样本的格式仍需核定兼容边界。核心账本维持 59/66。
- Rust 1.94.0 改动文件格式、工作区严格 Clippy、独立 Artisan、三语站点和 18 项浏览器检查通过。集成独立格式提交 `45a982138` 后复查全仓格式通过。临时 MySQL、3.5 GiB 隔离 Cargo 产物和文档缓存已回收，PHP 对照及验收日志保留于 `/private/tmp/openrust-04-05-file-coverage.fTlKoYPe`。

## 传输编码与规范 Base64

- 固定 Laravel `validateEncoding` 使用 PHP mbstring 的传输编码检查，`validateBase64` 使用严格解码及规范字符串比较。PHP 8.5.12 容器内的 mbstring 源码显示，旧式 BASE64 忽略 `=`、空格、制表符及 CR/LF。Quoted-Printable、HTML-ENTITIES 和 UUENCODE 的解码分支接受任意字节。固定 Goravel 的编码规则和旧版传输格式 MIME 表均已核对，复用当前 `base64` 依赖的严格流式解码，避免为了验证保留整份解码后的数据。 [固定 Laravel 验证规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php) · [Laravel Encoding](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding) · [固定 Goravel 编码规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go#L1712)
- PHP 直接执行固定 Laravel 验证 trait，校验 BASE64 的 256 种字节、另外三种传输编码的 9,000 个确定性样本，以及严格 Base64 的正反例。真实 MySQL 8.4.13 环境的 HTTP 和验证路径共 10 项通过。新增 HTTP 覆盖宽松与规范规则的不同结果、非字符串、长输入、错误字符和 multipart 字节。三语示例分别实测 HTTP 200 和两条 422 路径。
- Rust 1.94.0 全仓格式、工作区严格 Clippy、独立 Artisan、三语站点及 18 项浏览器检查通过。已回收临时 MySQL、3.1 GiB 隔离 Cargo 产物及文档缓存。PHP 对照和验收日志保留于 `/private/tmp/openrust-04-05-transfer.9mTJpuvo`。
- UTF-7／UTF7-IMAP 等状态型编码、部分旧式字表、其他文件格式和交付单元完整复验仍待核定。核心账本维持 59/66。

## UTF-7 与 UTF7-IMAP

- 字符串与流式 `encoding` 支持两种 UTF-7。验证器逐段读取移位字节，校验变体各自的字符表、UTF-16 代理对、尾部位及结束符，使用固定大小的位缓冲。对照固定 Laravel 的编码入口和 PHP 8.5.12 容器内的 mbstring 源码，保留标准 UTF-7 和 IMAP 变体各自的终止行为。固定 Goravel 的编码入口已核对，旧版没有找到可复用的 UTF-7 实现。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding) · [固定 Goravel 编码规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go#L1712)
- 对照固定 Laravel／PHP 的 136,114 个向量通过，覆盖两种编码各自的全部 65,536 个单独 UTF-16 码元、直接字节、移位结束字节、随机码元组合和损坏尾位。真实 MySQL 8.4.13 环境下，HTTP 与验证路径共 11 项通过。实际上传覆盖两个字符表、代理对和 422。三语示例在真实 HTTP 上返回 200 与 422。
- Rust 1.94.0 全仓格式、工作区严格 Clippy、独立 Artisan、三语站点及 18 项浏览器检查通过。临时 MySQL、3.3 GiB 隔离 Cargo 产物和文档缓存已回收。对照样本和验收日志保留于 `/private/tmp/openrust-04-05-utf7.TSfFRsQR`。
- 下一批处理 TIFF 和 ICO 图片尺寸。相同的真实 100 × 100 TIFF、ICO 样本在 PHP `getimagesize` 可读，OpenRust `dimensions` 实测返回 HTTP 422。其他旧式字表和文件格式仍待按整单元验收核定。核心账本维持 59/66。

## TIFF 与 ICO 图片尺寸

- 固定 Laravel 的 `validateDimensions` 读取 `FileHelpers::dimensions()` 所用的 PHP `getimagesize`。本批对照 PHP 8.5.12 的 TIFF／ICO 目录逻辑。TIFF 校验首个图像目录的范围，处理大小端、内联尺寸和 EXIF 尺寸覆盖。ICO 取位深最高的完整条目，并在位深相同时取最后一项。零宽高编码和只有合法条目计数的 ICO 头沿用 PHP 的 256 像素回退。Rust 使用受边界检查的切片读取，无须解码像素或增加图像处理依赖。固定 Goravel 的 `ruleDimensions` 和旧版 `v0.x` 已核对，旧版未找到对应目录解析可复用。 [Laravel FileHelpers](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/FileHelpers.php#L62) · [Laravel Dimensions 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-dimensions) · [Goravel 尺寸规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go#L1539)
- 固定 Laravel PHP trait 与 OpenRust 实际 multipart HTTP 对照 17 个样本，包括真实 100 × 100 TIFF／ICO、EXIF 尺寸、符号整数、ICO 多条目和不完整目录。有效尺寸匹配与不匹配均通过对照，BigTIFF、TIFF 越界偏移和残缺目录返回 422。三语示例的 `file` 与 `dimensions` 组合已随真实上传执行，Laravel `image` 仍排除 TIFF／ICO。
- 真实 MySQL 8.4.13 环境下，图片、验证 HTTP 和验证路径共 26 项回归通过。根应用 `./artisan inspire`、全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已回收临时 MySQL、3.3 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-tiff.wPcej4uK`。其他 PHP 图片格式与旧式编码表仍待核定，核心账本维持 59/66。

## PSD 与 JPEG 2000 图片尺寸

- 按固定 Laravel `dimensions` 所调用的 PHP 8.5.12 `getimagesize` 读取 PSD／PSB 文件头、JPEG 2000 裸码流的 SIZ 标记，以及 JP2 第一个根级 `jp2c` 码流。JPEG 2000 采用 PHP 报告的参考网格尺寸 `Xsiz`／`Ysiz`。JP2 保留延伸到文件末尾的根级码流行为，也复现缺失根级码流、扩展长度盒和越界偏移的拒绝行为。Rust 按切片边界读取，无须解码像素或引入依赖。固定 Goravel 的尺寸规则和旧版 MIME 表已核对，旧版未找到可复用的尺寸解析。 [Laravel FileHelpers](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/FileHelpers.php#L62) · [Laravel Dimensions 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-dimensions)
- 以真实 PSD、JP2、裸码流及边界夹具共 26 个样本执行固定 Laravel trait 与真实 multipart HTTP 对照。尺寸匹配返回 200，尺寸不匹配、缺少可读文件头或组件计数越界返回 422。三语示例的 `file` 和 `dimensions` 组合实际执行。MySQL 8.4.13 环境下图片、验证 HTTP 和验证路径共 27 项回归通过。三语站点及 18 项浏览器检查通过。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy 通过。已回收临时 MySQL、3.3 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-psd-jp2.peCCCPGJ`。IFF、WBMP、XBM、SWF／SWC 和旧式编码兼容仍待核定，核心账本维持 59/66。

## IFF、WBMP、XBM 与 SWF 图片尺寸

- 对照固定 Laravel 的 `FileHelpers::dimensions()`、PHP 8.5.12 `getimagesize` 及固定 Goravel 的尺寸规则。IFF 读取 ILBM／PBM 的 BMHD，处理奇数长度块的填充。WBMP 读取 Type 0 文件的变长尺寸，保留 2048 上限。XBM 读取预处理宏尺寸。SWF／SWC 从舞台矩形读取 twip 并转换为像素。旧版找到格式 MIME 表，未找到对应尺寸解析。 [Laravel FileHelpers](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/FileHelpers.php#L62) · [Laravel 尺寸规则](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-dimensions)
- SWC 复用已锁定的 `flate2` 1.1.11，声明为框架直接依赖。以固定大小缓冲区读取并检查 zlib 结束及校验和，只保留舞台矩形所需字节。PHP 允许的压缩与未压缩 SWF 走尺寸验证，LZMA 型 SWF 沿用 PHP 的拒绝行为。
- 固定 Laravel／PHP 与实际 multipart HTTP 的 29 个对照样本通过，覆盖合法尺寸、宽高不符、IFF 块长度越界、WBMP 上限、XBM 宏取值、SWF 截断和损坏压缩流。MySQL 8.4.13 环境下共 28 项图片与验证回归通过，其中 SWC 上传回归跨越多个解压缓冲区。三语站点、18 项浏览器检查及文档规则的实际上传组合通过。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy 通过。已回收临时 MySQL、3.7 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-legacy-image.Vc7TozQA`。旧式编码表及整单元对照尚待完成，核心账本维持 59/66。

## 单字节编码字表

- 按固定 Laravel 的 `validateEncoding` 与 PHP 8.5.12 mbstring，补齐 `CP850` 和 `ArmSCII-8`。对全部有定义的编码直接接受任意字节。Windows-1251／1254、ArmSCII-8 和 ISO-8859-3／6／7／8 按 PHP 拒绝未定义位置。Rust 只验证字节，不转码。固定 Goravel 的编码规则和旧版实现已核对，旧版未找到 CP850 或 ArmSCII-8 验证可复用。 [固定 Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding) · [固定 Goravel 编码规则](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/validation/rules.go#L1712)
- 枚举 PHP 下 25 个单字节编码的全部 256 个字节、空输入、全部合法字节组合和全字节组合，共 6,475 个向量。固定 Laravel PHP trait 与真实 multipart HTTP 逐个对照通过。MySQL 8.4.13 的 15 项验证回归通过，覆盖三语示例的字符串形式文件规则与 Fluent 文件规则，以及单字节非法位置的 422。三语站点与 18 项浏览器检查通过。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy 通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-single-byte.VyopsajR`。多字节、状态型编码及整单元对照待完成，核心账本维持 59/66。

## BIG-5、CP950、EUC-KR 与 UHC

- 对照固定 Laravel `validateEncoding` 与 PHP 8.5.12 mbstring。分别核定每个编码的 ASCII、首尾字节范围和未分配位置。BIG-5 与 CP950、EUC-KR 与 UHC 使用各自的 PHP 字节规则，上传内容按原字节验证。固定 Goravel 编码规则和旧版已核对，旧版未找到对应字表验证可复用。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 对原先使用的 WHATWG 解码器逐字节对照，BIG-5 有 4,850 处差异，CP950 有 1,193 处，EUC-KR 有 8,822 处，UHC 的单字节及双字节穷举一致。
- 每个编码覆盖全部 256 个单字节、65,536 个双字节和 1,024 个确定性混合序列，共 267,264 个向量。固定 Laravel PHP trait 与实际 multipart HTTP 逐文件核对 200 与 422，通过全部对照。首轮复验发现 UHC 的 `81 A1` 扩展遗漏，修正并加入回归。三语文档示例随声明式与 Fluent 文件规则实际执行。MySQL 8.4.13 环境下 16 项验证回归、三语站点及 18 项浏览器检查通过。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy 通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-double-byte.HOTrRMmb`。其余多字节、状态型编码和整单元对照待完成，核心账本维持 59/66。

## EUC-CN、CP936 与 GB18030

- 对照固定 Laravel `validateEncoding` 与 PHP 8.5.12 mbstring。EUC-CN 按已分配的双字节位置校验。CP936 接受 PHP 支持的 `0x80`、`0xFF` 单字节及双字节范围。新增 `GB18030-2022`；两版 GB18030 的字节有效性相同，支持 ASCII、双字节和两个已分配的四字节指针区间。校验保留原始字节，版本间的字符映射差异属于转码行为。固定 Goravel 与旧版已核对，旧版未找到可复用的对应验证器。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 单、双字节穷举对照中，原有实现的 EUC-CN 有 733 处差异，CP936 有 260 处，GB18030 有 258 处。两版 GB18030 各自的 1,587,600 个四字节候选逐项对照 PHP mbstring，Rust 字节扫描全部一致。
- 固定 Laravel PHP trait 与真实 multipart HTTP 对照的 268,190 个上传值全部通过，含四种编码的单、双字节穷举，两个四字节区间的边界与截断，以及 4,096 个确定性混合序列。MySQL 8.4.13 环境下 17 项验证回归通过。三语文档中的声明式和 Fluent 规则实际运行，三语站点与 18 项浏览器检查通过。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy 通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-gb18030.Tq5zNRpJ`。其余多字节、状态型编码和整单元对照待完成，核心账本维持 59/66。

## EUC-JP、eucJP-win、CP51932 与 EUC-JP-2004

- 对照固定 Laravel 的 `validateEncoding` 与 PHP 8.5.12 mbstring。补齐 `CP51932` 与 `EUC-JP-2004`。四个变体共用 ASCII 和半角假名扫描，并按各自的双字节和三字节字表校验。CP51932 拒绝以 `8F` 开头的三字节字符，eucJP-win 和 EUC-JP-2004 各自保留与标准 EUC-JP 不同的扩展位置。Rust 校验原始字节。固定 Goravel 和旧版已核对，旧版未找到可复用的对应验证。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 穷举每个变体的 256 个单字节、65,536 个双字节及全部 65,536 个 `8F` 前缀后的双字节组合。原有 WHATWG 解码器对 EUC-JP 有 457 处接受结果差异，对 eucJP-win 有 1,612 处。修订后的字节校验全部通过固定 Laravel 对照。
- 固定 Laravel PHP trait 与实际 multipart HTTP 逐文件对照 529,408 个值，覆盖上述穷举、4,096 个确定性混合序列及截断。MySQL 8.4.13 环境下 18 项验证回归通过，三语文档的声明式及 Fluent 规则实际运行。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-euc-jp.OSERGLP0`。其余 Shift-JIS 变体、多字节与状态型编码和整单元对照待完成，核心账本维持 59/66。

## Shift-JIS 与移动变体

- 对照固定 Laravel `validateEncoding` 和 PHP 8.5.12 mbstring。保留 SJIS、CP932／SJIS-win 的字节契约，新增 SJIS-mac、SJIS-2004 和 DoCoMo、KDDI、SoftBank 三个移动标签。DoCoMo 和 KDDI 在有效字节范围上共用 CP932，SoftBank 另外校验旧式表情转义中的选择符、字节范围与复位。完整选择符和有效表情字节允许在输入结束时省略复位。固定 Goravel 与旧版实现已核对，旧版未找到可复用的对应验证。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 独立穷举显示原 SJIS、CP932 在单、双字节上已与 PHP 一致。本批使用相同的字节扫描和各自的可接受位置来支持其他变体，保留原始输入。
- 八个标签的单、双字节穷举、8,192 个确定性混合序列及 3,330 个 SoftBank 转义样本，共 537,858 个上传值，与固定 Laravel PHP trait 的实际 multipart HTTP 对照通过。MySQL 8.4.13 下 19 项验证回归通过，三语文档的声明式与 Fluent 文件规则已通过实际上传。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-sjis.hj2stQIR`。状态型编码、其余多字节编码和整单元对照待完成，核心账本维持 59/66。

## JIS 与 ISO-2022-JP 系列

- 对照固定 Laravel 的 `validateEncoding` 与 PHP 8.5.12 mbstring。实现 JIS、ISO-2022-JP、ISO-2022-JP-MS、ISO-2022-JP-2004、ISO-2022-JP-MOBILE#KDDI 和 CP50220／21／22 的验证行为。复用日文 EUC 已核定的 JIS 字表，按各变体处理转义、移位和扩展。JIS 与标准 ISO-2022-JP 要求以 ASCII 状态结束，另外六个标签按各自的 PHP 解码检查保留结尾行为。固定 Goravel 与旧版已核对，旧版未找到可复用的对应验证。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 八个标签分别在九种前置序列下穷举所有双字节，累计 4,718,592 个校验结果与 PHP 一致。原 ISO-2022-JP 解码器在该矩阵中有 5,387 处结果差异。
- 固定 Laravel PHP trait 与实际 multipart HTTP 对照 65,584 个上传值，包括转义选择符穷举、截断、移位、恢复 ASCII、扩展字表和 8,192 个确定性混合序列。MySQL 8.4.13 下 20 项验证回归通过，三语文档规则已通过实际上传。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-jis.DkSJ1cSM`。其余多字节、状态型编码和整单元对照待完成，核心账本维持 59/66。

## HZ、EUC-TW 与 ISO-2022-KR

- 对照固定 Laravel 的 `validateEncoding` 和 PHP 8.5.12 mbstring。HZ 与 ISO-2022-KR 复用已核定的 EUC-CN、EUC-KR 字节表，保留转义、移位和输入结尾行为。EUC-TW 按 PHP 采用的旧版 CNS 11643 检查第 1、2、14 平面。第 14 平面的位集合按行生成，对每个 `8E AE ROW COL` 查询 PHP `mb_check_encoding`，将结果存入第 `ROW - 0xA1` 行的第 `COL - 0xA1` 位。固定 Goravel 和旧版已核对，旧版未找到对应验证可复用。 [Laravel 编码规则](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Concerns/ValidatesAttributes.php#L1020) · [Laravel Encoding 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#rule-encoding)
- 独立八组遍历共 524,288 个 PHP 字节结果通过，覆盖 HZ 与 ISO-2022-KR 的普通和移位状态，以及 EUC-TW 的双字节和三个平面前缀。
- 实际 multipart HTTP 复验上述矩阵及 12,032 个固定 Laravel 样本，共 536,320 个上传值，覆盖转义、截断、全部 EUC-TW 平面选择字节和确定性混合序列。MySQL 8.4.13 下 21 项验证回归通过，三语文档规则均随 HTTP 验证执行。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。已回收临时 MySQL、3.1 GiB 本批 Cargo 产物和文档缓存。证据目录 `/private/tmp/openrust-04-05-hz.ppqxtT3w`。当前 PHP 的编码名称中剩四个移动版 UTF-8，整单元对照待完成，核心账本维持 59/66。
