# 文档教程消费者核对

2026-11-24。按用户授权，以固定 Laravel 文档示例发现并修复实际能力缺口，沿用现有章节交付映射和统一差异账本，不另建审计引擎。此记录只关闭已列出的教程子范围，不表示全框架或整章 API 全部对齐。

## 第一组：请求实例与 Foundation 注册的六个入口

固定文档 [URL Generation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/urls.md#validating-signed-route-requests)、[Validation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md)，行为依据 [FoundationServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Providers/FoundationServiceProvider.php#L136-L194) 与同提交 UrlGenerator；框架13.40.0、文档960eedf0。官方raw已联网读取，并与既有固定源码对应注册段核对。

| 教程／公开入口 | 实际问题 | 本轮处理 |
| --- | --- | --- |
| Request::hasValidSignature | 门面有实现，http::Request实例无方法，教程直接调用编译E0599 | 增加has_valid_signature(absolute)，调用现有URL服务 |
| Request::hasValidRelativeSignature | 同上 | 增加has_valid_relative_signature() |
| Request::hasValidSignatureWhileIgnoring | 同上 | 增加has_valid_signature_while_ignoring(ignore,absolute) |
| Request::hasValidRelativeSignatureWhileIgnoring | 同上 | 增加has_valid_relative_signature_while_ignoring(ignore) |
| Request::validate | 实例缺失；既有门面流程没有Precognition字段筛选和成功钩子 | 增加实例入口，复用原Factory／Validator、字段过滤和成功钩子 |
| Request::validateWithBag | 实例缺失，门面独立维护一份流程 | 增加实例入口，仅对Validation错误设置错误包；门面统一转发 |

新方法均为正式实现，没有占位、动态全局请求或第二套签名／验证引擎。v0.x的Request URL模块没有这六个实例入口；其URL服务已有HMAC验签，但使用旧框架结果类型和依赖。沿用当前已接通的Url服务、验证器、请求上下文和Precognition，不重新复制旧签名算法。当前源码按原生Rust命名；PHP的可选absolute参数在Rust显式传入true，relative便捷方法不需要该参数。

### 可复现反例和消费者

- 原始基线aa44ab444：实际Rust编译六个Request调用全部E0599，见request-instance-before.log；额外类型推断错误由缺方法连带产生，不算第七个能力缺口。
- Precognition最初测试放在普通路由闭包中。框架与固定上游都会跳过该动作，因此没有真实调用验证器；该夹具失败不能证明验证器行为。修正为precognitive中间件之后的验证中间件，明确断言请求已经进入预验证，再通过with_container调用Request门面。旧实现对仅验证name的请求返回email缺失422；同一测试在修复后返回204且不执行后续副作用。非法name返回422且错误列表不包含未选中的email。见request-validation-before-final.log。
- 初始夹具的捕获闭包不符合Routing::new的函数指针契约，已改为应用容器注入计数器；没有扩大Router装配接口。缺MySQL前置的一次既有回归失败已按专用夹具重跑，不记作产品缺陷。
- request_signature_tutorial.rs：实际TCP验证四种实例入口，合法／篡改／到期／忽略参数、signed中间件403、跨应用相对签名密钥隔离及轮换；GET含JSON正文时query保留签名值、input允许读取未签名正文，不能混淆可信边界；普通请求验证与具名错误包入口。
- request_validation_tutorial.rs：实际HTTP中间件验证Precognition成功／失败、无副作用、普通请求全字段校验与未验证数据过滤。
- tests/request_tutorial_docs.py按锚点直接提取三语urls／validation的六段正式Rust代码，生成三个真实HTTP Kernel消费者。验证签名链接、正文覆盖边界、401、201、422、字段过滤与长度限制；不维护平行示例实现。生成文件仅在target/tmp隔离源码内。

### Rust适配

增加入口层后，既有完整门面消费者触发默认编译器布局递归上限。将内部validator.validate_request的Future置于一次Box::pin，不要求应用添加recursion_limit或关闭lint，不改变公开async调用形式。每次实际验证增加一次堆分配，不复制请求、输入数据或额外访问网络／数据库；本轮未做性能基准，不声称性能提升。错误类型、具名错误包、真实请求认证解析与既有JSON／Web错误管线保留。

### 验收状态

原始组合的七个目标共41项通过，Precognition浏览器专项ignore1项不计通过；MySQL8.4.13夹具实际运行。包括新增5项真实TCP／生命周期、三语消费者3项，以及既有门面17、URL6、Web验证2、Precognition8项。验收最后组合纳入7c5166af4及本单元源码：同样41项通过；补强具名错误包的预验证分支后再2项通过。全workspace/all-targets严格Clippy（-D warnings）与fmt通过；三语内容／站点构建、常规浏览器、根及/docs/全部导航和无API导出的参考页检查通过；核心all-features Rustdoc生成并逐项确认六个新方法锚点。未构建完整联合API发布产物，未重跑需要固定前端夹具的Precognition浏览器ignore专项。之前磁盘中断的Clippy不计通过。

## 范围与后续教程队列

- 六个入口的基础教程、本轮参数组合与生命周期属于本组；并不因此关闭完整Request、Validator或URL模块。
- 请求validate的PHP额外messages／attributes参数、ignoreQuery闭包参数、完整URL章节的current/full/previous、模型键、URI组合、默认值／优先级及无效签名自定义渲染等，继续按现有章节映射逐例核对。已有内部能力或同名门面不代替实例参数、教程和消费者证据。
- 其他所有章节仍以laravel-docs-delivery-map.md与framework-parity-data的逐项记录为准，不能把此前403目标回归等同于所有官方教程通过。
- Settings按其settings-coordination/README.md最终闭合范围保留已闭合状态；生产多机／滚动发布、Windows及其他缓存驱动专项归发布验收，不重新作为该模块功能开发缺口。
- 本轮不覆盖异常处理等其他在途修改，不推送。

## 收尾记录

最终证据在target/tmp/docs-tutorials-20261124/：base-commit、final-base-commit、request-instance-before.log、request-validation-before-final.log、final-combination.log、gates-final.log、docs-build.log、docs-browser.log和rustdoc.log。普通指南不启用API导出，核心Rustdoc单独核对六个方法，不混称完整站点联合API发布验收。Rust1.94.0，Mac ARM64；本批编译符号仅以子进程环境设为0，正式profile／增量／正常Cargo编译并发未改。

第一次严格检查触及32GB停线，守卫返回75并记录停止时权限错误，未算通过；同项目的清理记录随后确认profiles清理与约62.8GB释放，空间恢复后继续。源码、日志、下载、数据卷和新在途修改保持；最终保留当前热缓存和验收快照，不为收尾再清空缓存。临时MySQL由正式夹具创建并回收，无开发库写入。

本轮实际新增行为只涉及请求的六个标准入口及门面统一流程，未重开已闭合Settings，也未改动其他进行中的异常／文件系统／认证等代码。代码以PHP默认Provider宏的职责固化为Rust方法，不提供PHP式运行时替换同名实例方法的承诺；现有宏与容器扩展方式保持。对照不依赖PHP运行时，本轮未重新执行完整PHP／PHPUnit，对已有固定源码与已验核心行为的引用不伪装成新上游运行证据。

## 第二组：URL 查询规范化、验签过滤回调与异常渲染

2026-11-24，基线63e6d328a，沿用同一固定文档／框架提交。重新运行固定框架的UrlGenerator／Request（PHP8.5.12、Symfony7.4），三种地址的current/full/to根地址输出保存在target/tmp/docs-chapters-20261124/reference/url-oracle.json，脚本和固定源码保留。PHP实际确认current根地址不带尾斜杠，full根查询使用/?；测试最初错误要求current根地址带/，已依据实际PHP修正，未改动正确的根地址生成机制。

- 原Url::full直接返回原始path_and_query，与上游request.fullUrl不符。现复用Request::normalize_query_string，移除路径尾斜杠、排序查询键、规范编码。旧回归曾要求原始顺序与小写%2b，已按上游真实结果更新为a=1&z=%2B；签名校验仍使用原始查询串，未对验签原文规范化。
- 请求实例与Request门面的两种while_ignoring入口接受Into<SignatureQueryFilter>，数组／切片／Vec转换为Keys，回调显式传Predicate。保留现有数组写法，复用已有回调过滤引擎；不忽略expires的现有安全约束保持。
- 实际TCP验证自定义InvalidSignature渲染器在过期链接返回自定义403，而未选中的路由仍使用默认403。没有改写异常模块或创建第二套渲染管线。
- 签名回调与Vec实际消费者纳入既有跨应用密钥隔离、轮换、正文不受签名保护等用例；新增current/full根地址、根查询、非根尾斜杠的实际HTTP断言。

三语URL正文已同步规范化说明与回调／异常闭包示例。此组关闭列明子范围，不把继承的URL其他功能回归当成整个章节逐例通过。APP_URL显式优先和previous同源过滤是既定项目行为；PHP默认从请求推导来源、previous允许外部Referer，仍明确保留差异。后续继续核对URI、模型键和默认值优先级；不能据此关闭完整URL类型。

验收：request_signature_tutorial5、urls6、uri_helpers2，共13项通过。严格workspace/all-targets Clippy的首次32GB停线中断不计通过；安全清理恢复后重跑退出0，见url-clippy-resumed.log。fmt与三语站点构建通过。清理只涉及构建profiles，源码／日志／依赖／数据保留；守卫触线后实测可用约94GB。后续最终消费者补验结果写入本记录，不混入Validation尚未验收源码。

URL第二组最终补验：清理后实际重编译，request_docs_generated三语3项、签名5项、urls6项、uri2项，共16项全部通过；见url-accepted.log。url-fmt.log和url-docs-build.log退出0，最终8份源码／三语正文与隔离验收字节一致，SHA见url-source-sha.json。

## 第三组：验证消息与字段显示名贯通开发者入口

基于a746dffed及本组独立差异，不包含进行中的优先级、Vite、异常或模板修改。固定FoundationServiceProvider的validate宏将可选参数转交Factory::make；固定FormRequest::createDefaultValidator将messages()/attributes()传给同一Factory。v0.x验证实现未提供可复用的对应FormRequest钩子；现有Factory已支持两个映射，因此只补必要的公开接线，不复制验证器。

- Request::validate_with(rules,messages,attributes)、validate_with_bag_and_options(bag,rules,messages,attributes)，Request门面同名转发。
- Validator::make_with(data,rules,messages,attributes)，原make(data,rules)继续传默认空映射。
- FormRequest增加默认返回空映射的messages()/attributes()钩子，创建原验证器时传入，之后with_validator、after、授权、预验证及错误包管线保持原顺序。
- Rust无可选函数参数，原validate/validate_with_bag调用保持，额外参数使用明确方法；两个BTreeMap只属于当前验证器，不修改共享语言资源、应用Factory或其他请求。没有新增进程全局状态。

实际TCP覆盖实例、门面（含具名）、手动Validator与FormRequest的required/min消息占位和字段显示名、成功字段过滤；具名请求与表单走Web 302，携带真实会话cookie读取flash，确认article错误包保存自定义消息。五种预验证路径均为204+成功响应头，失败为422自定义消息，验证位于实际中间件而非被跳过的路由动作。新增三语正式示例由tests/request_tutorial_docs.py直接提取，累计九段，三组消费者真实编译并执行422和201断言。

最终组合32项通过（request_facade_full17、request_signature_tutorial5、request_validation_tutorial2、validation_options_tutorial3、validation_web2、三语提取3），0忽略。工作区严格Clippy之前用独立来源运行，未带优先级等在途实现；workspace/all-targets -D warnings、fmt和三语站点构建退出0。证据target/tmp/docs-chapters-20261124/validation-{final-v2,clippy,fmt,docs-build,doc-extraction}.log；首次夹具缺少Session trait导入编译失败已修正，未算作框架缺陷或通过证据。

本组只关闭可选消息／属性参数和FormRequest对应钩子；完整Validation章仍须逐项核对，包括固定文档的未知字段拒绝和现有明确未实现的email扩展模式，不以本组32项回归关闭整章。

## 第四组：URL 默认值优先级与完整章节教程对账

新增configuration::Middleware::prepend_to_priority_list和append_to_priority_list。按固定Configuration/Middleware及Foundation/Http/Kernel的相对优先级机制，延后装配相对条目，允许priority在相对调用之前或之后设置；prepend选择最早目标，append选择最晚目标，无目标时追加，已有类型不重复加入。数组／类型名在Rust用TypeId列表表示。只在应用装配时处理，路由请求仍复用既有优先级排序，不新增每请求注册或进程全局状态。v0.x没有可复用的同等配置入口，使用当前MiddlewareRegistry。

实际HTTP的路由故意按“后置→绑定→默认值”注册，应用装配后观察为defaults→binding→after。默认优先级与显式重设priority两种配置均通过，绑定得到真实模型值；同请求的命名路由、控制器action、query数组及Uri复用locale=fr默认值。独立候选经touch首方源码强制重编译；一次复制保留旧mtime导致Cargo使用旧库并报缺方法，已纠正缓存来源并重跑，不视为实现缺口。MySQL夹具两轮均安全回收。

| 固定URL文档章节 | 本地入口与教程消费者 | 状态／差异 |
| --- | --- | --- |
| Generating URLs：路径、合并查询、覆盖、数组 | Url::to/query；url_defaults_tutorial实际HTTP、urls固定PHP夹具 | 通过；数组用显式UrlValue::Array保持顺序 |
| Current URL：current/full | 第二组实际PHP oracle与TCP | 通过；full已修复规范化 |
| Previous URL／path、session previousRoute／Uri | urls真实会话回归 | 通过既定同源策略；上游允许外部Referer，本项目保留同源过滤，不声称完全等价 |
| Named routes：多个参数、额外查询、自定义模型键 | urls；eloquent_route_binding实际HTTP+MySQL8.4.13及SQLite | 通过；选定accessor/cast失败传播，模型不因生成URL额外查询 |
| Signed／temporary／relative | request_signature_tutorial及urls实际HTTP | 通过；SystemTime表示过期时间，签名原始查询不改变 |
| Request验证、忽略键／回调、signed中间件 | 第二组实例、门面、数组/Vec/回调与403实际TCP | 通过；忽略expires的现有防护保持 |
| Responding to Invalid Signed Routes | with_exceptions render::<HttpError>匹配InvalidSignature | 过期自定义403及普通默认回退通过 |
| Controller actions | Url::action；url_defaults_tutorial实际HTTP | 通过；Rust显式注册稳定action标识，不反射PHP类名 |
| Fluent URI | Uri::to/route/action/signed_route/temporary_signed_route、with组件方法；url_defaults_tutorial HTTP及uri_helpers | 通过列明教程；签名结果与同请求Url输出一致，组件验证保留 |
| Defaults及Middleware Priority | defaults→binding→after，含晚调用priority | 新增相对优先级入口并通过；默认值仍按请求隔离 |

本表只核定本章教程组合，不替代每个UrlGenerator／Uri公开方法的完整类型审计。APP_URL显式优先是既有项目选择，固定文档默认使用请求来源；已在三语保留说明。全章记录列明这两项行为差异，不能称为逐字节完全等价。

验收：独立MySQL8.4.13模型绑定2项（含SQLite回归）、中间件生命周期5项、默认值1项、urls6项，共14项通过。补强Uri所有工厂与链式组件后的默认值1项再通过。workspace/all-targets严格Clippy及三语构建通过；新增测试最终再定向严格Clippy和fmt。三语浏览器、根及/docs全部导航、无API导出的参考页检查均退出0。日志位于target/tmp/docs-chapters-20261124/defaults-*.log及chapters-browser.log；原源码和日志保留，不重复清空热缓存。
