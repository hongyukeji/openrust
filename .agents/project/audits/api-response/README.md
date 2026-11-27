# 统一业务 API 响应

日期：2026-11-24。范围为用户确认的业务 JSON 封装、根应用公开信息接口及嵌套字段编辑；不是全部门面对齐结论。沿用[默认 API 基础](../api-basics/README.md)，不新建另一套安装或响应管理器。

## 依据与决定

- 固定 Framework v13.40.0（1449bf241017087fbd38c118ecaabef3f6797fdc）：[ResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/ResourceResponse.php)、[PaginatedResourceResponse](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Resources/Json/PaginatedResourceResponse.php)、[异常 Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)。复用 Resource、分页 links/meta、异常渲染及最后响应钩子；统一业务封装是用户确认的项目扩展，不能称为上游默认格式，也不是 JSON:API 协议。
- 成熟方案参考：[laravel-responder](https://github.com/flugger/laravel-responder)、[Fractal](https://github.com/thephpleague/fractal)。采用简单序列化器及元数据扩展思想，不引入额外依赖、Transformer 管线或 JSON Patch 系统。错误保留真实 HTTP 状态；不以 HTTP 200 表示所有失败。
- 已核对同级 openrust-v0.x（6d1a81924）的 framework/src/core/error.rs：保留 success/code/message、可选 data/meta/errors 的契约思想与 PageMeta；不移植旧版兼容 HTTP 状态模式、自动时间戳和请求 ID 等未约定默认字段。
- 配置依赖 001e18c44：api.response_envelope 读取 API_RESPONSE_ENVELOPE，未设置默认 true，不向 .env.example 添加该环境项。显式 ApiResponse 始终返回业务格式；开关作用于选中的 JSON 异常及普通 Resource，不盲目重写原始 JSON、文件、流或 HTML。JSON:API 保持原协议。

## 公开契约与接线

- openrust::http::{ApiResponse, ApiResponsePath, PageMeta}。success(data)、empty()、failure(status, code, message)、with_status、with_meta、with_errors、with_message、with_message_key、additional、with_field、without_field。失败构造器的 message 是开发者明确选择的安全文字；需要本地化时使用 with_message_key。默认成功消息通过现有 Lang 在最终序列化时解析，使用请求语言、参数替换、语言回退及应用覆盖，不创建全局静态语言状态。
- 常规格式 success/code/message，业务 code 是字符串，例如 ok、validation_failed。缺失／null data 默认省略，合法空对象、空数组、false、0、空字符串保留。1xx／204／205／304 无正文；有正文时，显式响应的成功标记和 HTTP 状态必须一致。字段编辑只操作 JSON，不改变 HTTP 状态。
- additional 接受 JSON Map 或键值对迭代器，浅合并字面顶级键。with_field／without_field 接受根起点的点路径，也可传片段数组以访问含点号的字面键。按调用顺序应用，后者覆盖前者；缺失删除无错误，缺失对象自动创建，null 可作空占位，经过非 null 标量则报序列化错误，深度使用现有 data 工具限制。
- 复用现有 Arr/data 规则：* 是字面键；连续数字键可表现为数组，跳过下标或删除中间元素保留键并表现为对象，不自动重排；字符串删除优先删除完整的顶级字面键。开发者可覆盖／移除标准字段或显式加回 data:null，负责定制后协议一致性。
- 普通 Resource 使用原有分页 data/links/meta，再补默认业务字段；显式 Resource 元数据优先。状态映射复用异常的稳定 code，避免错误 Resource 返回 success:true。JSON:API、with_response、异常自定义 render/respond 钩子继续生效。业务错误不泄露异常 source，即使 APP_DEBUG=true；翻译源损坏返回安全 500，不把诊断文字当成功消息。
- 框架现有18语言各新增 response.json，由现有 CATALOGS／FileLoader／lang:publish 消费。应用无需预置语言文件。根应用 ApiController::index 从 Config 读取 app.name/app.version，routes/api.rs 注册 GET /，随 API_PREFIX 成为 /api 或 /api/admin。无需数据库或认证；不改变可选认证安装机制。自定义业务路由继续受 install:api 覆盖保护，--force 仍可能替换路由文件。

## 验收证据

独立源码快照 target/tmp/facade-alignment/source；复用独占缓存 target/tmp/facade-alignment/cargo-target，日志和脚本 target/tmp/api-response/。Cargo 通过 tests/build_guard.py、Rust 1.94.0、--locked、DEV/TEST DEBUG=0 执行。support/mod.rs 的候选只包含本批路径工具导出，不包含协作者在途 report 导出／实现。

- final-root.log：api_info 5项真实 TCP HTTP；application_skeleton 2项；startup 28项（真实 Artisan 安装、源码生成及启动消费者）；welcome 36项，共71项通过。HTTP 覆盖配置名称/版本、18语、并发隔离、请求内切换语言、自定义翻译参数和回退、默认及自定义前缀、开关关闭、404/405/401/422/429/500、Allow/Retry-After、空载荷、顺序编辑、分页、非成功 Resource、JSON:API、损坏翻译源。
- regression.log：真实 Artisan lang:publish 在根 lang 和旧 resources/lang 发布 response 组，重复执行与中文资源保护通过（该命令带筛选器，其他目标显示0项不计通过）。
- resource-regression.log：SQLite API Resource 数据库回归1项通过；该命令筛选掉的其他测试不计通过。
- field-regression.log：显式响应2项、字段操作6项、data_helpers 3项、support_arr 3项通过。覆盖数组追加/稀疏/删除、字面键、深度、标量冲突、状态、空值和序列化错误。fields-resumed.log 使用最新状态映射版本复验14项，全部通过。
- docs-build.log：三语内容检查与 VitePress 构建通过。docs-browser.log：Chrome 桌面/移动端、三语、根挂载和 /docs/，12组页面与锚点检查通过。
- clippy-final.log 首次最终检查因32GB停线被中断，不能计通过。守卫停止进程组时报 Operation not permitted；确认无 Cargo/rustc/clippy 后重新调用守卫，持所有 profile 锁清理完成，保留锁 inode、源码、日志、依赖下载及数据库。disk-cleanup.log / build-guard-events.jsonl：释放83,040,133,120字节，可用115,034,480,640字节。未删除或改写现有数据库、数据卷。

- clippy-resumed.log：完整 workspace/all-targets 严格 Clippy 通过，耗时2分30秒；fmt.log：完整 workspace 格式检查通过。
- mysql-resources.log：独立 Docker MySQL 8.4.13、OPENRUST_REQUIRE_ELOQUENT_MYSQL=1，真实 Resource 数据库消费者通过1项；临时 tmpfs 容器自动清理，标签复查无残留，不使用或删除数据卷。
- 安装发布模板 api-config.stub 同步响应开关及严格布尔解析，保持可独立手动接线，不要求存在应用私有 env 模块；接受 true/false、1/0、括号形式（大小写不敏感），非法值报配置错误，与根应用 get 的布尔契约一致。startup 在真实环境装配中消费模板并检查8种情况，安装器仍保留用户配置。

- template-verified.log：安装模板更新后 api_info 5项及 startup 28项复验通过，包含8种布尔输入；首轮夹具使用捕获闭包不符合配置装配 fn 指针契约，修为无捕获后重新编译运行，失败轮不计通过。clippy-delivery.log：最终 workspace/all-targets、-D warnings 再次通过（24.81秒）。

实际命令（均在上述守卫／缓存下执行）：

```sh
cargo +1.94.0 test --locked --manifest-path target/tmp/facade-alignment/source/Cargo.toml --test api_info --test startup --test application_skeleton --test welcome
cargo +1.94.0 test --locked --manifest-path target/tmp/facade-alignment/source/Cargo.toml -p openrust-framework --test api_response_fields --test api_response --test data_helpers --test support_arr
cargo +1.94.0 test --locked --manifest-path target/tmp/facade-alignment/source/Cargo.toml -p openrust-framework --test api_resources resource_responses_on_sqlite
python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/api-response/mysql_resources.py
cargo +1.94.0 clippy --locked --manifest-path target/tmp/facade-alignment/source/Cargo.toml --workspace --all-targets -- -D warnings
npm --prefix target/tmp/facade-alignment/source/docs run build
node target/tmp/api-response/browser.mjs
```

数据库包装脚本将独立实例 URL 映射到 OPENRUST_TEST_ELOQUENT_MYSQL_URL，同时设置 REQUIRE，避免缺环境静默跳过。源码指纹保存在 target/tmp/api-response/delivery-sha256.json；最终候选排除其他协作者在途错误教程、report helper 与路由性能原型。

## 边界

本批未改数据库查询或队列协议；没有以 Worker 或真实厂商投递冒充 HTTP 封装验收。其他数据库版本／操作系统发布矩阵不在本批，已有未受影响路径不重复计新验收。已有 API 前缀边界提交653ecd726及配置提交001e18c44单独保留，不夹带重复提交。未推送。

收尾检查可用空间 54,093,144,064 字节（54.09 GB）；无本批临时数据库容器遗留。正常增量缓存继续复用，已废弃编译产物在本轮停线时直接删除，不进废纸篓；验收日志和源码保留。

## 完整教程示例补齐（2026-11-24）

- 三语 responses 正文补齐最简单成功响应，空载荷／空对象／空数组和204无正文，嵌套字段与数组修改／追加的完整 JSON、数组中间删除及跳过下标保留数字键的对照，以及中英语言文件和完整响应对照。明确默认消息示例使用 en 请求语言，避免把教程语言与请求语言混淆。保留既有成功／201创建／422字段错误／分页示例，注册函数明确 pub(crate)，JSON 采用多行排版便于移动端阅读。
- 从三语正文实际提取8组 Rust、13组 JSON，各语言代码／数据完全一致且 JSON 可解析。只读核对本轮编译快照的 ApiResponse 与 Arr/data 实现和当前对应文件一致，复用既有隔离快照及独占缓存构建框架库；未把其他协作者的异常处理和路由原型在途修改纳入快照或提交。
- 使用提取的函数注册真实公开路由，在 en、zh-Hans 两个应用配置下完成26个TCP HTTP请求，逐项比较完整正文和状态，包括200、201、422、204零字节正文、分页、嵌套编辑、数组稀疏键及参数翻译。最终 extraction-final.log、runtime-final.log 通过；没有仅凭 Markdown 正则检查宣称示例可用。
- 最终 npm --prefix target/tmp/facade-alignment/source/docs run build 通过，包含三语内容／链接检查；Chrome 在 / 和 /docs/ 挂载下完成三语桌面／移动端12组检查，每组核对新增章节锚点、正文、页面脚本及页面无横向溢出；简体中文桌面和移动截图复查通过。日志、提取源码和截图保存在 target/tmp/api-response-examples/，不在正式正文放验收过程。
- 命令均通过 tests/build_guard.py 执行。一次性独占示例测试可执行文件验收后直接删除，不进废纸篓；实际大小及清理前后磁盘读数见 cleanup.json。保留源码、日志及正常复用的 Cargo／文档构建缓存，不触碰数据库或数据卷。本轮只提交三语正文和本条记录，不推送。

## 移除全局自动封装

按上述固定 Laravel 基线恢复框架默认 JSON 异常和资源响应。根应用配置与 `install:api` 的生成模板只配置 API 前缀。旧环境变量或配置缓存中的封装设置不再参与响应生成。

- 默认异常使用 `message`，验证异常另含 `errors`。生产 500 使用安全消息，调试时沿用框架的诊断字段。HTTP 状态和 `Allow`、`Retry-After` 等响应头沿用原有分派逻辑。
- 普通 `JsonResource` 和集合按资源自身规则包装数据，分页继续提供 `data`、`links` 和 `meta`。JSON:API 沿用协议资源的格式。
- 默认 `GET /api` 由控制器将 `ApiResponse::success` 传给 `Response::json`，统一格式包含 `success`、`code`、`message` 和 `data`。Fortify 等应用中显式构造的业务响应仍按各自契约输出。

本次在隔离源码中使用 Rust 1.95.0 验证。旧环境变量分别为 `true` 和 `false` 时，真实 TCP 请求覆盖默认 `/api`、401／404／405／422／429／500、资源分页和 JSON:API。安装模板也验证了旧设置为非法值时仍可只读取 API 前缀。相关测试通过 34 项。启动套件另有一项现有失败，断言启动错误省略底层消息，而 `BootstrapError::ProviderBootFailed` 的现有 Display 实现会附加底层错误。本批验收复跑跳过该项。

## ApiResult 与 HTTP 响应分离

OpenSpring 的 `CmsContentApiController` 将 `Result.success(data)` 交给 `ResponseEntity`。OpenRust 沿用这个职责划分，将已有业务 JSON 对象改为 `ApiResult<T>`，由 `Response::json` 指定状态并生成响应，取消路由专用转换。保留成功标记、业务码、消息翻译、空数据省略和字段编辑规则。调用方显式使用 `Response::no_content()` 或 `Response::empty(status)` 返回无正文响应。

默认应用和生成的认证 API 显式构造 `ApiResult`。默认异常处理器对 API 路由及 API 前缀下的请求产生失败的 `ApiResult`，包括验证字段错误，保留原有 HTTP 状态及 `Allow`、`Retry-After` 等响应头。空前缀时根据实际匹配的 API 路由识别异常。显式自定义响应、HTML、普通 Resource 包装和 JSON:API 继续由各自入口决定格式。

验收使用 Rust 1.95.0。全工作区测试目标检查通过，并用 `openrust-fortify/socialite` 复核可选入口。独立应用 HTTP 验证默认及自定义前缀的 200／404／405，核对 HEAD 空正文、`Allow` 和前缀之外的 HTML 响应。根应用与框架 26 项相关测试通过，包含独立 MySQL 8.4 的 Precognition 验证。另有两项 Fortify 真实 HTTP 测试通过，覆盖密码令牌签发、状态码和最近确认。Precognition 的浏览器辅助脚本用例按原测试配置忽略。

## Fortify 业务 JSON、普通资源与空 API 前缀

沿用上节的 Laravel 资源响应和 OpenSpring 业务数据／HTTP 分层对照。检查了旧版 `v0.x` 的 `core/error.rs`，其中业务类型持有 HTTP 状态，不复用该职责。当前实现复用纯数据 `ApiResult`、资源响应钩子和异常渲染器。

- Fortify 默认业务响应覆盖登录及待双因素挑战、密码确认及状态查询、注册、密码重置、账户更新、双因素管理和验证邮件重发。保留各入口的状态和响应头，返回 204 的入口继续生成空正文。Fortify 路由仅对已协商为 JSON 的默认异常启用业务封装。会话 Passkeys 从该中间件组排除。
- 普通 API 资源使用 `data` 承载数据，保留顶层 `links`、`meta`、其它非保留元数据及 JSON 编码选项。资源声明的 4xx／5xx 使用失败业务格式。Web 包装和 JSON:API 协议由原入口处理。
- 异常处理在尝试会话验证重定向之前识别已匹配的 API 路由。空前缀时，带会话且请求 HTML 的 API 验证失败仍返回 422 和字段错误，普通未匹配路径继续按请求协商。

隔离源码使用 Rust 1.95.0 运行根应用、Fortify（含 Socialite）和框架的相关测试。167 项通过。真实 TCP 覆盖会话与空 API 前缀、资源状态和响应头、SQLite／独立 MySQL 8.4.13 的资源分页、Fortify 的成功和失败响应，以及 JSON:API 和会话 Passkeys 的既有格式。默认应用二进制另验证了 `/api`、`/api/admin`、HEAD 空正文和 405 的 `Allow`，确认前缀外的缺失路径仍返回 HTML。
