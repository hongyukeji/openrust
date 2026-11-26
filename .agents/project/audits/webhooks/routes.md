# Webhook 公开路由、控制器与具名工厂

## 范围与固定依据

持续目标中的 WH-00／WH-01／WH-02 后续批次；不关闭完整批次或 Webhook 模块。接续[原文接收记录](receiver.md)，固定 Client `ff886d1bae33d35ad98b6c91403f280df158129d`。本批逐代码复核 Provider 的宏／Config 绑定、WebhookConfig 构造及 Controller／Processor，沿用[首批调研与许可证](README.md#固定来源与采用理由)，不另建路由引擎、全局工厂表或新的依赖。

另联网核对固定 [Laravel Router](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Router.php#L1241)：currentRouteName 读取 Router 的实际匹配状态，而非 Request 自定义 route resolver。实现以请求内的既有匹配名称保存这个状态，避免在共享 Router 上添加可变 current 字段或跨请求共享。未执行 PHP／Composer；不将源码阅读写成 PHP 测试通过。

初始隔离基线 `4439351b2`，验收集成基线 `cafd464d66945cbe5131672dfe02b659218d40af`。集成只读取其他协作者已提交的树，不带入其 WIP；归档、原始成功／失败日志在 `target/tmp/webhooks-routing-20261125`。Rust 1.94.0，locked，复用本持续目标独占 `target/tmp/driver-extensions/cargo-target`，正常增量、并发及 build_guard；不把该隔离组合等同于实时共享工作树所有 WIP 已验。

## 公开入口与职责

- `WebhookRoutes` 为原生 `Route` Facade 提供 `webhooks`／`webhooks_with`；`WebhookRouter` 提供底层 Router 入口。Rust 缺省参数通过 `WebhookRouteOptions` 表达，默认配置 default／方法 post，name／methods 可改；Facade 返回 Result<()>，原生 route 修改放在 with 回调中，底层 Router 返回 &mut Route。这是避免登记借用逃逸的 Rust 表达，不以手工 post 夹具代替宏入口。
- 方法白名单严格保留 get／query／post／put／patch／delete，拒绝大写参数、head／options／trace／connect 等；GET 继承核心 HEAD，OPTIONS 自动协商与405／Allow 顺序由现有 Router 完成。错误方法在登记前整体拒绝，未留下半条路由。组前缀／名称／域名／中间件、覆盖、约束、默认值和缓存均走原生机制，没有重排路由或改变缓存 version1／methods 格式。
- `WebhookController::invoke` 是公开入口，原生 Handler 使用 Provider 的 WebhookConfig 绑定和同一 Processor。配置名来自实际匹配路由，复核后的 `Route::current_route_name` 不采用 Request 自定义 resolver 名称；普通 CLI 应用上下文返回 None。`Request::route_name` 则保留自定义 resolver 的语义，route_is 复用该读取。没有活动 App 的 Facade 仍报绑定错误；HTTP 显式借用处理器先用 with_container。
- 唯一名称模式追加 Str::random(8)，配置选择保留固定源码的 after 首个 webhook-client-／before 首个点语义；不是认证令牌。Route 列表／缓存及多进程重启时各自生成随机名称，配置解析仍一致。不擅自修正含点配置名在此模式下的源行为。
- 类型化注册入口为 WebhookRuntime::register_signature_validator／register_webhook_profile／register_webhook_response／register_webhook_store／register_job_dispatcher。只复用当前应用 Container 的具名 Factory<T>；别名仅是编译好的 Rust 回调键，不反射执行字符串。按角色隔离相同 default 别名，未知／错类型返回带 field 和原 BindingResolutionError 的错误，不跨应用或账号回退。默认注册不覆盖已存在的绑定。
- `from_application(...).await` 在指定应用的原生 Callback／call 上下文构造，并让 Processor 在同一应用执行，嵌套不同 App 后恢复调用者；未新增通用作用域 API。HTTP 的工厂与流程共用一次请求借用，实际自定义验证器工厂可注入 RequestHandle 并在验证阶段使用，借用结束后不得持有使用。手动消费者示例用原生 App.call 包住构造和执行，适用于依赖当前上下文的组件。
- 固定构造顺序保留：Validator／Profile／Response 创建在前；模型／存储只在验签和 Profile 通过后解析，因此有效忽略事件不实例化未知或空模型引用。Job 先核对注册，保存后经 prepare 构造／准备，再清旧异常并 dispatch；构造失败也在异常保存范围。工厂接收不可变配置和 Container，Dispatcher 的 prepare／dispatch 接收实际 WebhookCall，不把派发接缝冒充已完成的上游抽象处理 Job。
- signature_validator／webhook_profile／webhook_model／process_webhook_job 为必填实现引用，不因已注册内置实现就静默补齐缺字段；仅 webhook_response 缺失或 null 选默认值。store_headers 的 null 按源空默认处理，不扩大为保存所有头。别名校验／长度是明确 Rust 约束；模型字符串的实际绑定延后到保存阶段，保持有效忽略事件的行为。
- 默认库仍为已实现的 database Store，需可信稳定 scope，connection 为具名数据库连接或 null。发布模板将 scope 留 null、Job 名留空，未配置端点不能假确认；无 Provider 时路由登记即失败。低级 WebhookRuntime::new／原有配置、签名与模型方法仍可使用。新增 WebhookConfig 的五种实现引用、scope／connection Builder 支持不经 JSON 的运行时配置；密钥不进入 options。

## HTTP 与资源边界

固定 InvalidWebhookSignature／InvalidConfig 是普通异常，公开控制器默认500；旧手动夹具的403／404是其应用策略，未复制到包默认行为。错误来源保留，应用仍可在异常处理器映射。超限原始正文返回原生413；没有关闭全局 CSRF，测试只在应用层排除单一 web 回调路径。

控制器读取原文后创建不可变 WebhookRequest，再进入用户实现；保留原始正文／URI／方法／头，即使全局输入中间件已经解析并修整 JSON／form，也不把修整值重新编码用于验证。新增 Request::body_bytes_with_limit 使用较小预算检查缓存和流，保持原生8MiB上限。body_bytes 仍为 async 入口，两个入口共享同步检查／写回及原有有界 collector，不新增 Future 嵌套或逐次装箱；失败和取消后不把已消费流当作空正文重读。前置中间件已读取的内存不会被较小端点预算倒退消除，三语说明该边界。

## 实际验收

- Client 契约6项、工厂14项、Processor10项、Provider1项通过。包含字段缺失／可空响应／不可静默默认、错类型绑定、无 Provider 登记失败、CLI 无当前路由、源首个点规则、运行时 Builder、不同 App 的注册／Facade 构造及执行隔离、调用者恢复、模型延迟、构造错误与异常清除顺序。不是仅搜到名称或内部函数即算公开入口通过。
- Core 原文预算6项实际验证 Content-Length 在读流前拒绝、分块首次超限即停止、不提高全局上限、缓存仍受较小预算约束、空预算、取消后不能当空正文重试。Request 输入30／Facade17、路由方法2／快捷入口3项，以及已提交观测查询1／作用域 Future 布局3项回归通过。骨架2项和三语6段文档实际编译消费者通过，包含 Facade trait、注册工厂及 App 上下文手动处理入口。
- PostgreSQL18.7（180007）、MySQL8.4.13、SQLite：每库公开入口111个真实 HTTP、69个独立 Artisan Worker，分别覆盖未缓存、config＋route 缓存、唯一名称＋双缓存三种状态。真实 migrate／route:list／config:cache／route:cache／clear、缓存格式、默认配置、七种匹配方法、HEAD 空响应、OPTIONS／405 Allow 顺序、大小写方法登记拒绝、重复覆盖、组／域名大小写／中间件、默认参数／约束、Unicode／百分号路径与查询编码、路径／正文不能选择其他账号、替换 Request resolver 不能改变当前匹配配置均通过。
- 同批验证默认／Standard 协议、真实自定义验证器（保留 HMAC 并注入当前请求）／Profile／Response／数据库 Store 工厂、有效忽略不触及未知存储、Job 工厂失败记录、默认无效签名500／未知配置500、超限413，以及仅指定 web 路径排除 CSRF、其他 web 路径419。JSON／form 重复字段／multipart／二进制原文从持久记录及独立 Worker 回读；已解析的大 JSON 缓存仍被较小端点预算拒绝。
- 前批接收回归在当前最终二进制重跑，三库各18 HTTP／6 Worker，包括存储表不可用、派发失败、异常大小及清除、不同 scope 读取隔离；另重跑50个签名 HTTP／Artisan／包发现／发布保留定制／缓存无明文密钥回归。未把这些 Worker 读取等同于正式可靠业务 Job／租约恢复或集群通过。
- 集成基线后全 workspace／all-targets＋两 feature 严格 Clippy、全仓 fmt 通过；最后增加域名夹具后又做该应用目标严格 Clippy／fmt。三语内容与站点构建、根／子路径三语两包12页面检查通过。已保留原有500kB块体积提示；许可固定检查沿用实际执行，不声称完成制品发行与商业合同审批。

## 失败、资源及未闭合项

原始4439351基线在 Fortify two_factor_model／installed_actions，以及相关 Core 模型教程的严格 Clippy 出现布局查询深度溢出。首次误猜为新读取封装导致；将本批全部框架文件恢复成原始基线的独立对照仍失败，已纠正归因。没有提高 recursion_limit 或降低标准。只在其他协作者提交 cafd464d6 的作用域／SQL观测类型擦除修复后集成复验，最终完整严格门禁通过；这些修复不列入本批交付文件。基线探测与失败日志保留。

首次方法工厂测试误将 &Path 传给 Application::configure，编译器拒绝后按原契约传 PathBuf；原生 App.wrap 需要编译型 Callback，修正为既有 App.call／Callback。大正文提早拒绝使完整发送的 Python 客户端出现 BrokenPipe，改为 Expect 预检并实际读取413，且另以完整 JSON 验证已缓存正文预算；没有把断连接当通过。构建曾触及32GB停线，守卫安全终止（75）、持锁清理约112GB编译产物后重跑，未删除源码／日志／数据卷，不把中断算通过。

仍缺：完整新应用安装／迁移发布、配置字段／类继承最终裁决，解析 payload／附件、get_attachments／prunable，正式处理 Job／重试／连接／超时，持久验证分类／Hint／Deferred／去重冲突／领取代次／恢复与补偿。发送 Builder、安全连接、投递／运维、双拓扑及供应商／交付平台检查继续后续批次。组件字段以不可变配置元数据＋生命周期 Runtime 表达，不将其他类型同名当作该源类型全部字段已对齐。终轮日志为 accepted-contracts.log、accepted-app.log、accepted-sqlite.log、accepted-http-worker.log、accepted-docs.log；临时应用／隔离数据库和站点产物已清理，保留源码、日志及可复用 Cargo 缓存。WH-00至WH-07及总目标仍进行中；不推送、不发布、不操作正式环境。

## 接收扩展入口逐类型复核

- 固定上游配置仓库按名称覆盖，未知名称返回空值。Rust 仓库按相同名称区分大小写地覆盖，保留已取得的配置快照，并在应用及请求作用域内隔离。已有契约测试核对显式覆盖与未知名称，Provider 测试核对声明中重复名称取最后项及独立应用读取各自凭据。现行路由验收另核对当前匹配路由选择的具名配置。 [固定配置仓库](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookConfigRepository.php)
- 固定上游默认 Profile 允许处理。Rust 公开 trait 接收请求及只读配置，可返回类型化错误。原生默认实现通过具名工厂和手动 Processor 使用。既有真实 HTTP 验收覆盖默认接收、有效请求被自定义 Profile 忽略，以及被忽略的请求未写库或入队。 [固定 Profile 接口与实现](https://github.com/spatie/laravel-webhook-client/tree/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookProfile)
- 固定上游 Response 接收请求和配置，默认返回 200 及 `{"message":"ok"}`。Rust 适配保留这两个输入与默认响应，工厂可返回原生 `Response` 或类型化错误。既有 HTTP 入口在缓存前后核对默认 200 和 JSON、自定义 202 和正文。 [固定 Response 接口与实现](https://github.com/spatie/laravel-webhook-client/tree/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookResponse)

本次复核的是五个直接声明类型及对应的既有消费者证据。修改仅涉及逐类型清单和验收记录。Job 进程退出后的领域结算、模型继承及外部拓扑继续按各自门禁验收。
