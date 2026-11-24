# Error Handling 教程逐节对账

## 顺序与固定参考

URL Generation与Validation已按两个固定章节闭合，不重开。按交付映射The Basics顺序，下一章为Error Handling；原计划01-06默认报告、01-07渲染／脱敏及04-08报告限流已具备实现／回归，但不能据此关闭整章教程。原生日志、限流与视图作为已有依赖，不复制平行实现。[固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md)、固定框架1449bf241017087fbd38c118ecaabef3f6797fdc的Handler／Configuration／Contracts／helpers逐入口核对。

旧v0.x有集中exceptions模块及report/render/context配置，无对应support report助手或完整类型自身行为／标记契约可直接提取；复用当前处理器和公开门面，避免重装旧版错误体系。先报告入口，再类型自身行为／标记，再错误视图与发布，最后总对账。[18节清单](section-map.json)明确保留未闭合项。

## 第一簇：report助手

固定Foundation/helpers的report调用当前应用ExceptionHandler。此前只有Exceptions::report与Application::report，不能以它们的存在代替support助手。新增support::report，薄委托现有门面的拥有所有权入口；不安装新处理器，不复制日志、忽略、限流、去重或响应流程。异步LogResult沿用已有日志契约；SharedException克隆保留原实例，普通独立错误不是同一实例。该助手不生成HTTP响应。

三语正式示例由tests/error_tutorial_docs.py直接提取编译，三组真实TCP保持200／recovered并核对报告回调和原日志落盘；不是平行示例。另实际Application::handle_command执行report:recover，报告后仍输出continued并退出0；真实处理器验证共享实例去重，替身捕获验证原类型／原身份并保留其原本不执行默认处理器去重的语义；无应用作用域明确返回错误。实际main构建与Artisan inspire通过。

十目标93项0忽略，workspace/all-targets严格Clippy、fmt、三语站点构建通过，浏览器结果在最终补录。首次候选误带共享support/mod的他人data_forget_parts导出，已剥离并保留工作区该改动；初次新测试漏Facade trait、生成测试未排版均已纠正复验，不记为通过。没有改动API响应／配置／路由或其三语在途正文；共享errors文档仅核定本批新增助手段，support/mod仅核定助手导出。

本批证据target/tmp/error-tutorial-20261124/，源码基线001e18c44及本批接线；后续共享HEAD推进与本批验收边界分别记录。18节整章仍开放，尤其类型自身report/render、ShouldntReport和错误页／发布，不以本批93项关闭这些差异。


第一簇最终补验：吸收已提交03990c985三语说明及653ecd726请求API前缀实现；本批十目标93项0忽略仍通过。额外api_prefix_boundary目标有1通过／1失败，失败为响应缺少success字段的封装断言，原始combined-consumers.log保留；该API封装链路仍在其他协作者工作区，未夹带修复，也不宣称全仓回归通过。磁盘触线中断的Clippy不算通过，统一守卫随后清理本项目profiles并记录释放83,040,133,120字节，源码／日志／依赖／数据卷保留；恢复后workspace/all-targets严格Clippy2m31s、fmt、三语构建、实际main／Artisan均退出0。三语浏览器／根及子路径／无导出的API参考检查全0，最终原身份断言再3项与定向Clippy58.89s通过。最后使用独立索引核定共享文档助手段及support导出，保留其他在途块；类型行为簇尚未验收，不在此提交。


提交后复核：第一簇ea9e11d72的父提交已推进到5e1144182，旧候选整文件暂存误删了该父提交刚增加的support::data_forget_parts私有导出。工作区字节未被改写，该行仍存在；现以当前完整HEAD归档＋仅恢复此导出构造独立快照，cargo check --lib已通过（32.09s），立即单独恢复提交，不把这种跨批回退当作合理差异。后续共享文件候选须以最新HEAD为底只应用本批差异，并逐块核对非本批段与父提交相同，不能只核定本批路径。原始对账和失败证据保持。


## 第二簇：异常类型自身行为与忽略标记

