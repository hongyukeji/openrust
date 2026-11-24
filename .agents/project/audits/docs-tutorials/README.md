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

## 第五组：未知字段拒绝与教程中的Blade错误回退

基于967b29334的独立候选。固定FormRequest::validateNoUnknownFields读取JSON／表单正文叶子键，对照原始validationRules，放入after回调；排除_token/_method，不核对查询与上传。已用固定PHP8.5.12／框架13.40.0实际执行七组嵌套、通配符、字面点、确认字段及空键案例，保存为reference/form-request-unknown-fields.json正式测试数据；原PHP脚本与输出留在target/tmp/docs-chapters-20261124/reference/unknown-fields-oracle.*。

- FormRequest::fail_on_unknown_fields返回Option<bool>：Some(true)/Some(false)显式启闭，None继承应用默认；默认关闭。
- 新的foundation::http::FormRequestDefaults由原ValidationServiceProvider注册，Provider::boot可启用应用默认。PHP进程静态布尔值适配为每应用AtomicBool，不影响其他应用；真实双应用反例证明隔离。没有新增环境变量、额外验证器或进程全局缓存。
- 仅在启用策略时读取正文缓存并收集键及路径片段，不再复制JSON字段值；复用RuleSet路径解析、原译文、错误包、after与Precognition顺序。未声明长字段名返回422，不因把不可信键当作受限规则路径解析而返回500。仍遵守既有输入大小／深度预算，没有性能基准结论。
- FormRequest准备钩子仍按现有Data接口运行，不把Data的单独修改自动写回HTTP正文；本策略读取真实正文。该既有Rust适配不等同于PHP Request子类对象别名完全对齐。

实际HTTP补验中，官方@error('field','bag')/@else/@enderror返回500。原AST只有错误块且解析器遇到Else拒绝。新增私有fallback块及渲染分支，空错误显示clean，具名真实会话flash显示错误；编译缓存版本从45升46并验证旧缓存失效重建。共享工作区另有Vite变更占用46，因此工作区保留合并版本47及对应测试；本单元暂存／验收只含46和本组三个AST修改，未夹带Vite注册、宏或声明。

额外PHP oracle发现其@error带@else时$message清理仅在else分支，错误为真可能泄漏到外层变量。Rust沿用原局部绑定，不复制这一PHP编译副作用；官方教程HTML错误／空状态分别已验收，但不声称变量泄漏完全相同。原输出保留于error-else-oracle.json，无改写历史证据。

最终冻结组合：22个目标121项通过、0忽略，含真实MySQL8.4.13验证presence／typed FormRequest／文件规则、验证器生命周期／扩展／消息、31项请求与三语／未知字段消费者，以及视图和编译缓存回归。日志unknown-all-final.log；此前unknown-all-accepted.log同为121项，之后补强了长字段名实现与断言再全量通过。未知字段专项4项包含七组固定PHP数据、JSON/URLencoded/multipart、真实Web302具名cookie及Blade两个分支、应用默认启闭与Precognition。tests/request_tutorial_docs.py累计直接提取十二段正式三语代码，三组HTTP Kernel消费者通过。新增无必要引用触发的一次Clippy失败已修正，无lint豁免；unknown-clippy-frozen.log的workspace/all-targets -D warnings和unknown-fmt-frozen.log退出0。三语构建、浏览器、两种挂载的全部导航和无API导出参考检查通过。

### Validation章节当前对账边界

| 教程部分 | 本轮或复验消费者 | 当前结论 |
| --- | --- | --- |
| Request即时验证、自定义消息、具名错误包、JSON422 | request实例／门面、options、三语直接提取 | 已通过列明入口组合 |
| 授权、prepare/passed、typed FormRequest注入、文件上传 | validation_http真实HTTP（含MySQL）、current_password、request_value | 既有消费者复验通过；Data准备接口仍为既定Rust适配 |
| after、bail、stopOnFirstFailure、safe/validated | validation_lifecycle／mutation／extensions | 56项章节回归中的对应类型已通过，不代表全部上游类方法 |
| 未知字段拒绝、应用默认与单表单覆盖 | 新增4项真实HTTP与PHP数据 | 本组已接通并通过 |
| 重定向、旧输入、具名错误、敏感字段、Blade@error/@else | validation_web、form_request_unknown_fields、视图回归 | 本组修复else，实际HTTP通过 |
| 条件、嵌套、标量、邮件RFC、DNS、密码、文件、数据库规则 | validation_paths/scalars/email/dns/password/http等 | 本轮复验通过现有实际支持范围；不能视为所有规则教程完整对齐 |
| 创建Form Request：make:request | 核对发现仍列在console/planned.rs | 明确未闭合，接续正式生成器及生成文件消费者验收 |
| 每字段Closure规则、Implicit Rule对象、Rule fluent builders/anyOf/forEach | 类型／入口待完整核对，现有extend_*不代替专用入口 | 接续教程差异，不写为语言不适用或已完成 |
| email DNS/spoof/strict/filter等扩展模式 | 现有明确拒绝，不能用RFC通过替代 | 仍为已知功能差异，后续独立实现／验收 |

本组关闭具体新增入口和失败路径，Validation整章仍未宣布闭合。以上未完成项继续当前开发，不推送，不混入其他在途源码。

## 第六组：make:request正式生成器与生成文件HTTP消费者

固定RequestMakeCommand与request.stub要求name、-f/--force、默认Http/Requests命名空间、可覆盖stub，以及authorize默认false和空rules。原CLI已实际复现：`make:request StoreArticleRequest`退出1并报告unexpected argument，根本未生成文件，见request-make-before.json。v0.x没有对应可复用的请求生成器；复用当前GeneratorCommand、source_name_parts、原子write_generated和Stub发布机制，不新建脚手架系统。

- 新增正式RequestMakeCommand，目录app/http/requests，嵌套／App前缀名称按既有生成器规则转换为Rust模块文件名；拒绝无效名称、路径穿越、Rust保留模块名及文件／目录模块冲突。
- 新增request.rs.stub并接入Stub::ALL与应用级stubs覆盖；生成Default表单类型，提供authorize和rules，默认明确拒绝。框架FormRequest默认授权行为不改，只对新生成的表单按固定上游默认拒绝。
- -f覆盖文件，无force重名保留文件、报告冲突且退出0（与固定通用生成器返回false的退出契约一致）。生成器不改写用户模块声明；Rust需要开发者在父mod.rs显式声明后编译，三语说明准确列出步骤。
- 新增命令帮助、消息、默认未授权提示和注释采用简体中文；保留准确crate导入、CLI命令及参数。模板为空规则属于新应用代码的开发起点，不是框架占位命令。

实际验收：根二进制3项请求生成器和2项资源／发布回归通过；tests/request_generator_tutorial.py直接提取三语创建章节各2条shell命令，通过真正`./artisan`分别在三个独立应用执行，三语生成文件字节一致。复制真实生成代码到隔离测试夹具（SHA前后一致），再编译为typed FormRequest动作，真实TCP确认默认403且动作不执行；仅自定义模板明确授权后，缺失／短title422，合法title201且admin过滤。发布和重名／force／无效路径均从实际二进制验收，未用平行手写实现冒充生成器输出。

在本地HEAD新增0684e8e01后刷新隔离来源为该提交＋本组差异，touch首方源强制实际编译，再取得5项CLI与1项生成HTTP通过；request-generator-final-combination.log、request-generated-final-combination.log。workspace/all-targets严格Clippy和fmt、三语构建通过，浏览器最终来源见request-generator-browser-final.log。较早一次测试夹具误用了根应用未依赖的tempfile与PathBuf借用已改为既有标准库临时目录模式；一次typed动作返回裸Response不符合既有IntoRouteHandler契约，改为正式Result签名，未把它伪装成框架缺陷。

三语新章节creating-form-requests已同步路径、显式模块声明、默认拒绝、退出码和stub覆盖。第五组表中make:request缺口由本组关闭；每字段Closure／Implicit对象、make:rule与fluent builders等仍按专用入口接续，email扩展模式仍非已完成。源码／日志位于同一项目target/tmp下，独立测试应用已自动清理，生成消费者夹具和热缓存供后续验收复用；不推送。

第六组最终文档复验发现既有check_scope把所有带support-status的页面限定为恰好3个h2，已扩展的已提交Vite页面含6个h2而失败；这不是生成器或页面内容空缺。检查改为逐项要求唯一的support-status／available-entry-points／scope三个h2锚点，继续保留实际HTTP200、非空正文、原生引用、三语覆盖和浏览器错误断言。完整两种挂载／三语导航复验及普通浏览器、无API导出参考页均退出0，见request-generator-browser-rechecked.log；首次失败保留，不计通过。未修改Vite功能或其在途字体实现。

## 第七组：字段闭包、隐式对象与原始字段名

固定ValidationRuleParser／InvokableValidationRule将一次性Closure包装为规则，隐式对象的public implicit标志控制present／nullable检查；固定Validator在调用自定义规则前还原占位字段名。v0.x已有for<'a> Fn(Field<'a>)式同步自定义规则，但没有当前对象隐式标志；提取其借用／Send+Sync模式，沿用当前ValidationRule、RuleFailure和原验证器，不恢复旧RuleError协议。

- 为同步Fn(&RuleContext,&str,Option<&Value>)->ValidationResult<Vec<RuleFailure>>实现ValidationRule，直接供Rule::custom使用；不注册全局规则名，原异步规则对象签名保留。
- ValidationRule新增默认false的is_implicit钩子；隐式对象在缺失、空字符串或nullable null时进入原validate，sometimes仍能跳过缺失字段。标志不自动制造required错误；对象可以明确通过。隐式对象失败不自动等同builtin required的停止行为，非bail时后续规则仍执行。
- 对自定义规则统一传递还原后的字段名，字面author\.name在规则内看到author.name。普通名称用Cow借用，仅有转义时分配字符串；不声称有基准性能提升。

