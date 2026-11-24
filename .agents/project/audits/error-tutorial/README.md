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