固定Handler的实例report／render和context不能仅以bootstrap回调代替。新增公开contracts::debug::ExceptionBehavior及Configuration/Facade的use_exception::<E>，注册一次具体类型元数据，报告和渲染都借用原异常，不生成相同字段的替身。自身report先于全局回调，Stop表示已处理，Continue使用原回调和默认日志；render返回None则接续全局渲染器／默认响应。context先于全局字段和默认guard身份，每次实际报告只计算一次，不在诊断序列化时重复执行；保留原类型名登记。重复注册无重复副作用，两应用隔离。

新增ShouldntReport独立契约；标准Error类型擦除无法自动发现额外trait，因此into_exception()／SharedException::unreported是明确的原生所有权边界，不谎称裸类型实现marker后自动可识别。标记和原始Arc身份一起保留，new/from_arc/from_typed_arc及clone均保持。标准处理器在限流／回调／日志之前忽略；stop_ignoring不能取消类型自身标记。只穿过框架透明传输层，不因业务source包含被忽略错误而忽略外层类型；映射为新根后按新根策略判断。没有更改序列化字段、事件、CLI或错误显示文本。Rust报告方法用ReportDecision替代PHP的void／bool，依赖可由构造器经已有容器取得，不反射参数签名；这不是缺方法的替代写法。

四项专项含真实TCP、Artisan exceptions:own-methods、实例／全局顺序、None回退、具名context、一次调用、所有权构造器、映射、外层source和运行期Facade／应用隔离；三语正式完整代码直接提取编译并以真实TCP分别核对500／409、已处理／忽略及原日志。13目标102项0忽略通过，含原93项及已提交API前缀2项；先前API封装断言失败已随5e1144182的完整已提交配套进入快照，现复验通过，不再作为该范围阻塞。workspace/all-targets严格Clippy54.20s、fmt、三语构建／浏览器、实际main及Artisan inspire全0。证据behavior-{tests-v2,doc-tests,consumers,clippy,gates,browser}.log；首次测试Body／LogContext／异步App::make使用错误已纠正，不计通过。本簇不包含尚未验收的错误页对象、默认页面／发布、渲染失败处理；后两节继续开放。


## 第三簇：错误页对象、专用回退与模板发布

固定文档custom-http-error-pages及Handler::renderHttpException/getHttpExceptionView要求真实$exception、独立空ViewErrorBag、精确页与八种专用页，并提供vendor:publish --tag=laravel-errors。此前仅有status/message及三个排除状态，没有默认模板发布，且视图失败被吞掉；现复用原Factory／ViewObject／Provider发布注册表，无第二套模板引擎或发布器。旧v0.x未提供可复用的对应模板实例／发布实现。

新增对象持有HTTP边界已有Arc或映射SharedException，读取原错误，getPrevious穿过透明共享包装，最多32层；不复制消息为错误DTO。模板composer修改原source后可立即读到新消息。HTTP Kernel及Next中间件两个实际边界均接线，映射使用映射后的拥有者；并发32请求／取消后Weak强引用归零及后续消费通过。不透明借用的低层同步render无法伪造所有权，正式文档明确使用HTTP边界或SharedException；四个PHP模板方法及原生别名保留，模板表达式不机械改名。

精确应用页优先，普通状态走4xx/5xx，401/402/403/404/419/429/500/503走内置专用页。新增八份独立编写的中性HTML，内置与发布后复用同一资源，默认不泄露内部消息，不复制上游页面样式。沿用既有外部laravel-errors标签，属于必须保留的CLI契约；发布默认保留已有文件，--force覆盖。视图错误先报告再respond，生产保留原HTTP状态并回退内置响应，调试选择新的ViewError／500；避免递归失败。

6项专项包含三语正式Blade片段直接提取、真实TCP、原始对象／来源／响应头、空错误包、映射对象、八专用及通用回退、JSON绕过、失败报告先于respond、真实Application::handle_command发布及覆盖保护。最初Blade测试指令边界写法及HeaderMap夹具类型错误已纠正；随后实测暴露Next边界缺接线并修复，失败日志不计通过。pages-tests-final.log 6通过0忽略。完整关联回归与门禁结果后补。