固定PHP实际执行十组普通／隐式／nullable／sometimes／明确通过组合，保存validation-rule-scheduling.json并通过真实TCP回放。一次初始夹具直接读request.json，混入已执行的HTTP空串转null，导致与裸Validator输入不一致；改为读取原始body_bytes提供同样JSON给手动Validator，不修改生产HTTP归一化。正式请求入口仍使用原输入处理，三语直接提取的request.validate示例同时覆盖正常HTTP归一化。

最终基于e61dc0279的独立组合：18目标89项通过、0忽略，包含真实MySQL8.4.13的presence、typed FormRequest、文件和验证器生命周期；新增2项实际TCP验证字段闭包、其他输入Data读取、字面点、bail、执行错误500后恢复、普通／隐式调度和具名类型。补强隐式失败后第二条规则继续执行的断言，再2项通过；未将其算为两个额外功能。三语十八段正式Rust代码直接提取为3组HTTP Kernel消费者，覆盖闭包422、合法201与隐式缺失拒绝。workspace/all-targets严格Clippy及最终专项严格检查／fmt、三语构建与两种挂载全导航／浏览器／无API导出检查退出0。证据rule-all-accepted.log、rule-implicit-chain.log、rule-clippy.log、rule-chain-clippy.log、rule-fmt-final.log、rule-docs-build.log、rule-browser.log；磁盘守卫清理后正常复用缓存，没有改动正式Cargo配置或其他在途代码。

本组只关闭直接闭包与对象隐式标志、列明原始字段名和只读Data消费者。DataAware/ValidatorAware的可变setter、逐失败首选语言参数、make:rule、fluent builders与其他规则仍需逐类型处理，不能用只读RuleContext或extend_implicit冒充整个类型已齐。三语说明准确标明当前接口不提供的参数；Validation整章继续，未推送。

第七组暂存核对后统一了普通／隐式两个调用分支的Cow路径，随后真实字段与三语消费者5项、专项严格Clippy和全范围fmt再通过；rule-frozen-{tests,clippy}.log及rule-fmt-frozen.log为最后补验。源码与候选逐字一致，未提高递归上限或添加lint豁免。

## 第八组：规则生成器、隐式模板与生成类型名称冲突

对照固定13.40.0的Foundation/Console/RuleMakeCommand与rule／rule.implicit stub：name、--force/-f、--implicit/-i、Rules命名空间、应用自定义模板均使用原GeneratorCommand与发布／原子写入机制。固定源：https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/RuleMakeCommand.php 。v0.x未找到可复用的make:rule；不复活旧RuleError协议或另造生成系统。

- 新增RuleMakeCommand、两个正式Rust stub及Stub发布接线。默认生成app/rules；嵌套与App前缀遵循既有规则，模块由开发者显式声明。重复无force保留文件并退出0，--force覆盖；目录模块冲突与非法名称拒绝。
- 默认规则返回空失败列表，隐式模板只增加is_implicit=true，不把该标志误当作必填拒绝；业务验证由生成文件的validate实现。普通和隐式分别读取对应的应用已发布stub。
- 新发现：合法类型名Request／FormRequest／Box／Option／Vec／Result与原请求模板导入或标准库类型冲突，实际编译复现15项错误。请求和规则模板改用模块限定及标准库绝对路径，保持授权／规则行为；不禁止合法名称或添加lint豁免。原始反例见request-symbol-collision-before.log。

基于f3c15d1ad独立组合，根CLI8项（request3、rule3、resource2）通过。tests/request_generator_tutorial.py与新增tests/rule_generator_tutorial.py直接提取正式三语创建章节各两条命令，分别使用真实./artisan生成；每类三语生成文件字节一致。原始生成文件不经人工修订复制到隔离测试模块，并校验格式化前后SHA一致：请求6个冲突名称编译、默认403、定制后422/201；规则6个冲突名称编译、默认普通／隐式均通过、定制后普通缺失/空串跳过、nullable null通过、隐式缺失/空串/null拒绝422、合法输入200，共2项实际TCP测试通过。日志rule-generator-tests-v2.log、request-symbol-collision-accepted.log、rule-generated-http.log及两个generator-commands.json保留。

第一次严格检查和文档构建触及32GB停线，均中断、不计通过；守卫持锁清理释放61,940,658,176字节，源码／日志保留。恢复后workspace/all-targets严格Clippy（1m19s）、全范围fmt、三语构建通过，见rule-generator-clippy-resumed.log、rule-generator-fmt.log、rule-generator-docs-resumed.log。早期一次隔离组合混入新Stub切片与旧publisher的array.map，已刷新为同一提交基线后验收；此夹具错误不冒称正式源码缺陷。

make:rule与生成模板消费者本组闭合。逐失败首选语言、DataAware/ValidatorAware可变setter、fluent builders/anyOf/forEach及email扩展模式仍是明确待处理事项，不将本组生成器验收写为Validation整章完成。

第八组三语浏览器检查、根与/docs两种挂载的全部导航，以及不带API导出的参考页检查均退出0，见rule-generator-browser.log与对应三份check日志。没有新增数据库／容器／后台服务；保留同工作流热缓存与证据，临时应用由脚本退出时删除。

## 第九组：规则失败的首选语言、即时翻译和复数

固定PotentiallyTranslatedString::translate／translateChoice在调用时执行翻译，InvokableValidationRule将最终消息交回原验证器。复用现有Translator::string／choose和验证器字段替换器，不新建语言目录、并发全局状态或另一套复数算法。固定来源：https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/PotentiallyTranslatedString.php 。v0.x未提供可复用的逐失败首选语言入口。

- RuleFailure::translate(context,replacements,locale)与translate_choice(context,count,replacements,locale)立即解析消息；None使用当前验证器语言，Some只影响本条消息。RuleContext借用原验证器翻译器；原data借用接口保持。
- Rust显式传RuleContext，替代PHP失败闭包持有的隐式翻译器；避免无上下文的静态查找，也不延迟到整条规则返回后才选择语言。实际同一回调先英语翻译、再切法语翻译、恢复英语，证明各条已解析消息不被后续语言改变。
- 显式翻译后仍由原MessageBag去重和原验证器替换字段占位符，不再受通用规则消息覆盖。翻译参数只处理一次；包含:attribute或:token的替换值不会被再次作为全部翻译参数递归展开。原RuleFailure::new默认行为不变。
- RuleFailure新增translated标志，构造器默认false；RuleContext新增框架内部翻译器借用。仓库及三语示例均使用现有构造器／规则回调，未发现外部结构体字面量消费者；直接构造旧公开结构体的下游源码需改用构造器。HTTP JSON／消息包字段、CLI、默认语言与旧消息行为不改，不能将此结构扩展称为所有Rust源码无差异。

早期内部候选用延迟语言元数据，在最终复核时发现与PHP调用时序不符，已在提交前替换成显式上下文借用的即时翻译。首次新夹具误写不存在的Response::text，编译失败后改为原Response::json；不是框架缺陷，也不计通过。固定PHP真实输出保留validation-failure-translation.json，覆盖中/英/法、指定语言、默认语言、回退、单复数、缺失键、字面量、字段替换、规则消息覆盖优先级、去重与翻译时序。

最终6目标20项通过、0忽略（failure_translation、rule_tutorial、mutation、lifecycle、extensions、options）；新增真实TCP测试并发24次均匹配固定PHP错误数组。三语二十一段正式代码直接提取并编译为3项实际HTTP Kernel消费者，使用应用法语词条得到完整消息；合计23项测试，不将24次请求计为24个功能。最终日志failure-translation-final.log、failure-translation-{doc-extract-final,doc-tests-final,clippy-final,fmt-final,docs-final}.log；workspace/all-targets -D warnings、fmt、三语构建通过。最后一轮原始参数嵌套反例补强后已重新执行上述最终门禁。

本组关闭指定语言／复数及显式翻译时序子范围。DataAware／ValidatorAware可变setter、Rule fluent builders／anyOf／forEach及email扩展模式仍按原清单接续，不提前关闭Validation整章。不包含其他协作者的队列／云接线在途代码，没有新建持久化服务或容器。

第九组最终三语浏览器、两种挂载全部导航与无API导出参考页检查全部退出0，failure-translation-gates.log的六个阶段均为0。新借用与原回调共同通过workspace严格门禁，未添加命名或其他lint豁免；证据与热缓存保留，临时PHP对照容器使用--rm退出，无项目数据库或卷变更。

## 第十组：AnyOf备选组与原工厂／文件／数据库消费者

固定Rules/AnyOf按顺序为每组调用Facade Validator::make，传入父验证器的消息和字段名，一组通过即短路；关联规则与关联值使用字段映射，其余值包装为索引0。固定来源：https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation/Rules/AnyOf.php 。v0.x未找到对应可复用类型。复用原Factory、RuleSet、ValidationRule、消息包与presence verifier，不新建子验证器实现或手工判断规则成功。

