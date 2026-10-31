# Session 当前事实

> C04-D1限定子范围已完成适用集成验收，提交以Git为准。契约见 [ADR-082](../decisions/adr-082-session-store-and-flash.md)，唯一执行队列见 [todo](../todo.md)。

## D8 previous 与预检保存边界（限定子范围已验收）

`previous_url` / `previous_route` 返回 `Result<Option<Value>>`，保留缺失和显式 null；setter 沿已有属性状态机。GET 在真实 Route 下游返回后记录 full URL 与可空名称，不按状态码过滤；HEAD、ajax、prefetch 和已确认的 Precognition 不更新。resolver 在记录名称时再次读取，保留可替换 resolver 的可观察调用边界。

已确认的 Precognition 仍 start、执行GC、处理Cookie，仅跳过自动 driver.save 和 previous 更新；客户端同名头不足以跳存。Cookie 组装错误先于driver保存返回，失败路径保持no-store并清理请求队列；不能承诺外部副作用或应用主动save被回滚。授权提取顺序边界见 [HTTP当前事实](http.md)。

根PG18/MySQL8.4实际 Session previous 精确回归各1项已通过，并保留各67项模块HTTP证据。全特性workspace与不同模型静态审查已有结果；Rust1.97全特性全目标check已通过；新生成应用5/2/2、array单进程真实TCP6项已实际通过，不替代TLS或集群范围。具体队列见[todo](../todo.md)，不以基础Store已有验收代替完整Session/Web栈。

## D9 旧输入（限定子范围已验收）

`Session::flash_input(Value)`只接收对象或数组，将`_old_input`纳入原flash状态机；`get_old_input/get_old_input_or/has_old_input`先选择该bag，再以完整字面键优先dot读取。显式null与缺失分开，false/0保留；每次变更只有完整记录预算编码成功才发布，失败不改变属性或flash元数据。

`Request::flash/flash_only/flash_except/flush`复用同一输入仓储与Session；空only保存`[]`、空except保存全输入，flush仅清旧输入bag。`RequestInfo::old/old_or/old_all`无Session时返回缺失、指定默认或空数组，写入则报`session_layer_missing`。手动flash默认不剔密码；唯一模块显式排除敏感字段。multipart文件尚未并入输入bag，不能据此称上传输入完整对应。confirmed预检不自动保存或老化，单纯attempting仍按普通请求保存。

