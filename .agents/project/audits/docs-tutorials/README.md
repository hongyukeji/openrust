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