- 新增Rule::any_of、rules::AnyOf::new、AnyOfRules::{Value,Fields}及两种From转换；.into()进入原自定义规则管线。Rust显式Vec<Rule>／RuleSet替代PHP动态数组。逐组保持声明顺序，支持混合与递归组、空备选拒绝、单个空组通过，missing／nullable仍由原外层调度决定。
- RuleContext仅借用父消息／字段名及原始转义属性路径；每组从当前应用门面工厂创建验证器，保留已注册扩展、replacer、数据库验证与实例配置隔离。与固定实现一致，依赖活动应用门面作用域，不在无作用域时暗建另一套工厂。
- Data私有scope只重定向当前值和上传路径，保留原上传句柄及shape，不复制文件内容。外层值不会被子验证器的过滤结果替换；额外字段是否允许仍由外层数组键规则决定。无吞掉子执行错误的分支，没有声称基准性能收益。

固定PHP实际执行17种标量／关联／数字键／通配符／空组／missing／null／空串组合，并分别有／无自定义消息，34组输出落validation-any-of.json。初版候选误以为AnyOf外层自定义消息不覆盖，实际对照反例证明会覆盖，已改回原消息键管线并通过34组逐项实际HTTP回放。PHP对照补装Egulias4.0.4及锁定依赖在target/tmp下；主机php7.4缺tidy库启动失败，未改主机环境，改用已固定PHP8.5.12临时容器完成；对照容器退出即删除。

实际TCP还覆盖混合／递归组、工厂扩展和replacer观察到继承消息及字段名、成功组短路不执行后续回调、外层数据不被子输入0污染。multipart验证普通文件、嵌套文件、括号内字面点字段，成功后返回真实文件句柄检查，3KB文件触发2KB规则并逐字段422。首次夹具用顶层doc.name被既有归一化成下划线，required反例及时暴露未验证字段；修正为container[doc.name]，没有修改正式请求归一化。另一次测试配置误用捕获闭包，原ConfigLoader是函数指针；已按现有基路径／环境加载方式修正夹具，不扩大框架ConfigLoader范围。

最终MySQL8.4.13组合4目标28项通过、0忽略：AnyOf2、既有validation_http23、failure_translation1、rule_tutorial2。独立AnyOf数据库用例验证存在／不存在／后备组，并配置故意不可达read端口，证明沿用write presence连接。记录any-of-mysql-frozen.log；验收独占容器已清除，测试表正常DROP，无数据卷操作。三语24段正式代码（含27个函数入口）直接提取成3组HTTP Kernel消费者，覆盖两类新教程，合计本组31项，不把34组数据或multipart请求重复算功能数。

最终any-of-gates.log六阶段全0：三语提取与运行、workspace/all-targets -D warnings（45.45秒）、fmt、三语构建、浏览器和根／docs挂载全部导航、无API导出参考页检查通过。实现与验收快照逐字核对，保留日志／源码／热缓存；未混入队列／云功能等在途改动。

当前明确余项：ForEach的编译规则与动态字段展开、其余Rule fluent builders、DataAware／ValidatorAware可变setter、email DNS／spoof／strict／filter扩展模式。AnyOf已闭合，不再列为未实现；Validation整章仍不能宣布完整对齐。

## 第十一组：ForEach编译、动态字段与关联路径修复（2026-11-24）

本轮重新核对固定文档全文；下载内容与已有reference/validation.md及urls.md的SHA一致，固定文档分别为210db605c0c0db85e7ebbe8c4299e629fc7320dd6ebf9c3f1c69afd6468cfbfd及3f0fbaf1241207927c794b189ff3c0209e71be67443f4d28a84436a702753e63。新增section-map.json逐节记录14个URL小节与179个Validation小节；未复核的小节保持开放，不把锚点数量当作完成率。旧版v0.x未找到可复用的ForEach／DataAware对应机制，沿用当前RuleSet／Factory／Validator装配。

### 契约与生命周期

- Rule::for_each返回NestedRules，回调接收值与属性；for_each_with与NestedRules::new提供四参数版本。NestedRuleSet::{Value,Fields}分别表达同一值规则与相对字段规则。Rule::compile、contracts::validation::CompilableRules、Rule::compilable实际参与编译；不是在普通Custom规则中另跑验证来冒充动态字段展开。
- 原Factory创建时编译，get_rules与重复validate不重复调用回调；set_data／set_rules同步重编译。编译后的规则保留原通配来源、Precognition筛选、隐式数据库列名推导及投影字段；空规则列表仍保留该属性，空匹配不调用回调。现有规则／展开预算与深度限制继续执行，展开条目沿用4096预算而非错误套用1024声明限制。
- 保持既有不返回Result的set_data／set_rules签名：编译失败缓存原错误类别，后续get_rules／验证持续返回错误，不使用旧编译结果悄悄通过；修正后恢复。Factory::make及可失败的add_rules立即返回错误。不可复制的执行原因用Arc保留原Error::source链，未把原因截成字符串。此为明确Rust错误表达适配，不声称setter同步抛异常完全一样。
- Rust闭包不能省略参数，常用两参数与完整版本用不同构造方法表达。原始路径使用稳定转义，不泄漏PHP内部随机占位符；完整上下文NestedRuleContext、for_each_context／NestedRules::with_context及CompilableRules::compile_context提供原Data与typed file，普通四参数仍保留固定上游的扁平数据和父项语义。空的类型化flat映射／validated映射维持既有JSON对象表示，PHP空数组歧义在测试中仅对对应容器规范化，未改变用户数组值。

### 关联修复及证据

- 固定NestedRules、ValidationRuleParser、ValidationData、Rule::compile与Validator::sometimes源码逐项对照；固定PHP8.5.12真实执行11组ForEach用例，保存validation-for-each.json，包含创建时调用次数、扁平数据／父项、显式点路径、递归组、规则顺序、消息与投影。另4组sometimes上下文保存validation-sometimes-contexts.json。
- sometimes在条件判断前编译动态规则；末尾.*条件接收当前项，其余路径接收对应父项／字段，修复此前一律父数组／根数据的偏差。动态追加在set_data时按既有契约清除。
- 字面点路径复验发现用普通Arr::set/forget会混淆层级与字面点，已复用验证器path解析增加规范写入／删除；父路径也按片段解析，不用rsplit切断转义点。文件替换只清理文件元数据后写值，保留数组位置，不让旧shape重新覆盖新标量。原上传句柄不复制内容；实际multipart替换后JSON值42、文件引用消失已验证。
- 实际HTTP回放11组固定数据，增加编译失败500后正常请求200、Precognition只验指定展开字段204／422、typed file与递归Trait文件编译、字面点字段、1201条合法展开、运行期追加及错误恢复；独立MySQL8.4.13以故意失效read地址验证推导id列与原write presence连接。

最终隔离来源0eeb12e3a加本批源码：5目标36项、0忽略（for_each4、validation_http23、mutation6、lifecycle1、rule_tutorial2）通过。三语正式示例直接提取为3组HTTP Kernel消费者，覆盖两参数、父项bulk条件和相对字段过滤，通过；日志foreach/mysql-context-final.log、foreach-doc-{extract,tests}-final.log。首次新夹具误用Middleware命名空间／缺Ok包装已纠正，不算框架缺陷。普通回归只需要JSON数据，运行不依赖PHP。

两次触及32GB停线，守卫清理并恢复；一次中断收尾报EPERM，本批构建已退出，项目构建守卫完成全局持锁清理后空间恢复，未绕过锁清理或计中断为通过。最终foreach/gates-resumed.log的Clippy、fmt、三语构建、浏览器四阶段均0；workspace/all-targets -D warnings完成1m20s，先前最终三语消费者两阶段也0。新完整上下文与主测试源码已核SHA；临时容器均由验收器回收，日志／源码保留、缓存继续复用。

本批关闭ForEach及上述关联差异。全文复查另外确认safe子集／合并／集合／迭代入口、Enum/In对象及密码默认／泄漏验证等缺口，连同其余fluent builder、DataAware／ValidatorAware、email模式继续本轮处理；不以原关键词清单或本批36项宣布整章完成。

## 第十二组：ValidatedInput 筛选合并集合与类型化文件（2026-11-24）

固定文档 working-with-validated-input 及 Support/ValidatedInput、Contracts/Support/ValidatedData、InteractsWithData 逐入口核对；旧实现没有正式 support 导入及 only/except/merge/collect，已先用实际 Cargo 负消费者复现（safe/before-cargo.log），非仅搜索判断。沿用原验证投影、Data 路径预算、Collection 数字键合并和上传句柄，不再新建输入体系。

公开 support::ValidatedInput 与原 validation 路径兼容；ValidatedData/Arrayable/ArrayAccess trait 用正式泛型消费者验证，增加 only、except、merge、collect、to_array、键值迭代、索引及 offset 操作。ValidatedFormRequest::safe 借用原已验证结果；Validator::safe 仍要求成功验证。构造器本身不运行验证。筛选返回类型化结果以保留文件句柄，集合使用 RequestValue；JSON 投影沿用文件 null，不输出本地路径。Rust 没有 PHP 动态属性，使用原生索引与显式 offset_set/offset_unset；缺失索引为 null，路径使用既有转义机制。only 接收具体路径，不把通配投影冒充上游数据访问的任意表达式；本簇不宣称 InteractsWithData 全部通用入口已闭合。

固定 PHP8.5.12 运行5组数据保存 validated-input.json，回放筛选／嵌套／合并／数字重排／集合／迭代／null 读取。实际 TCP 另覆盖类型化 FormRequest 成功200与失败422、未验证 admin 不进入数据、multipart 文件字节、独立子集与文件合并、替换后清元数据、原结果不变。关联修复删除最后数字键后的追加状态，复用 Collection 的私有 append state，不因重建结果错误覆盖旧键；删除全部后继续原编号、验证投影后追加都通过。