最终再核对rendering-exceptions中的命名视图响应：公开Response::view本身是异步，但旧render回调为同步，不能以手拼HTML替代此消费者。该关联入口在本簇之后单独接通并验收；在其完成前不关闭整章。

第三簇最终门禁：18目标138项0忽略通过，workspace/all-targets严格Clippy1m08s、fmt、三语构建全通过；最后仅排版测试函数后6项复验通过，定向Clippy再核对。浏览器check_browser/check_scope/check_api_snapshot --without-export均0。实际main的help vendor:publish、8页发布／保护／force、HTTP /up、自定义404、启用既有业务封装后的JSON404、HEAD空正文、发布后默认404、Artisan inspire及Ctrl+C退出0通过。首次实际JSON夹具沿用了旧Not Found文本，已按当前既有默认封装Resource not found.修正断言；重复使用临时目录导致original取到自定义页的夹具错误已换全新目录复验，均不修改产品默认值或把失败计为通过。证据pages-{consumers,clippy,fmt,docs,main,artisan,browser,runtime-final,tests-formatted,final-test-clippy}.log；无存活测试服务器。


## 第四簇：异步命名视图消费者与最终逐节复核

固定rendering-exceptions正式示例返回response()->view。现有Response::view是异步请求级转换，不能在同步render闭包await，也不能以手拼HTML代替。新增Configuration::render_async、Facade::renderable_async及ExceptionBehavior::render_async，复用原渲染器注册表与实际ResponseFactory；同步／异步配置保持同一注册顺序，类型自身方法仍先执行。类型异步默认方法只委托同步render一次；Err(HttpError)报告新错误并走默认响应，不递归重入失败的用户回调，最终respond一次。同步Exceptions::render保留同步语义，不阻塞执行器；正常HTTP及异步render_http执行两类回调。异步闭包使用Send Future及Sync错误借用，是原生调度约束，不把缺少命名视图消费者归咎语言限制。

真实专项覆盖Response::view（Request::with_container）及view_for_request、模板错误回退、精确回调顺序、None到后续／默认、异常自身异步方法、映射后的通用错误、运行期Facade和respond一次。三语新增Rust／Blade示例从正式Markdown直接提取，实际TCP请求带Accept: application/json仍得到自定义HTML409及转义后的原消息；没有平行配置实现。旧测试包装处理器不支持configure，已在替换前注册运行期回调；同一HttpError类型的两条映射会按既有唯一类型规则替换，夹具已合并为同一映射，不改产品契约。失败原始日志保留，不计通过。

最终19目标142项0忽略通过，含原异常报告／渲染／Context／映射／替身／关闭处理、视图原生值／查找、请求与Response/Session门面、API前缀及新教程消费者。workspace --all-targets --locked严格Clippy49.99s、fmt、三语构建通过；实际main的HTTP/Artisan/Ctrl+C复验通过，真正./artisan脚本的inspire及vendor:publish也退出0。最新已提交业务响应三语说明进入最终文档构建，不夹带其正文。原记录证据async-{tests-v3,consumers,clippy,fmt,docs,docs-final,main,artisan,runtime-final,launcher,browser}.log。

### 最终范围与必要差异

18个固定文档锚点与section-map逐项集合相等，每项有实际公开契约和消费者，不再使用重复泛化证据代替具体入口；该18节教程范围闭合。URL Generation14节与Validation179节保持已闭合，不重做；Settings既定功能状态保持不变。此结论不等于完整Debug类逐方法、整个View/Logging模块、跨平台或全框架正式发布。

保留四类原生边界：Error/Result与显式StatusCode／headers代替throw和动态可选参数；类型可选方法用use_exception建立元数据，ShouldntReport在擦除前保留标记；共享错误持有真实Arc，不从不透明借用伪造拥有者；I/O渲染使用显式异步入口，同步低层入口不运行异步回调。模板来源最多32层，避免循环／无界遍历；模板表达式四个上游方法名及原生别名均保留。默认错误HTML为独立编写的中性页面，保留状态／通用标题和覆盖机制，不承诺上游视觉像素一致。laravel-errors属于既有外部CLI标签，不因品牌清理更名；项目身份和crate路径保持。没有修改序列化格式、事件、数据库、既有CLI默认值或业务响应开关。