main默认750/31 ignored、全特性1491/105 ignored、公开consumer、根双库各72+old-input1、真实TCP六场景与fresh生成5/2/2已按适用范围验收；[证据](../memory/testing.md#c04-d9-main-组合限定子范围验收通过)不关闭完整HTTP/Session、RedirectResponse或CSRF/FormRequest。

## D10 错误包与请求视图共享（限定子范围已验收，已提交 `784045c`）

`flash_errors` 沿原 Session 属性/flash 状态机保存具版本 envelope，命名自定义包经公开 codec 锁外编码/恢复；`read_errors` 返回独立读取快照。请求middleware在start之后、handler之前恢复视图共享对象，恢复失败按原错误传播并保持私有响应不可缓存，不静默降为空对象。

已有关联对象经 `RequestInfo::put_view_error_bag` 更新命名包时同步持久候选，但不自动延长flash；整包属性替换（含同值替换）断开旧对象关联。缺失fallback修改不写Session；save老化/start加载/invalidate依原属性变化回收关联。回调不持状态锁，重入快速报错；完整记录/对象预算都成功后原子发布，资源关闭仍沿原Session生命周期。视图需要请求入口，不能将用户对象保存在单例Views。

固定Laravel六案身份参考、共享对象/参数回归与HTTP接线已有证据；v9 MySQL8.4.13生成模板认证2项实际通过且隔离库已DROP。D10-C独立公开consumer1项与最终no-session/MySQL8.4.13 TCP已通过，自有库/端口/敏感文件清理完成；旧输入TCP按批次保留，本簇限定子范围已验收并提交 `784045c`，见[当前测试记录](../memory/testing.md#c04-d10-c-请求局部视图错误共享验收中未提交)。不据此关闭CSRF、验证器或整个Session/Web栈。

## 源码与公开边界

R2-X新增 `SessionStoreHandle::store()`，供可信Provider装饰器保留原factory及原store实例；独立Session consumer验证受管理Cookie往返与装饰层计数。直接调用该逃生口绕过请求Session状态、容量和handle层删除检查，不能用于不可信业务输入。main组合与双库回归证据见[testing](../memory/testing.md#r2-qx-main-组合验收通过)。

- `src/session.rs` 导出 `Session`、`Expiry`、`SessionId`、`SessionRecord`、`SessionStore`、写入模式/结果、`SessionStoreGuarantees`以及数据库、显式内存、Cookie客户端快照和可选 Redis store；`session` feature 与 `session-redis` 的实际依赖以 Cargo.toml 为准。
- `Session` 在同请求内共享状态。属性方法同步返回 Result，真正 store I/O 的 start/save/migrate/regenerate/invalidate 为 async；同步锁不跨 await。I/O 期间修改明确拒绝，取消释放 busy，不后台补保存，也不承诺撤销已发生的外部写入。
- ID 为大小写敏感的40位 ASCII 字母数字，构造即有；有效外部 ID 未命中记录时保留。启动补缺失或 null 的 token，非 null 的假值保留。ID/记录 Debug 脱敏；显式字符串与序列化仍可能暴露凭据，不能写入日志。
- 属性读取采用完整字面键优先的 Arr 语义，缺失与 null 分开。flash/now/reflash/keep 使用同一属性树，每次 save 推进老化。JSON 表示不能保留 PHP 数组删除尾项后的隐藏 next-free 历史，具体差异见 ADR。
- 已加载记录仅允许原子 Update；记录删除、过期或重复 start 发现撤销时拒绝以旧认证数据重建。该策略是保留的 OpenRust 安全增强，不冒称 Laravel 缓存 Session 默认 PUT 语义。普通同 ID 并发仍可能最后写入覆盖，不提供默认分布式锁。
- 持久键与 envelope 以原始 ID 字节的 SHA-256 绑定，不额外持久化原始 ID。受管理读取/编码有大小与深度边界；自定义 driver 须遵守公开契约，框架仍校验返回记录。数据库独立到期列是授权截止上界。

## D2 Store 公开 API（限定子范围已验收）

- `remember` 仅缺失或 null 时计算；回调和序列化不持状态锁，可重入，最终写入以最新状态重新校验完整记录预算。回调自身副作用不回滚，不提供跨请求原子计算。
- `push` 对缺失/null/false 从空数组追加，复用同一属性树；`remove` 复用 pull，Rust 的 `None` 与 `Some(Null)` 区分缺失/null。`increment/decrement` 默认步长1，`increment_by/decrement_by` 显式步长，复用 PHP 数值前缀解析和数组合并工具；警告升级错误与不可表示非有限 JSON 不发布新属性。
- `set_name` 发布前经公开 CookieFactory 编码与 Header 校验，真实出站中间件读取新名称；入站仍按配置名，不自动清除旧 Cookie 或改变 ID。`handler` 返回同一实际驱动 Arc，直接驱动调用属于受管理状态机之外的逃生口。
- 最小 session-only helpers 9项、HTTP 5项与独立自定义驱动 consumer check/test（具名测试1项）实际通过，均退出0、源码无漂移；模块显式 session 定向3项通过、43项过滤。唯一模块与生成模板已接入业务调用，修后12项门禁、根真实双库HTTP各54项、三类新生成应用4/2/2及资源收尾已完成；不据此关闭整个 Session。
- PHP 缓存运行时实际输出22条裸表达式与22条固定官方 HandleExceptions 方法路径，来源及限制见 `tests/http/fixtures/laravel-session-d2/`；不代表完整 Laravel Store/Container/logger 启动。详细日志及失败修正记录见[testing](../memory/testing.md)。`set_id/set_handler` 与请求注入的安全生命周期仍待后继实现。

## D3 扩展生命周期（限定子范围已验收）

- `set_id`与`set_handler`即时选择目标、保留bag/started；`handler`改为Result传播锁错误。逻辑driver、请求I/O实例与已加载来源分开记录，显式目标Insert碰撞拒绝、成功后Update；普通同绑定不回退Insert。已观测源撤销拒绝，跨driver检查不提供原子撤销。
- 迁移销毁留本请求有界tombstone；pending跨绑定start及destroy拒绝，不能混入目标认证键或误删未拥有目标。只有系统随机ID允许有限重试，显式重申当前ID也保留指定意图。
- `SessionRequestContext`提供有界Cookie快照和同请求CookieJar，普通driver不绑定，需请求driver返回本请求I/O实例；hook忙碌立即拒绝，middleware在start前注入。不能把上下文存在应用级单例；内置双Cookie Session driver的实现与验收独立列于D4。
- 首稿最低check/Clippy和修后旧10+9+5回归通过；独立审查随后发现四项问题并已交核心修复。最终公开定向36项、模块非空隔离4项与不同模型增量复审已过；最终快照12项门禁全部通过（默认668/全特性1328 passed，忽略29/102不算实服务）。最终外部consumer check/test及真实双库完整模块target各55项已通过（含显式driver非空TCP隔离链）；三类新生成应用4/2/2及资源收尾亦已完成；完整内置Cookie driver与容器extend仍按后继交付，证据见[testing](../memory/testing.md)。

## D4 Cookie driver（限定子范围已验收）

- 固定`CookieSessionHandler`、`SessionManager`、`StartSession`及官方Cookie driver集成测试已审计；四份源码哈希与限定PHP表达式探针的边界见[来源记录](../../../tests/http/fixtures/laravel-session/provenance.json)。该来源核对不等于完整PHP oracle；本簇Rust实际证据另列。
- 契约采用双Cookie、请求绑定、统一Cookie保护和秒级包含截止；公开`ServerSide`/`ClientSideSnapshot`区分后端条件写入与客户端快照。PHP异常弱类型比较的严格差异、浏览器实际容量和不可全局撤销边界见[ADR-082](../decisions/adr-082-session-store-and-flash.md#d4实现契约与明确差异)。
- D4内置Cookie driver限定子范围已验收：final3完整12门禁通过（默认668/全特性1367 passed，29/102 ignored不算实服务），根真实双库加密各62/无加密安全负例各1、新生成plain5/双库认证各2、真实TLS代理链与仓内Chromium HTTP脚本4项及不同模型审查通过；root target清零、五旧consumer不变。本簇不关闭PHP异常弱类型、全局撤销或完整Session/Web能力，详细证据见testing。请求内保存后读取解析同请求队列的完整数据，以载荷摘要防止写后外部覆盖，不能以元数据合成空属性记录。


## D5 GC lottery（限定子范围已验收）

- 当前Spec见[ADR-082 D5](../decisions/adr-082-session-store-and-flash.md#d5-gc-lottery与公开driver契约)：默认`[2,100]`，start成功后业务前调用实际请求绑定driver的公开GC，数据库/Memory清扫已过期记录，Cookie/Redis保noop/TTL职责。
- 绝对expiry、配置变化/显式期限与Laravel last_activity及Array等时刻差异按安全增强公开；取消不后台续跑，已发生删除不回滚，不建立第二存储或分布式请求锁。
- D5请求级GC lottery/公开driver回收与追加expiry索引迁移限定子范围已验收：final4完整12门禁通过（默认668/29 ignored、全特性1374/103 ignored），根真实双库各62、Store/GC各2、Redis Store1/Auth10、新生成plain5/双库认证各2、独立审查及资源收尾通过；忽略项不算实服务，不关闭完整Session。公开`SessionGcMigration`在默认统一Migrator建表后只登记一次，自定义Migrator显式追加。当前可复跑证据与必要差异见[testing](../memory/testing.md#c04-d5-gc-lottery限定子范围已验收)。

## 当前证据与缺口

- Session-only定向基础测试18项通过（0失败/0忽略，390过滤，含名称推导）；公开生命周期测试10项通过，包含认证记录删除后重复 start/save 拒绝；进程内正式Router HTTP测试4项通过（均0失败/0忽略/0过滤），覆盖5xx/未修改保存、取消和缺失装配。它们是对应源码快照的定向证据，不能替代修改后重验、workspace 门禁或真实数据库运行。
- 公开 Store 真实PG18/MySQL8.4回归各通过1项命名fixture（0失败/0忽略/0过滤、sourceguard无漂移、两侧隔离锚库已核不存在），覆盖冲突/撤销、文本顺序/NUL/精确预算及授权截止上界；MySQL重复同记录更新也实际通过。这一专项不替代完整Auth/模块双库HTTP流程。
- Redis8共享Store专项1项命名fixture已实际通过（无失败/忽略/过滤、源码无漂移），覆盖独立连接共享、命名空间隔离、真实TTL、防复活、顺序/NUL及预算。仅使用自有随机命名空间并清理自有ID；不是集群或完整Auth Redis链验收。
- 当前统一实现的PG18/MySQL8.4完整Auth database target各8项及Redis8完整Auth runtime10项已实际通过，均无失败/忽略/过滤、源码无漂移；两侧数据库隔离锚库已核清理。配置最终覆盖8项、公开生命周期10项与进程内HTTP4项已定向重验通过。根PG18/MySQL8.4完整模块HTTP各53项及生成plain4/认证双库各2项另已实际通过。浏览器原生短TTL双库各4、Keycloak真实主流程各1、模块OIDC各8、新Session及默认入口TLS与最近修复后的完整12stage门禁均已实际通过；精确忽略项、候选映射与清理事实见testing，不继承旧tower证据。
- 旧输入Request/Session限定子范围已由D9验收；RedirectResponse、上传文件合并、CSRF、blocking、Session cache与其余driver生态按后继矩阵交付；本文件不将基础状态机视为整个 Session 能力完成。


## T01 测试入口组织（限定试点已验收）

纯行为扩展已作为私有模块挂入 `http_session`，原扩展文件及全部15项断言保持不变，与原26项在session-only/all-features各实跑41项并逐名对应。Session family入口8→7，源文件仍10个；环境子进程、DB/Redis及其他feature门控入口保持独立，不改变Session运行时能力。适用workspace门禁通过，计时可比性与未重跑真实服务的边界见[testing](../memory/testing.md)；入口减少不等于已证整体开发提速。