最终 safe/final-gates.log 三阶段全0：新测试2、mutation6、正式三语提取HTTP消费者3，合计11项0忽略；先前 ForEach4 与规则对象2 回归也通过。safe/gates.log 的真实 main 构建与 artisan help make:request 退出0；三语构建、浏览器、根与docs挂载、API参考检查均0。最后编号修复后重新执行上述11项、workspace/all-targets严格Clippy（20.82秒）与fmt，全通过；未用先前源码的静态证据替代最终修复。未改命令行为、错误响应格式或默认值。日志／原始PHP／负消费者保留于target/tmp/docs-chapters-20261124/safe及safe-*，无本批常驻服务或容器；热缓存继续由守卫管理。

本簇关闭教程已验证数据操作差异；接续 Enum/In/NotIn、剩余fluent builder、状态感知setter、email和密码策略，全章仍保持开放。

## 第十三组：Enum、In、NotIn 真实规则对象（2026-11-24）

固定规则对象源码及文档 rule-enum、rule-in、rule-not-in 对照确认原 Enum/In 只有空壳。现 Rule::r#enum::<E>() / rules::Enum<E> 复用应用已有 BackedEnum 和弱标量输入转换；only/except 接收原生枚举值，非空 only 优先，when/unless/when_else 配置同一实例，不另跑独立验证器。执行使用原 ValidationRule/RuleContext，沿用当前工厂译器及消息。显式双分支方法对应 PHP 可选第三参数；Rust JSON 载荷仍为后备标量，实例直接判断用 passes_case，不复制 PHP 反射／对象载荷。既有公开方法和格式保持。

Rule::r#in / rules::In、Rule::not_in / rules::NotIn 使用 Vec 保留成员界限，包含逗号／引号，不拼接未转义字符串。构造接受 Into<Value>，数字浮点／布尔／空值复用既有标量转换。枚举列表由应用显式映射后备值，避免假称语言支持运行期反射所有成员。r# 为 Rust 关键字语法，混合规则列表通过 Into<Rule>，不是新的序列化名称。

固定 PHP8.5.12 实际执行140组字符串／整数枚举、弱转换、null/数组、only/except 优先及条件分支，输出保存 tests/reference/validation-enum.json；弃用提示单独写stderr原样保留，不混入JSON或关闭诊断。真实 TCP 每组比较200/422与值／错误，再验证列表成员、逗号／引号、数组逐项限制和拒绝项。新增专项1项（含上述140数据组合）＋ForEach4＋mutation6共11项0忽略，三语正式示例直接提取的3项HTTP Kernel消费者通过。safe2前置回归亦通过，不将数据条目当作功能数。

最终 enum/gates.log 提取、消费者、workspace/all-targets严格Clippy（45.96秒）、fmt、三语构建与浏览器全0。当前候选实际main构建通过，APP_BASE_PATH指向隔离源码执行 artisan help make:rule 退出0。源码与验收快照核SHA；本批无常驻服务／数据库／容器。日志及固定源码留在target/tmp/docs-chapters-20261124/enum和enum-*，编译缓存由既有守卫复用。

本簇关闭上述三条教程规则对象缺口。剩余 fluent 仍在处理中；其中 url/uuid 参数过滤与 numeric:strict 已确认不能仅靠旧解析器名称视为完成，将在下簇同时修复。状态感知规则、email与密码策略仍开放，全章未关闭。

## 第十四组：链式构建器、参数化规则与条件编译（2026-11-24）

固定 Rules/{StringRule,Numeric,ArrayRule,Date,Url,Uuid,IpAddress,Distinct,Timezone}、Rule 工厂、ParameterizedRule、ConditionalRules 与布尔回调对象逐类型核对；复用既有 scalar/date/dimension/presence 管线及第十一组编译器，不在 builder 内另跑一套校验。旧版相关可复用部分已在前置组核对，缺少本批公开类型与条件对象，沿用当前正式实现补齐。

StringRule、Numeric、ArrayRule、Date 通过 CompilableRules 展开到同一字段规则，保留消息和投影。RuleBuilder trait 允许 Rule::string().min(...).max(...) 及数组链，并保留已发布 Rule::min/Rule::max 静态构造器：Rust 不允许同名固有实例方法；需显式导入 trait。提供直接类型化构造和 Into<Rule>，不把字符串替代写法计作类型已实现。数值小数边界显式 BigDecimal，避免不可失败隐式转换浮点；Date 参数使用既有 support::Date 或字符串，按添加约束时的格式转换，原应用时钟／时区不变。

ParameterizedRule trait 提供真实参数向量；Url/Uuid/IpAddress/Distinct/Timezone 与自定义 Trait 消费者进入同一解析器，保持成员边界。发现原 numeric:strict、url:协议和uuid:版本实际落入扩展分支，已接到内置规则，字符串／Trait／builder 三入口均验收；严格布尔／整数新构造保留原零参数入口。UUID nil/max及指定版本、URL协议、IP无效版本错误、区域时区均有实际消费者。Numeric::digits/digits_between/exactly 按固定源码添加 integer，不能仅附加位数／size；约束去重并保留首次顺序。String/Array 字符串 builder 的逗号分隔语义按固定源码保留，ParameterizedRule 单独保护向量边界，不擅改既有规则格式。

Rule::when/unless 与 ConditionalRules 在通配展开前求值；RequiredIf/RequiredUnless/ExcludeIf/ExcludeUnless/ProhibitedIf/ProhibitedUnless 各有真实类型和 Into<Rule>，回调在每个展开属性编译时运行。RuleCondition::lazy 接收零参数闭包，callback 显式接收 typed Data；稳定Rust不能以PHP动态可选参数表达重载，分支和列表显式传入。缺省分支空规则仍保留字段声明，用原 bind 处理空规则，而非错误返回500。重复验证不重算条件，set_data/set_rules 按原编译生命周期重算。

固定PHP8.5.12 实际130组 builder 数据、50组条件及生命周期保存为 validation-fluent.json 和 validation-fluent-conditions.json。附加 portable-ascii 2.0.3 原源码／许可仅放target/tmp作实际PHP依赖，Ramsey复用原4.7.6；未将PHP或新依赖引入运行产物。JSON对照启用PRESERVE_ZERO_FRACTION，避免把严格整数反例2.0错误重放成2；诊断独立stderr保留。HTTP默认空串转null按正式全局中间件验收，原工厂对照仍比较未转换的精确值，未弱化断言。

最终 fluent/gates.log 六阶段全0：6目标42项0忽略（fluent3、enum1、for_each4、scalars5、mutation6、http23）；三语正式示例直接提取并运行3项HTTP Kernel消费者，覆盖两种构造、协议／版本／严格模式及条件分支。workspace/all-targets严格Clippy47.61秒、fmt、三语构建与浏览器三项均通过。额外 MySQL8.4.13 真实验收3目标30项0忽略（与上述有重叠，不累加成独立功能数），write presence 与动态展开仍通过。实际main构建、artisan help make:rule、artisan inspire 全部退出0；fluent/consumers.log记录四阶段成功。验收容器已自动清理，测试服务已退出，源码与日志／热缓存保留，项目磁盘守卫保持启用。

最后移除静态无参数分支中的冗余参数判断，fluent/final.log 的专项3项、workspace严格Clippy（21.20秒）、fmt再次全部通过；与前述正式教程内容及可观察行为一致。

本批关闭已列构建器及其参数分发／条件求值差异。DataAware/ValidatorAware、email扩展模式、密码默认与泄漏策略仍开放；全文复核还发现数组错误的 :index/:position/:ordinal-position 及多层占位符缺口，已登记接续，不把本批或原关键词清单当作全章闭合。其余 Rule 工厂与规则类型仍按逐节清单对账。

## 第十五组：DataAware／ValidatorAware setter 与同一验证器借用（2026-11-24）

固定 Contracts/Validation/DataAwareRule、ValidatorAwareRule、InvokableValidationRule 与 Validator::validateUsingCustomRule 核对：本批对应教程的新 ValidationRule 包装器，真实顺序为 setData→setValidator→validate，不能误用旧 Rule 包装器的调用顺序。旧版没有可直接复用的同型可变 setter；复用当前 Request/context.rs 的任务作用域借用和取消恢复机制，不全局缓存验证器、不另建并行验证器。

新增独立 contracts traits、Rule::data_aware／validator_aware／aware 与 ValidatorHandle。稳定Rust无法动态检查可选trait，显式适配器选择 setter；普通 Rule::custom 不加状态锁。setter 使用可变借用与 ValidationResult<()>，适配器只对同一有状态规则实例串行执行 setter／正文。Data 直接借用原类型化数据，用户可只提取需要的字段；Validator 内部数据／规则使用Arc只读快照，实际写入保持原行为，未作未测量的性能收益声明。

ValidatorHandle::with 与 with_async 读写调用方同一个 Validator，不能以只读副本冒充 setValidator。任务token、弱句柄和互斥借用防止跨任务／结束后访问；冲突借用和递归验证明确返回错误。RAII在取消时恢复原实例并释放规则锁，不回滚已经执行的业务写入。规则正文拿到原先捕获的值，后续规则读取更新后的值。RuleContext 为setter后的不可变快照；file() 保留原始值对应的上传对象，setter替换文件后仍可读取原字节，实时数据元信息则已清除。规范转义路径留在内部，规则参数仍为可显示属性名。

实际联动修复：运行期间修改消息不清掉此前错误；手工错误包参与 bail／presence 跳过，清除错误后恢复presence执行，避免旧失败标记粘滞。修改规则后 nullable、numeric、sometimes、bail及隐式停止逻辑读取当前规则集合；原局部迭代集合保持固定上游的写时复制语义。setter引入编译失败保留原错误类别和已有错误，修正后恢复；执行结束／取消后恢复运行状态。

