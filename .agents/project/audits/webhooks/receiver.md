# Webhook 手动接收与原文持久化批次

## 范围和固定依据

持续目标中的 WH-00／WH-01／WH-02 局部实现，不关闭任一完整批次。沿用[首批固定来源与许可裁决](README.md#固定来源与采用理由)，Client 固定 `ff886d1bae33d35ad98b6c91403f280df158129d`，复核 `WebhookProcessor.php`、`Models/WebhookCall.php`、`WebhookConfig.php`、`Events/InvalidWebhookSignatureEvent.php`、`Jobs/ProcessWebhookJob.php` 与 Provider。未重新执行 PHP／Composer，Rust 参考行为按固定源码、实际消费者及既有字节向量验证。旧版无可复用的通用 Webhook 包，保留 Pay／Cashier 现有专用通知流程，不混用其事务或表。

源码归档基线 `be4cf61a0a81c62dfbf37682e229c58970f81741`。已用 Git blob 对比确认隔离归档中全部非本批跟踪文件符合该树；`artisan.bat` 只有既定 CRLF 导出差异。验收不包含其后其他对话的认证／Session／New Relic 改动，不宣称整个实时工作树已验。源码及原始日志在 `target/tmp/webhooks-receiver-20261125`，Rust 1.94.0，复用同一持续目标的独占 `target/tmp/driver-extensions/cargo-target`，经 build_guard、locked、增量与正常并发。

## 实现与明确适配

- `WebhookProcessor::new/process` 为公开无控制器入口，`WebhookRuntime` 显式注入 Store、任务派发器与当前应用原生事件 Dispatcher，并支持自定义 Validator／Profile／Response。保留验签→Profile→模型委托保存→清除旧异常→派发→响应顺序。验证返回 false 时先派发强类型 `InvalidWebhookSignatureEvent`；验证器错误不转成无效签名、Hint 或成功响应。Profile 忽略不保存／派发；任何前置错误阻断成功响应。清除旧异常失败也在派发步骤的异常处理范围中，未提前走成功分支。
- 事件保留请求与配置的 Arc 快照，供应用内可信监听器读取；事件没有自动序列化的诊断属性。包不建立跨应用全局事件容器。`VerifiedWebhook` 私有构造，仅由 Processor 在验证与 Profile 通过后创建；`models::WebhookCall` 是可由自定义适配器构造的数据记录，本身不是验证令牌。自定义验证器的可信政策仍由应用负责。
- 模型入口 `store_webhook`、`headers_to_store`、`header_bag`、`headers`、`find`、`save_exception`、`clear_exception` 委托独立 `storage.rs`；SQL、映射及驱动行为不放进模型，业务顺序由 Processor 协调。`WebhookStore` 是扩展接缝，默认 `DatabaseWebhookStore` 复用既有 Connection／Schema／Query；不另建数据库引擎、通用租约服务或全局注册表。
- Provider 注册内置 `CreateWebhookCallsTable`，三库通过真实 Artisan migrate。接收记录保存 UUID、稳定应用／租户 scope、scope 摘要键、配置名／修订、完整请求 URI、method、原文、选择的重复／非 UTF-8 请求头、unix seconds 创建时间及异常。Scope 最多128字节，读取和更新同时按范围与 ID 限定；SHA256 范围键避免 MySQL 默认大小写不敏感排序规则混同不同范围，读回再比较原始范围。Scope 从应用可信上下文取得，不把调用者任意传值当授权。UUID／unix seconds 是当前 Rust 存储表达，不冒充已实现 Eloquent 父类的所有契约或多节点租约时钟。
- 原文及原始头值用已有锁定 base64 0.22.1 编码到跨驱动 long_text，避免重编码正文与非 UTF-8 JSON 损失；base64 不是加密。数据库访问、保留及加密策略仍需后续运维门禁。原文沿用快照16MiB硬上限／配置默认1MiB，持久 URI 最多8192字节，method 最多32字节；还须由 HTTP 入口在读取流时限量。头预算沿用128值／64KiB。
- 头默认不保存；选择列表大小写不敏感，保留重复值，显式字符串 `*` 才表示全头保存。Rust Builder 的 `with_store_headers(&["*"])` 映射到该字符串；配置数组含 `*` 拒绝，避免错误扩大为保存全部敏感头，这是明确严格适配。配置类型／头名／数量错误均拒绝。
- 派发失败保存固定受限诊断，不落任意第三方错误文本／堆栈，诊断保存成功后原错误仍向调用者返回；若诊断保存也失败，则返回该存储错误，保持固定源码的先保存再重抛顺序。手动 save_exception 支持应用脱敏的 JSON，最多8192字节。内存异常字段只在持久更新成功后改变；MySQL 无变化更新返回0时按同 scope 查询存在性，不误报丢失记录。
- `WebhookJobDispatcher` 是明确命名的**派发接缝 trait**，没有占用上游 `Jobs::ProcessWebhookJob` 的名称，也不是该抽象 Queueable Job 的完整移植。实际夹具实现仅把记录 ID 与可信 scope 交给既有 Artisan Queue，独立 Worker 按 ID 读取原记录，不发送密钥／完整模型，不因当前接收密钥变化或旧时间窗过期重新验签。未自动执行任意供应商业务或写入领域状态。

## 实际验收

- Client：原有6项签名／配置契约、Provider1项与新增 Processor10项通过。覆盖成功调用顺序、原文／URI、强类型无效签名事件及请求／配置载荷、验证器错误、有效忽略、存储失败、派发失败持久诊断、清除异常失败、响应失败、重复／非 UTF-8 头选择及自定义验证器仍受正文预算约束。Provider 另覆盖 store_headers 类型／非法头／数量／数组星号拒绝及字符串星号／空列表允许。
- 三语六段文档实际编译消费者及骨架2项通过。三语接收示例真实编译公开 `DatabaseWebhookStore`、`WebhookRuntime`、`WebhookProcessor`，不把其他类型的同名入口算作已实现。
- `tests/webhooks_receiver_acceptance.py`：PostgreSQL 18.7（180007）、MySQL 8.4.13、SQLite 每库18个真实 HTTP 请求、6个独立 Artisan Worker，缓存前后验证无效签名403／有效忽略200／未知配置404／正文超限413均不落可信记录或任务；二进制／带空格 JSON／Unicode 原文、完整 URL 与百分号编码、method／修订、重复及非 UTF-8 头原样回读。Authorization／签名头未选中，不入库；任务只含 ID／scope，不含正文或密钥。不同 scope（包括只有大小写不同）读不到记录。Worker 用不同的当前密钥仍按接收记录工作，jobs／failed_jobs 最终均0。
- 真实临时改名 webhook_calls 使存储不可用，HTTP 返回500，不创建处理任务；恢复表名后继续验收。派发接缝主动失败时保留原记录、返回500并写固定诊断，不伪造确认。模型 save_exception／clear_exception 与重复清除真实执行，超限 JSON 失败后旧值不变。所有这些测试仅操作隔离数据库，不触及开发或正式数据库。
- 首批50个签名 HTTP 请求／Artisan／包发现／配置缓存／重复发布回归在当前二进制重跑通过。workspace／all-targets＋两 Webhook feature 严格 Clippy、全仓 fmt 检查通过；许可证固定检查通过。
- 三语内容检查和文档站构建通过；12个 Webhook 页面（根／子路径×三语×两包）浏览器检查增加接收章节及公开入口验证。保留已有500kB块大小提示，不调阈值掩盖。

首轮失败如实保留：夹具最初把原生 Request URI 误期望为相对路径，真实 HTTP 实为带 host 的完整 URI，修正预期并固定 APP_URL；PG CLI 的尾换行误作计数差异，读取结果规范化后复测；MySQL 无序 SELECT 选中了上一轮已清空异常的记录，改为按前后 ID 集定位本次记录，不依赖查询隐式顺序。另有初始 Rust insert 调用误传单个 Record、未传 &[Record]，编译器拒绝后按现有 Query 契约修复。相关失败日志保留，不以修正预期掩盖实际运行时问题。

## 未闭合与下一批

继续公开 Route::webhooks／Controller、配置依赖工厂和严格类型解析；不把测试手动 Route::post 当公开包路由宏已交付。源宏确实只接受小写 get／query／post／put／patch／delete；组、前缀、同名／多配置、唯一名称、缓存及专用 CSRF 边界还须实测。

模型的解析 payload／form／附件、get_attachments、prunable、Eloquent 父类／trait／覆写、完整发布迁移及正式可注册业务处理 Job 尚未完成；逐类型清单保持 partial／pending。源码复查发现初始声明扫描遗漏带空格 union 的 storeHeaders，已补清单，不以扫描结果代替逐项审计。

接收去重／冲突、持久信任分类、受限 Hint、Deferred、处理状态／租约／代次、提交与派发之间崩溃补偿、恢复扫描、业务事务幂等、清理／Gate／配额及同一实现的双拓扑仍待 WH-03／WH-06／WH-07。当前每个有效重复请求仍会形成独立接收记录，不宣称可靠处理或恰好一次。创建时间来自当前进程，不作为尚未实现的跨节点租约时钟。

发送 Builder、安全连接／SSRF／DNS固定／TLS／代理、持久投递与重试／重放等继续后续批次。远程多主机、交付平台、生产TLS、真实供应商／账号许可仍未验。最终记录见 accepted-public-entries.log、accepted-runtime.log、accepted-docs.log；本批临时应用／隔离数据库与站点生成物已清理，保留源码、日志及可复用 Cargo 缓存。不推送、不发布、不操作正式环境；持续目标保持进行中。