本轮无真实外部阻塞；没有数据库或驱动实现变更，不将这些定向结果计作三库／跨平台发布验收。两个32GB磁盘中断按守卫规则清理profiles并恢复验收，未把中断算通过；日志／源码／依赖下载／数据均保留。提交前保留其他协作者工作区和索引，全部提交本地且禁止推送。最终提交号与清理结果另附。

最终复验命令入口（均由 `tests/build_guard.py --workflow docs-chapters --target-dir target/tmp/rust-naming/cargo-target --tmp-dir target/tmp/pv` 包装Cargo，源码位于 `target/tmp/error-tutorial-20261124/source`）：

- `python3 -B tests/error_tutorial_docs.py --source target/tmp/error-tutorial-20261124/source` 从三语正文生成四个教程测试目标。
- `cargo test --manifest-path <source>/Cargo.toml -p openrust-framework --locked`，逐一传入 `--test`：`exception_response_docs_generated`、`async_exception_docs_generated`、`error_page_tutorial`、`view_native_values`、`view_finder`、`request_facade`、`response_session_facades`、`api_prefix_boundary`、`exception_behavior_tutorial`、`exception_behavior_docs_generated`、`report_helper`、`error_docs_generated`、`exceptions_facade`、`exception_reporting`、`exception_rendering`、`exception_map_facade`、`exception_context_facade`、`exception_handler_fake`、`exception_ownership`、`without_exception_handling`。
- `cargo clippy --manifest-path <source>/Cargo.toml --workspace --all-targets --locked -- -D warnings`、`cargo fmt --manifest-path <source>/Cargo.toml --all --check`、`cargo build --manifest-path <source>/Cargo.toml --bin main --locked`；最新HEAD额外两份已提交路由原型测试进入最终Clippy组合，不夹带本批提交。
- `npm --prefix <source>/docs run build`；`node <source>/docs/tests/check_browser.mjs`、`check_scope.mjs`、`check_api_snapshot.mjs --without-export`。Playwright／浏览器采用既有安装，环境定位见临时browser.py。
- `main artisan help vendor:publish`、`main artisan vendor:publish --tag=laravel-errors [--force] --no-interaction`、`<source>/artisan inspire`／`vendor:publish`；实际独立main在随机端口、独立APP_BASE_PATH、array Session及禁用其他Runner下处理 `/up`、普通/JSON/HEAD缺失页，再Ctrl+C核对退出0。禁用后台服务仅用于HTTP/Artisan专项隔离，不修改应用默认配置。

最终逐例补核：固定respond示例的419→back()->with(message)不能用只读回调直接完成。现补Configuration::respond_async与Facade::respond_using_async，最终回调允许借用同一可变Request；同步与异步最终回调互相替换，后注册者生效，不叠加。原同步入口保留，正常HTTP等待异步回调；没有引入全局Session或第二套响应器。三语正式configure_response／expired_response代码直接提取，file Session下真实TCP覆盖Bootstrap与运行期Facade两种入口：419转302、Location以/form结尾、加密Cookie往返、下一次读到消息、再下一次已老化删除、404保留且旧回调不再执行。定制重定向不可用时保留原响应是文档示例明确策略，不吞掉框架内部错误。底层回退逻辑由原HTTP渲染共享，异步渲染错误同样保留预制HttpResponseException与既有验证重定向，不另造响应流程；预制418及respond只一次已专项覆盖。

最终完整组合20目标145项0忽略通过；最后一次workspace/all-targets严格Clippy48.84s、fmt、三语站点构建、三组浏览器检查均退出0。实际main重新验证/up、普通／JSON／HEAD404、定制／发布模板、发布保护／force、inspire及Ctrl+C退出0；最后真正./artisan脚本的inspire与发布也通过。证据respond-{tests,consumers,clippy,fmt,docs,main,artisan,browser,runtime-final,launcher}.log。之前142项阶段结果保留为当时状态，最终以本段和18节清单为准；无外部阻塞。用户的“下一章”按计划关闭Error Handling；Logging仅为接续顺序，未在本轮开始新模块。