固定PHP8.5.12实际29组记录保存 validation-aware.json：setter顺序与原始数据、缺失／空值／nullable／implicit调度、原值与更新后的数据、错误消息及手工错误、通配字段后续数据、bail与规则替换时序。空PHP数组到类型化对象仅在对应数据／错误容器规范化，未将普通数组统一改写。真实TCP覆盖成功、422、setter失败500后下一请求200；两个应用共享同一规则对象的32请求并发隔离；子任务与作用域外句柄拒绝；异步借用／冲突／重入、取消恢复；DataAware单独／ValidatorAware单独／组合入口；真实multipart及嵌套字面点文件替换。最初根表单点号按既有PHP输入规则转为下划线，夹具改为保留字面点的括号字段，不改输入解析行为。

最终按ba6d7b32018d32badfc67a1af8f088fedea9db06已提交HEAD加本簇重组隔离源码，不依赖其他在途改动。MySQL8.4.13八目标47项0忽略通过（safe2、aware6、failure_translation1、fluent3、for_each4、http23、mutation6、rule_tutorial2），正式三语示例直接提取3项HTTP Kernel消费者通过。日志 aware/gates-head.log 前三阶段0；Clippy曾触及32GB，守卫安全停止并持锁释放62,857,183,232字节，未把中断计通过。恢复后 aware/head-resumed.log 七阶段全0：workspace/all-targets严格Clippy（1m55s）、fmt、三语构建／浏览器、实际main构建（1m37s）、artisan help make:rule及inspire。更早一次守卫因并行构建尚存未清理，待其退出后正式守卫完成清理并恢复，源码／日志未动。临时MySQL已回收，无常驻本批服务。

验收期间00bf786d6提交了并行测试缓存隔离，差异已只读核对（Testing与Blade缓存目录装配），未夹带其在途修改，也未冒称重验其专属功能。当前本簇入口全部按上述隔离组合核SHA；其余已提交内容保持原样。

最后将三语既有缓存失效说明限定为验证执行之外的修改，避免与运行期保留当前错误包矛盾；仅文字调整，三语重建15.50秒与对应HTML内容断言通过（aware/docs-final-wording.log）。

本簇关闭状态感知setter及以上关联差异。完整章节继续：其余基础Rule工厂、Confirmed.custom_field／CurrentPassword.guard、数据库where闭包入口与数组条件、email模式、密码策略和数组消息索引占位符。rule-builders总节改为部分已验，避免只因新增若干类型就关闭全节；URL与Validation整章仍不在本批宣布完成。

## 第十六组：命名工厂、确认字段与认证 guard（2026-11-24）

逐项核对固定 Rule 工厂及 Rules/{AcceptedIf,DeclinedIf,MissingIf,MissingUnless,PresentIf,PresentUnless,ExcludeWith,ExcludeWithout,InArray,InArrayKeys,MissingWith,MissingWithAll,PresentWith,PresentWithAll,Prohibits,RequiredArrayKeys,RequiredWith,RequiredWithAll,RequiredWithout,RequiredWithoutAll,ProhibitedIfAccepted,ProhibitedIfDeclined,RequiredIfAccepted,RequiredIfDeclined,Regex,NotRegex,Confirmed,CurrentPassword,ArrayKeys,Contains,DoesntContain,ImageFile}。旧版可复用内容已在前置组核定；本批补的是类型／工厂入口，继续复用原解析器、编译器、文件及认证管线，不另跑校验实现。基础 accepted/declined/exclude/filled/missing/prohibited/hex_color/json/list/mac_address/ulid/active_url 也接通专用构造。

ParameterizedRule 保留比较值成员边界；AcceptedIf 的非字符串值按 JSON 编码，另五类按固定源码的 null/布尔文字与标量转换，不能统一强转。ComparisonValues 接受标量、数组及集合；字段列表显式数组／迭代器对应 PHP 可变参数，Rust 无可变参数方法。ArrayKeys 保留原始逗号分割，Contains/DoesntContain 按原源码转义成员；Regex 原样保留模式。ImageFile 是实际公开类型，复用 File 的上传大小、扩展名、编码与尺寸处理，默认拒绝SVG。

Confirmed.custom_field 与 CurrentPassword.guard 同时提供类型入口及 RuleBuilder 链式入口；链式调用替换当前确认字段／guard，不能叠加旧默认约束。保留已有 Rule::confirmed() 与 current_password(Option<&str>)，Rust同名固有方法不能重载，以明确类型／trait表达。

固定 PHP8.5.12 实际生成101组行为和78组参数／声明对照，保存 tests/reference/validation-factories.json；无上游脚本进入正式源码。真实TCP回放成功／422及投影／错误键，实际multipart覆盖File、ImageFile默认SVG拒绝与允许SVG。真实认证guard／bcrypt证明显式web覆盖默认admin，错误密码及无认证admin拒绝；真实UDP覆盖A/AAAA、超时、失败恢复和实例隔离的active_url构造。三语正式教程直接提取编译运行，验证成功、确认错误、条件要求、数组键与排除投影。

隔离源码沿用已验收ba6d7b32018d32badfc67a1af8f088fedea9db06加第十五／十六组，未带入活跃其他修改。factories/gates.log 九阶段全0：MySQL8.4.13六目标37项0忽略（含三语3项）、workspace/all-targets严格Clippy1m20s、fmt、三语构建16.11s及浏览器、实际main构建7.75s与Artisan帮助／inspire退出0。首次三语代码混合规则数组缺少Regex.into()已由真实编译发现并修正，失败不计通过。核心／测试／教程与验收快照逐项SHA一致，证据 target/tmp/docs-chapters-20261124/factories/；本批临时MySQL自动回收，无常驻服务。

上述命名入口闭合；rule-builders总节仍保留部分已验，直到数据库where、email及密码策略完成。数组消息位置与其余全章复核继续，不宣称全章完成。

## 第十七组：数据库规则 where 的回调与数组入口（2026-11-24）

固定 Rules/DatabaseRule::where/whereNot 与 ValidatesAttributes::validateExists/validateUnique、DatabasePresenceVerifier::addConditions 逐项核对。此前仅 using 可传回调，where只接标量；本批让同一个 r#where 接收 DatabaseWhere::callback，仍复用原 Conditions／嵌套条件／write连接，不另造查询器。第二参数复用查询层 BindingValues，单值保持旧调用，数组／空数组分别进入 where_in／where_not_in。Null及布尔类型继续绑定，条件不会拼SQL。

Rust不能重载方法参数数量，也不能为普通方法省略参数；回调／空值条件显式传 Value::Null。DatabaseWhere 用专用回调构造区分字符串字段，避免稳定Rust中Fn泛型与外部字符串类型的重叠实现。该占位值不增加SQL条件。using、where_null、where_not_null、软删除及原标量入口保留。

固定PHP8.5.12＋intl实际执行SQLite内存表50组，覆盖标量／数组／空数组／null／false／not数组、嵌套OR再AND软删除，以及Unique数组／回调，正式JSON在tests/reference/validation-database-where.json。Rust在真实MySQL8.4.13的实际TCP全部回放，比较成功投影及422错误键；已有read连接刻意不可达，继续证明使用write连接。三语directory_rules直接提取编译，在实际MySQL与HTTP Kernel分别验收两种租户成功、已删除／不存在拒绝、重复邮箱拒绝，未用手写平行规则代替文档。

最终 database-where/gates-resumed.log 九阶段全0：六目标44项0忽略（文档6、aware6、factories2、fluent3、for_each4、http23）；workspace/all-targets严格Clippy、fmt、三语构建／浏览器、实际main与artisan help make:rule／inspire全部成功。更早TCP专项1项已过；文档夹具曾因捕获式配置闭包与fn指针契约不符失败，改从独立应用基目录读取前缀／验收环境读取URL后完整重跑，不计失败为通过。临时MySQL自动回收，无本批存活服务；源文件SHA与隔离组合一致。证据target/tmp/docs-chapters-20261124/database-where/。

数据库where教程缺口关闭；email、密码、数组消息位置与全章复核继续，整章仍开放。

## 第十八组：数组消息索引、位置、序数与输入（2026-11-24）

固定 FormatsMessages::makeReplacements／replaceIndexOrPositionPlaceholder／replaceInputPlaceholder 核对。先以真实反例证明 :index/:position/:ordinal-position 与 :input 原样泄漏，再补同一消息格式化管线；不在HTTP渲染层改字符串。数字路径按原规范转义段读取，支持first至tenth及other（第十一数字段），缺层保留、大小写不敏感；复用既有数值语法与php_integer_cast，不只支持非负整数数组下标。输入仅标量／null替换，保留值标签，数组保持原占位符。属性→输入→位置→规则参数的替换顺序与固定源码一致；翻译键先读取原文，再走同一格式化，避免译文与内联消息行为分裂。

关联反例：带字面点的字段原先错误命中浅层通配消息。内联通配符改为只消费同层，不将规范转义路径拆段；翻译模式回退也检查实际层级。保留旧精确字段消息优先，原BTreeMap原生类型和各类错误包契约不变。序数复用现有Number，按固定Intl默认英语；负数序数使用Intl的减号，i64上界加一依固定PHP转浮点，复用已有浮点／数字格式化，不做未测量性能声明。

为真实检查序数和后续email spoof，仅在target/tmp构建php:8.5.12-cli-alpine加intl的本地参考镜像，未引入PHP运行依赖。固定参考11组含多层、十二层／other、缺层、前导零、负数、科学计数法、非ASCII数字、字面点、数组／null／bool、数值上界；保存tests/reference/validation-message-positions.json。原行为baseline失败日志before.log保留，修复后全部通过。真实TCP逐组精确比较错误消息，显式保留空串配置与默认空串转null分别验收；默认响应和单独执行固定PHP得到的null数据组一致，不改预期凑通过。

三语store_photos直接提取并在HTTP Kernel执行，展示本地化文案中的位置、序数和输入值，不只检查编译。message-positions/gates.log九阶段全0：MySQL8.4.13八目标51项0忽略（文档6、aware6、failure_translation1、fluent3、for_each4、http23、positions2、mutation6）；workspace/all-targets严格Clippy24.81s、fmt、三语构建15.67s／浏览器、实际main15.30s与两条Artisan退出0。源／测试／教程与隔离组合SHA一致，证据target/tmp/docs-chapters-20261124/message-positions/。临时MySQL已回收，HTTP服务已退出；没有改写历史证据。

本批关闭数组消息教程缺口及以上关联问题；email、密码策略与逐节全章对账继续，整章未关闭。

## 第十九组：email扩展模式、链式入口与DNS测试门面（2026-11-24）

固定 Rules/Email、ValidatesAttributes::validateEmail、Factory::fakeDnsLookups、Facade、Egulias4.0.4 的RFC／NoRFCWarnings／DNS／Spoof／MultipleValidationWithAnd 及 FilterEmailValidation 逐入口核对。复用原RFC解析过程记录警告，严格模式不另用简化正则猜测；原生过滤复用现有PCRE2。Unicode防伪使用既有intl0.6.5的脚本扩展／增广脚本和UTS39高度限制组合，不以“包含非ASCII”或简单混脚本一概拒绝。开启同一依赖的confusables与idna特性，无新增依赖版本。

Rule::email原返回Rule保持，RuleBuilder提供rfc_compliant(bool)、validate_mx_record、prevent_spoofing、with_native_validation(bool)、rules；真实Email类型另有strict／when／unless／when_else与附加规则。typed builder模式按固定顺序，字符串规则保留原参数元信息并复用CSV解析，未知模式依固定源码退回RFC。组合模式全部执行，不能在RFC失败后提前跳过DNS等模式；CR/LF仍在进入模式前拒绝。DNS或spoof单独使用不暗中加RFC，正文明确建议组合。

Email附加规则通过原Factory的唯一make_shared装配链创建嵌套验证器，执行原resolver和当前扩展注册，与上游内部工厂机制对应；共享原Arc<Data>保留上传／类型数据，消息、属性、扩展、认证及DNS上下文沿用当前装配。普通邮箱不产生嵌套验证器。DataAware/ValidatorAware消费者实际拿到子验证器，原值／其他字段可见；手工错误经子验证器all消息归到父邮箱字段，外层bail不错误截断内部规则。没有新建平行校验器或unsafe全局状态。

DNS共享原active_url的Hickory传输与解析器配置，绕过hosts／localhost合成；MX/A查询通过后才查询AAAA，AAAA单独失败不抹掉A/MX成功，空MX拒收、保留域、父域至完整域名的顺序与地址记录回退依固定源码。每种地址／MX查询复用同一池，A和MX内部并发，不作未测量吞吐声明。新共享实现也修复active_url根点结尾处理，实际UDP已验。Factory／Facade fake_dns_lookups补齐；应用共享AtomicBool让已有验证器可观察切换，实例显式覆盖，应用间隔离，避免将PHP进程静态状态直接带入多应用异步进程。切换实例策略／解析器会失效旧验证结果。Rust布尔参数需显式提供。

本批实际构建PHP8.5.12＋原生Intl78.1参考环境：IDNA_DEFAULT实际为48（非过渡），与早期无Intl/polyfill证据不同。原api-f01历史文件完全保留，新validation-email-rfc-native.json记录原输入SHA、9381个原输入的实际通过值；其中24项Unicode边界改变。默认仍RFC、仍接受仅警告地址。RFC标签仅提升LABEL_TOO_LONG，不能把前导组合标记等其他诊断误报为长度。DNS默认未启用ContextJ/Bidi，复用成熟映射／校验／Punycode处理对应选项，不用更严格URL域名策略误拒；真实全量差分确认。

实际80,316项模式／数据组合（8924值×9模式）0差异，附加63项CSV引号、重复及空白模式固定结果；正式1944项样本由真实TCP重放，类型／链式样本分别调用公开入口。HTTP策略选择在测试中以JSON串传入，避免HTTP合法OWS裁剪损坏“模式前后空白”的输入；未改请求头协议。真实UDP验证MX/A/AAAA、null MX、SERVFAIL、AAAA失败、超时、父域回退、失败恢复；核对RFC失败后仍查询DNS、头部换行提前拒绝。两应用12组并发配对、共24请求及Facade开关／已有验证器／实例覆盖均正常。

原生过滤表达式来源PHP8.5.12 ext/filter/logical_filters.c，保留PHP许可及Michael Rushton原版权在framework/licenses/email-native-filter.txt，并完整附入THIRD_PARTY_NOTICES.md供运行分发；必要来源不作品牌清理。参考：[固定源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Validation)、[Egulias4.0.4](https://github.com/egulias/EmailValidator/tree/4.0.4/src/Validation)、[原生过滤源码](https://github.com/php/php-src/blob/php-8.5.12/ext/filter/logical_filters.c)、[复用的Unicode库](https://github.com/KarpelesLab/intlrs)。这些为必要来源和许可，不改公开协议或项目身份。

最终email-modes/gates-factory.log九阶段全0：MySQL8.4.13十二目标65项0忽略（含三语直接提取6项、80,316差分及真实TCP）；workspace/all-targets严格Clippy41.02s、fmt、三语构建16.12s／浏览器、实际main与artisan help make:rule／inspire成功。此前两处显式解引用Clippy错误、测试HTTP响应类型及OWS夹具失败均保留日志，已修复重验，不计失败为通过。触及32GB时守卫持锁释放68,243,480,576字节，保留源码／日志／依赖与锁，恢复后继续。证据target/tmp/docs-chapters-20261124/email-modes/，源／测试／三语与验收快照SHA一致，临时MySQL及HTTP/UDP服务已回收。

提交前复核发现旧嵌套辅助器只复制父验证器状态，绕过自定义resolver且错误继承父级手工值标签。已把Trait make和嵌套入口集中到原Factory装配链；工厂克隆共享注册状态，验证器仍保存创建时扩展快照。真实回归核对resolver在父／子分别执行、父级手工值标签不污染子规则、构造父验证器后注册的扩展在新子验证器执行；现有validation_extensions七项同时复验。最后只澄清Factory克隆说明注释，另做格式检查，不把注释变动当作新增功能验收。

剩余逐节审查已明确登记语言目录custom／attributes通配查找、尺寸消息类型键、通配字段显示名与嵌套标签的关联差异；尚待固定源码实际消费者复验和修复，随密码之后的消息功能簇处理，不能据已有翻译测试关闭这些条目。

本批闭合email教程模式及以上关联入口；不是整个Email类所有未入教程方法的完成声明。密码默认／泄漏策略／附加规则、全章逐节最终对账仍继续，不将关键词列表或测试数量当作整章完成。

## 第二十组：尺寸计算与声明规则的消息类型（2026-11-24）

固定ValidatesAttributes::getSize、FormatsMessages::getAttributeType及min/max/size/between逐项核对。密码内部复用原Factory前发现通用尺寸计算的关联差异：输入数组不等于声明array，失败numeric规则仍决定尺寸消息类别，bool/null须按标量文本长度，浮点须沿用既有PHP14位文本转换。先由实际固定参考378组复现99组差异，修复后全部一致；新增6组文件参考验证numeric、array/list优先于file的消息分类。金标准validation-sizes.json保留原输入类型、验证投影和实际HTTP序列化投影，不把1.0被默认JSON格式化为1误当成验证器改变输入类型。

复用原RuleFlags一次计算numeric与array/list标记，执行器和相邻gt/lt/gte/lte共用分类；array:keys属于array，独立array_keys不暗中变更类型。标量与相邻尺寸比较共用size/validation_size，不新增手写密码尺寸分支；保留原科学计数法指数限制、十进制比较及原错误包。文件仍按KB，消息类型按声明规则而非运行值选择。没有添加依赖或更改公开签名。

真实TCP回放378组JSON及6组multipart，精确核对422消息与成功投影；三语说明同步。sizes/gates.log九阶段全0：MySQL8.4.13十四目标70项0忽略（含原密码、email完整差分及三语6项），workspace/all-targets严格Clippy、fmt、三语构建／浏览器、实际main及artisan help make:rule／inspire通过。证据target/tmp/docs-chapters-20261124/sizes/，SHA与冻结快照一致；实际MySQL与HTTP服务均回收。初次HTTP夹具用保留小数零的参考投影比较默认JSON输出而失败，已补固定PHP实际wire投影并重跑，不改生产序列化格式。

本批只关闭尺寸及消息类型关联差异；语言目录通配／嵌套标签、密码默认与泄漏策略、全章逐节复核继续，整章未关闭。

## 第二十一组：语言目录custom／attributes及嵌套规则标签（2026-11-24）

固定FormatsMessages::getCustomMessageFromTranslator、getWildcardCustomMessages、getAttributeFromTranslations／LocalArray、getDisplayableAttribute逐类型追踪。语言custom尺寸先查类型键，再普通键；读取原翻译器custom组并展平，复用Str::is实现可跨点的语言通配规则。内联消息和属性名称保留单层通配，不能把两者合并为同一种匹配。组级attributes读取保留当前语言优先，不能逐项后备导致英文精确标签覆盖当前语言通配标签；本地空／0标签按固定源码退回翻译。嵌套email附加规则由原Factory自然取得同一属性映射，不在响应渲染层重写文字。普通驼峰属性通过既有Str::snake显示，未命名通配属性保留原路径和下划线。

新增22组固定PHP实际参考和文件加载器真实TCP，覆盖精确／通配／跨层、尺寸类型、字面点、内联优先、空名称、嵌套email及fr/en组级回退。初次参考夹具未刷新Facade原工厂，两个嵌套例错误得到未加载词条键；已修正容器绑定与Facade缓存再独立生成最终参考，不修改生产回退凑预期。三语新增JSON示例直接由Rust测试从正式Markdown提取，经文件加载、实际验证及真实HTTP执行，各语位置／名称／尺寸文案均核对，未维护平行示例资源。

旧validation_http将users.0.account_id人化为空格，与固定源码和参考第一例冲突；仅修正该预期，原始历史记录保持。最终message-catalog/gates-resumed.log九阶段全0：MySQL8.4.13十五目标72项0忽略，严格workspace/all-targets Clippy／fmt、三语构建和浏览器、实际main／Artisan帮助及inspire通过。首次中文示例多余空格及测试Clippy嵌套if问题已修复复验，不以失败当通过。原始日志、最终参考与SHA在target/tmp/docs-chapters-20261124/message-catalog/；本批MySQL和HTTP服务已退出。

本批关闭语言消息与属性教程及关联缺口；密码默认策略／泄漏查询／附加规则和全部179节最终对账继续，整章未关闭。

## 第二十二组：密码默认策略、原工厂附加规则与泄漏验证（2026-11-24）

固定Rules/Password、NotPwnedVerifier、UncompromisedVerifier及ValidationServiceProvider逐入口核对。旧v0.x只有基础validation runner和认证密码操作，没有该策略／泄漏类型可直接提取；保留当前复杂度正则，删除独立手写string/min/max执行分支，通过原Factory的同一make_shared装配链执行可迭代基础／附加规则，再执行after复杂度检查；读取原密码，不让DataAware修改子数据改变复杂度输入。初级失败不查询泄漏源，附加rules替换上一批，与email的追加规则语义区分。

Password::defaults读取当前应用策略，set_defaults注册Fn()->Password回调，required／sometimes基于配置，min／new保持显式构造；已构造策略不随设置变化。稳定Rust不能同名0／1参数重载，getter／setter分名并返回Result；Default trait不能返回Result时把装配错误保留为编译错误规则，不默默退回min8。应用工厂共享注册状态，回调在锁外执行，不把PHP静态全局策略复制为多应用数据竞争。教程约定为返回Password的回调，不声明PHP任意动态Rule返回值的反射能力。最大值0保留状态而不生成限制；applied_rules返回原生类型化状态（无新Serde协议），to_password_rules_string及公开IntoIterator真实消费者均验。

新增UncompromisedVerifier公开trait和NotPwnedVerifier默认Provider绑定，Factory保存弱容器并在实际查询时解析契约，重绑定可影响既有工厂／已构造验证器；独立Factory有显式set_uncompromised_verifier。trait用值＋有符号阈值替代动态数组。复用现有HttpFactory池、请求中间件和aws-lc SHA1；仅发大写前五位，Add-Padding=true，默认30秒，严格count>threshold，保留空值及字符串0、异常报告／失败开放、非成功HTTP响应回退。该行为与固定源码一致，是本地密码约束的补充，不宣称远端可用性保证。异常报告失败继续传播，不吞掉装配错误。

固定PHP实际456组策略／可空／输入、4组配置默认、Aware子验证器顺序及416组范围查询参考分别保存正式JSON。真实TCP回放456组，公开迭代器执行固定PHP导出的规则列表；两应用24并发默认策略互不污染，真实Artisan password:check核对四种默认入口、冻结对象和Facade验证。Aware规则实测data→validator→validate，原Factory resolver父／子各一次，原值复杂度失败且不调用泄漏源。既有历史api-f01密码参考完全保持。

泄漏验证以实际HTTP传输回放416组并额外150ms超时，核对本地范围服务收到的哈希前缀／padding、无完整哈希或明文、阈值、填充值／非数字计数、500、连接关闭、无查询和报告次数。另实际应用Provider/Factory/Facade路径验证已知泄漏422、初级失败不出站、503回退成功、运行期替换trait后已创建工厂不再出站。没有请求真实外部密码服务或发送用户密码。三语正式Provider＋控制器示例由tests/password_tutorial_docs.py直接提取编译，并在实际HTTP Kernel验201／422，不维护平行应用代码。

password/gates-final.log九阶段全0：MySQL8.4.13十八目标81项0忽略，workspace/all-targets严格Clippy／fmt、三语构建浏览器、实际main与Artisan帮助／inspire成功。最后新增公开迭代器与四种默认入口断言后，final-public.log四阶段全0：10项实际消费者、最终Clippy／fmt通过。早期缺泄漏回退文案、usize尺寸转换、测试ConsoleError转换及测试宏的Clippy问题均修复复验，失败日志保留。证据target/tmp/docs-chapters-20261124/password/；最终SHA一致，无本批存活容器或HTTP服务，临时源码／日志与热缓存保留。

本批关闭Password教程策略簇。完整章节人工复核另外发现Factory/Facade::validate和Validator::passes教程专用入口缺失，以及after回调列表、RedirectResponse::with_errors的Validator／数组消费者适配仍需核对修复；不以原make→validate绕行或errors()摘取关闭这些例子。全章逐节对账继续，本记录不是整个Validator／Factory类未入教程API的完整性声明。

## 第二十三组：教程专用执行入口、后置服务与重定向消息消费者（2026-11-24）

全文核查文件／encoding例子发现源文档直接调用Factory及Validator Facade的validate；此前只有make后手动validate，不是同一公开入口。新增Factory::validate与Facade::validate/validate_with，均委托原make执行，不建立第二套验证器。实例passes委托已有fails并取反，执行重新验证；实际手动空字符串unique示例不访问数据库。具体messages为已有错误包别名。

固定Validator::after和FormRequest::after的数组／服务对象消费者另行核查：after保持原闭包类型与推断，after_many接通已有Box回调列表，新增AfterValidationRule显式服务契约、after_rule／after_rules；into_callback接入原FormRequest回调列表，服务与闭包可按原顺序混合。稳定Rust不能自行实现对象调用运算符，因此通过具名trait表达__invoke；不修改原AfterValidation别名，不要求开发者手写平行调度器。

固定Contracts/Support/MessageProvider与RedirectResponse::withErrors接受Validator／MessageProvider／MessageBag／数组。新增真正的MessageProvider契约，Validator contract通过其原errors自动提供get_message_bag，外部原四方法实现无需追加样板；具体MessageBag也实现该契约。with_errors使用MessageSource的借用或拥有适配，既有消息包／验证器不增加一次克隆，实际ptr::eq验证借用；只有数组在边界构造原MessageBag。原生数组/BTreeMap支持单条字符串及多条字符串，复用已有去重、具名包与会话存储，不改数据格式、Session接口或原重定向行为。

实际TCP覆盖Facade直接成功／422、自定义消息／属性、passes空串及multipart encoding。真实Web按七种输入分别验证302、具名错误包、旧输入读取及下一次请求回收，包含dyn Validator和自定义MessageProvider；MySQL8.4.13专项通过OPENRUST_TEST_SESSION_MYSQL_URL显式接通实际database session，不把数组驱动当数据库验收。后置服务／列表／FormRequest均按实际入口验收，三语正式Provider之外新增直接验证／重定向／after服务示例由tests/validation_entries_docs.py直接提取，并执行实际HTTP Kernel，无平行示例实现。

final-entries/gates.log九阶段全0：25目标112项0忽略（含MySQL presence与Web session，其他目标依其原夹具），workspace/all-targets严格Clippy52.38s、fmt、三语构建浏览器、实际main及Artisan帮助／inspire成功。首轮新增Web夹具漏导入Validator trait已修复，不以未编译消费者计通过。源／测试／三语与冻结快照SHA一致，证据target/tmp/docs-chapters-20261124/final-entries/，本批MySQL和HTTP服务退出。没有宣称未改动数据库驱动的发布矩阵已通过。

全章审查继续发现sometimes多字段参数仍只接受单字段，以及内联消息碰撞／通配规则名优先级需要固定源码反例核对，登记下一簇，不以循环调用或精确查找绕行宣称完成。公开类型未入教程的其他完整API审计仍属原对账范围，本批不宣称整个Validator类已齐。两个章节的最终逐节结论待下一簇及最终组合复核。

## 第二十四组：多字段sometimes及内联消息层级优先（2026-11-24）

固定Validator::sometimes、dataForSometimesIteration及FormatsMessages::getFromLocalArray逐行核对。sometimes同一入口接收单字段、数组、Vec或借用切片，ValidationAttributes只承载有序字段参数；单字段Cow借用且最后一份规则移交，不给原单字段路径增加字符串分配或规则克隆。共享原单字段编译／追加管线，保留重复字段、空列表、字段顺序、通配项上下文和先编译再条件判断。关联差异：条件为false也须记录通配主字段；否则对已有具体字段的错误显示会错误人化下划线。现以原RuleSet.origins合并实现，未另维护平行状态。

内联消息依字段规则→规则→字段三个搜索层级，每层按调用方BTreeMap原生顺序匹配；不能预先用get精确键绕过较早的通配词条，也不能遗漏通配规则名。单层通配与语言目录跨层通配仍分开。PHP参考明确按相同键序ksort，裁决Rust原生BTreeMap的键顺序而非冒称保留PHP任意插入顺序；建议避免依赖重叠词条。普通无内联消息的成功验证路径不增加扫描，错误格式化仅构造一个字段规则键。

新增固定PHP实际10组多字段／条件／上下文、8组内联碰撞和24组原教程games条件参考。真实TCP通过Facade与validate_request回放全部42组，显式区分验证前原值的数值／字符串／布尔比较，复用既有标量桥而非手写另一套比较。三语正式多字段示例直接提取执行，覆盖数值及字符串100、缺失reason/cost和正常投影。原ForEach上下文与编译回归保持。

本批验收前重新从当前c904b0459归档整个HEAD，覆盖本批核定WIP、重建三语生成消费者，并触碰全部first-party Rust文件强制按当前源码重编译，未沿用初始ba6d快照当作最新HEAD。conditional-messages/gates.log九阶段全0：40目标159项0忽略（全部validation目标、请求签名、URL、表单未知字段、MessageBag、直接三语、真实MySQL presence／Web session等），workspace/all-targets严格Clippy46.84s、fmt、三语构建浏览器、实际main14.25s及Artisan帮助／inspire成功。源SHA与冻结组合一致，证据target/tmp/docs-chapters-20261124/conditional-messages/，MySQL及HTTP服务已回收。

逐章初步复核还发现Validation快速入门的Blade消费者不能据结构化errors属性读取判为完成：$errors->any()/all()/first()及old()助手需要真实模板／HTTP复验。进入下一簇处理，尚不关闭两个章节。此前消息簇的“保留旧精确字段消息优先”在本簇被固定源码碰撞证据纠正；历史证据不改写。


## 第二十五组：快速入门的错误袋方法与旧输入模板消费者（2026-11-24）

逐项核对固定ViewErrorBag／MessageBag、ShareErrorsFromSession、Validation文档的any/all/first/get/has、具名包、@error及old助手。原错误袋只转换为结构化映射，实际HTTP的$errors->any()返回500；不再用属性读取替代方法。新增私有只读ViewObject适配，方法委托原MessageBag，具名包共享原ViewErrorBag，属性／JSON仅按需投影；未修改验证、会话存储和原Serde形状。@error使用同一first语义并支持通配字段，旧结构化输入仍兼容。

old全局及request()->old复用既有异步请求桥与真实request.old，不复制Session逻辑；缺失可取默认，显式null不取默认，HTML照常转义。ViewObject仅新增默认关闭的只读属性投影，未公开服务反射。JSON序列化保持作用域检查并以RAII防止对象递归／超过64层对象投影，失败和重复别名不污染后续序列化；普通结构化数据保持原边界。Js::from沿用安全转义，不将对象字符串当脚本。稳定Rust的显式投影及资源边界写入三语Blade正文。

真实TCP覆盖默认／具名错误、all/first/has/get通配、@error、旧输入／null／HTML转义、下一次请求回收、Js::from及正式三语Blade代码直接提取执行；5项专项通过，序列化形状与原类型直接比较。validation-view/consumers.log为MySQL8.4.13的51目标230项0忽略，含验证全套、真实database Session和Blade／View回归；严格workspace/all-targets Clippy48.36s及fmt通过。文档首轮因重复锚点失败，改为独立displaying-errors-and-old-input后构建、浏览器、实际main及Artisan帮助／inspire均通过，最终正式代码提取5项再次通过。blade_arithmetic旧缓存测试硬编码45而生产已48，改为从实际当前版本降一验证重建，不更改生产缓存版本。源／测试与冻结快照一致；证据target/tmp/docs-chapters-20261124/validation-view/，本批临时MySQL及HTTP服务已回收。

本簇关闭Validation教程的错误展示／表单恢复。其余Blade指令全集、可变消息袋模板操作不属于本轮两章范围，不以本簇宣称Blade整章完成。两个章节逐节总对账与最终实际二进制验收继续。


## 第二十六组：正则修饰符与模式解析的关联缺口（2026-11-24）

逐节复核regex/not_regex的preg_match契约，发现原白名单拒绝合法S/X/r和修饰符空格／换行。对照PHP官方修饰符说明（https://www.php.net/manual/en/reference.pcre.pattern.modifiers.php）并使用既有固定框架＋PHP8.5.12实际执行；修复前真实Rust消费者首例返回InvalidRule。复用原PCRE2编译器：S接受且不额外study，X沿用原严格转义，r传入原生内联选项；模式前ASCII空白及修饰符间空格／换行按参考处理，未知标识仍拒绝。未新增引擎、依赖或通用全仓替换。

正式JSON金标准480组同时保存独立验证值与真实固定TrimStrings／ConvertEmptyStringsToNull管线后的HTTP结果，避免把输入归一化差异误归正则。最初HTTP夹具直接套用未归一化参考，在空串及带换行输入失败；现改由原生参考中间件实际计算wire结果后重跑，未更改生产中间件。Factory与Rule::parse／规则数组消费者、真实TCP逐组核对regex与not_regex成功／422消息；J重复组、n、A、D、多行、r跨ASCII字符、重复修饰符、成对分隔符及非法标识均覆盖。

regex-modifiers/gates-accepted.log九阶段全0：MySQL8.4.13共55目标236项0忽略，含全部Validation、真实database Session、三语提取、实际Artisan生成类型消费者、URI和Blade／View；workspace/all-targets严格Clippy／fmt、三语构建浏览器及实际main／Artisan通过。验证View新测试文件另做独立rustfmt并纳入本轮回归，补齐cargo fmt未排版的新测试文件。最终源与快照一致，证据target/tmp/docs-chapters-20261124/regex-modifiers/及final-chapters/；本批MySQL／参考容器和HTTP服务回收。

两个章节最终人工复核另发现FormRequest只有redirect_to，缺少教程RedirectToRoute对应的专用目标；不能用业务层手写Url::route绕行当作已实现。进入下一功能簇，最终逐节账本保持待提交，不提前宣布整章关闭。


## 第二十七组：FormRequest命名目标与异常重定向消费者（2026-11-24）

最终逐节阅读发现RedirectToRoute教程没有原生配置入口，先前redirect_to路径测试不能替代。按固定FormRequest::getRedirectUrl的明确URL优先、命名目标次之机制，新增默认无操作的FormRequest::redirect_route；失败后经已有UrlGenerator::for_request解析，不在成功路径构造URL，不让业务代码手动拼接替代。相邻缺口：原显式路径未走UrlGenerator::to，异常渲染器还只接受相对路径，会把合法生成的绝对URL丢弃；三语正式消费者实际返回/复现该问题。现接通原生成器，并让应用显式配置目标接受现有valid_http_url验证的HTTP(S)地址；Referer／会话上一页的既定同源过滤不变，不把应用配置目标与外来请求头混为一类。

固定PHP实际五组核对命名目标、URL优先、未知路由及空串／字符串0。真实TCP与MySQL8.4.13 database Session验证具名包／旧输入、路径优先、空目标回退、应用明确绝对URL、未知路由／非法URL的500以及成功204不解析目标；普通JSON失败422。原先显式路径Location由相对值修正为原URL生成器的绝对值，测试同步固定机制，未改错误包、JSON格式或状态码。UrlError经既有HttpError转换保留源链。三语正式trait＋类型化动作示例由validation_entries_docs.py直接提取并通过真实Kernel，实际路由与控制器生成消费者继续通过。注册夹具误用了Facade::get返回引用的写法，已使用既有正式get_with配置闭包，未另造静态可变路由引用；该原生借用方式写入最终适配说明。

form-redirect/gates-v4.log九阶段全0：55目标237项0忽略、workspace/all-targets严格Clippy25.46s、fmt、三语构建／浏览器、实际main构建、Artisan帮助及inspire通过。首次Facade链式夹具、正式示例暴露的绝对目标丢失及私有模块导入问题均已修复复验，失败不计通过。最终相关源码／测试／三语SHA与隔离快照相同。最后再次启动实际二进制，随机非默认端口首页GET／HEAD、/up、API JSON404、Web HTML404、Accept JSON404及SIGINT退出0；实际Artisan list/inspire/help make:request/help make:rule/lang:publish和重复发布保留自定义单键通过，证据final-chapters/runtime-final.log及runtime.json。三语Artisan生成配方实际生成的原始Request／普通与隐式Rule编译后通过HTTP，记录final-chapters/{request,rule}-generator*。

## 两个章节最终闭合（2026-11-24）

[逐节最终复核](final-review-2026-11-24.md)与[机器清单](section-map.json)覆盖固定URL14节、Validation179节。最后78个原pending／partial逐项阅读文档、追踪源类型并关联具体消费者；既有101节保留原证据并纳入组合回归。没有凭扫描、测试数量、内部等效写法或这几个关键词关闭整章。源章节SHA与锚点顺序已复核，章节约定范围无剩余功能缺口／外部阻塞；不扩大为整个框架、完整Blade／Validator类或跨平台发布已完成。

保留的已确认URL策略、原生类型／Trait／异步／错误与可选参数适配、Data准备边界、BTreeMap键序、只读对象资源边界见最终记录与三语正文。第一至二十七组历史失败和当时停点保持，不改写为一次通过。新出现的API响应／配置／路由以及路由原型属于其他协作者，未放进本轮快照、验收或提交；活跃工作区一次全差异格式复核命中其未排版代码后已改用本批显式范围核对，不修改他人代码，也不声称当前全工作区均已验收。

临时MySQL／参考容器和本批HTTP服务已退出；生成器临时应用自动回收。保留正式源码快照、原始日志、参考和可复用热缓存，仅回收最后运行验证的临时发布目录。最终核范围本地提交，禁止推送。
