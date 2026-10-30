# ADR-082：统一 Session 状态与 flash 生命周期

- 状态：Accepted，C04-D1限定子范围已实现并通过适用验收；完整Session范围仍开放。
- 上游：Laravel 13.39.0 `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`。
- 范围：L025.02 属性、ID/token、flash及现有driver公开扩展；不关闭完整 Session driver 生态、cache/blocking、Old Input、CSRF、redirect 或完整 web 默认组。

## 决策与短 Spec

1. 现 `auth::Session` 只是 tower 重导出；其 lazy load、dirty/非5xx save 和私有平键记录不能承载固定 Store。以唯一 `session` 领域原位重构现请求运行时，移除旧 SessionManagerLayer，不并存第二 bag、ID 或 store。
2. 复用现 PostgreSQL/MySQL SQL、表名解析、Redis namespace/TTL、Container/Scope、最终配置及 C04-C Cookie 保护。`auth` 依赖 `session` feature；`session-redis` 不反向依赖 auth。无新增 crate。
3. `SessionId` 为大小写敏感的40 ASCII字母数字，构造即有ID；无效外部ID立即生成新ID，有效ID读不到记录仍保原ID。随机性复用 getrandom，不以旧 tower ID格式为必要差异。
4. `Session` 是同请求共享 Arc 状态，唯一有序 PHP attributes bag；请求进入立即 start/read，缺或 null `_token` 才生成40字符，已有空串/false/0/其他非null值保留。token getter不强制重写用户值。
5. get/pull/forget完整 literal key优先再dot；星号不展开；存在null不使用default。only为顶层键选择；except沿Arr::forget；replace为put合并。put不做Request的trim/null转换。
6. flash/new/old都位于同一attributes中，flash和now先后次序保留；age仅在每次save执行，显式save也老化。keep空不等reflash，now→reflash可以延长。
7. StartSession handler前读取一次；每个正常返回的Response（含rendered500、未修改、stream响应）在body消费前保存一次。逃逸取消/未得到Response不后台spawn保存。保存失败走安全异常响应，不发送成功Session Cookie或部分响应。
8. 固定顺序 Scope→Cookie解密→Session→handler；响应Session保存/排Cookie→现Cookie合并/加密。原始wire仍供auth严格重复检查。失去Provider的Request.session返回安全500，不隐造内存store。
9. 整体持久记录默认上限20MiB（可配置），编码用受限writer、DB单查询CASE按octet_length有界返回、Redis GETRANGE最多max+1，解析前拒绝超限。复用1024项/64层/20MiB展开key预算；这是授权资源增强，不是Laravel自带限制。自定义driver须遵守传入read limit，可信应用Serialize的转换成本不冒称任意前置有界。
10. 本簇完整验收含属性/flash/ID/5xx/取消/持久化错误、唯一模块、生成应用、默认根TCP、真实PG/MySQL及适用Redis、Auth/OIDC/长连接撤销、公开customstore consumer、完整门禁与产物收尾。

## 公开扩展接口

`openrust::session` 提供：

```rust,ignore
pub struct SessionId(/* private [u8; 40] */);
impl SessionId {
    pub fn generate() -> Result<Self>;
    pub fn parse(value: &str) -> Result<Self>;
    pub fn as_str(&self) -> &str;
}
pub struct SessionRecord {
    pub id: SessionId,
    pub data: serde_json::Value,
    pub expiry_date: time::OffsetDateTime,
}
pub enum SessionWriteMode { Insert, Update }
pub enum SessionWriteOutcome { Written, Conflict, Missing }
pub trait SessionStore: Send + Sync + 'static {
    fn load<'a>(&'a self, id: &'a SessionId, max_bytes: usize, empty_expiry: OffsetDateTime)
        -> BoxFuture<'a, Result<Option<SessionRecord>>>;
    fn write<'a>(&'a self, record: &'a SessionRecord,
        mode: SessionWriteMode, max_bytes: usize)
        -> BoxFuture<'a, Result<SessionWriteOutcome>>;
    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>>;
}
```

`Session`公开同步Result属性读取/put/forget/flash等方法，只有start/save/migrate/regenerate/invalidate执行I/O并async；id/is_started/token为同步Result；typed读取另名避免把Laravel get的null强制变成Rust类型错误。现Auth调用迁到同一句柄，put/get_typed使用同一状态，不保持tower副本。`RequestInfo::session()`与独立extractor共享同一handle，managed Request由Deref复用。无Provider为`session_layer_missing`500。

## 写入与撤销：保留现有安全增强

固定 DatabaseSessionHandler::write：已知存在用UPDATE，缺失用INSERT且冲突UPDATE；CacheBasedSessionHandler::write是普通cache.put。不得把旧NX/XX说成Laravel默认。

主控按ADR-070裁决保留已验收防复活能力：新/未存ID以Insert写入且冲突不覆盖；已成功load的记录只允许原子Update，若被delete则返回Missing且绝不fallback Insert。Insert冲突生成全新随机ID并有界重试（最多3次），保留当前请求的属性；不能串入碰撞记录。Update Missing或Conflict统一安全500 `session_store_write_conflict`，完整替换handler响应、无成功Session Cookie，不新建ID或迁移旧认证attrs；Insert最多尝试3次新CSPRNG ID后同样安全500。handler此前的业务/外部副作用不能由Session保存回滚，调用方不得因该错误盲重试。

这是现有OpenRust授权安全增强，与默认Laravel cache PUT的并发观察不同，公开记录；不加全局串行或第二撤销store。一般同ID请求仍last-writer，非分布式锁。Auth/OIDC delete、失败重试、authorized_until均走同一store；长连接比较请求快照与持久user_id/expiry，不续期。

## 身份绑定与持久编码

DB键继续SHA256，但输入改为原始40 ASCII字节（大小写敏感），不沿用旧i128 little-endian摘要。公开Record.id在内存必须与load请求ID一致。持久JSON envelope包含版本、`id_hash`、attributes、expiry，查键与envelope摘要一致后重建请求ID；不新增持久原始会话凭据。DB保留同表data/expiry列，将data改为PostgreSQL TEXT/MySQL LONGTEXT保存受限JSON原文：原生JSON会重排键、改写编码字节，JSONB还拒绝NUL，不能兑现顺序、NUL及精确持久预算。无历史兼容义务，仅新建隔离DB/新应用，不就地修改共享用户DB。独立SessionMigration与包含它的AuthMigration不得同时注册；两者复用唯一DDL。DB继续使用原expiry列，Redis使用原namespace+摘要键；不新建表/存储内核。

坏序列化与store不可用区分：受管理payload超限413 `session_limits_exceeded`；持久身份/envelope不一致安全500 `session_store_data_invalid`；IO失败安全500 `session_store_unavailable`，不回显ID/数据/连接信息。损坏JSON语法沿源为空array匿名状态；有效JSON但envelope字段缺/版本/摘要不符安全500；不能把后端异常吞成空bag。custom store的max_bytes是公开驱动契约，框架无法强制其内部的分配前限制；返回后仍校验record ID/expiry/序列化大小和深度。

## D2：基础Store方法与公开扩展（契约与验收边界）

- `remember<T: Serialize>(key, FnOnce() -> Result<T>) -> Result<Value>`：非null命中直接返回；缺失/null才计算并写回，false/0/空串保留。回调及序列化不持锁，允许同句柄重入；最终写入覆盖同键、保留回调写入其他键。已发生的回调/序列化副作用不回滚，不承诺跨请求或分布式原子remember。
- `push<T: Serialize>(key, value) -> Result<&Self>` 复用现Attributes追加；缺失/null/false可追加，其他非数组类型失败。JSON对象按现PHP关联数组表示，追加整数键与稀疏规则复用同内核；已公开的隐藏next-free历史差异仍适用。
- `increment/decrement(key) -> Result<Value>` 默认amount=1；`increment_by/decrement_by<T: Serialize>(key, amount) -> Result<Value>` 对应上游动态amount。复用scalars数值词法和data左并集，整数溢出转有限double，decrement先amount乘-1再相加；数组加数组为左并集，混合非法类型失败。
- 采用固定Laravel HandleExceptions错误边界：leading-numeric尾垃圾产生的E_WARNING视错误且不写入；false追加的deprecation不升级异常。裸PHP表达式探针不等于完整框架oracle，分别记录来源与执行方式。不新建PHP式全局警告系统；非有限JSON结果返回错误、原值不变，此表示限制不冒称PHP存储INF等价。
- `remove(key) -> Result<Option<Value>>` 复用pull；Rust的None/Some(Null)保留现公开缺失区分，而上游PHP二者均返回null，按必要类型适配记录。
- 属性操作基于最新状态克隆、校验完整record预算后一次发布；发布前重验busy/closed/revoked。remember缺失路径先预检可变性，回调后仍需复检；操作本身失败不发布部分变更。
- `set_name(name) -> Result<&Self>` 复用CookieFactory校验和状态门控，响应中间件必须用Session.name实际输出。请求启动仍按原配置读取Cookie；改名不换ID、不自动删除旧Cookie。
- `handler() -> Arc<dyn SessionStore>` 返回同一后端引用，属于公开driver逃生口；直接调用driver不具有受管理Session的状态机与跨操作保证，不返回另一store或新bag。
- `set_id`、`set_handler`、CookieSessionHandler式请求注入等运行期安全生命周期仍是具体未交付项；不能新增绕existing/revoked约束的无保护setter，也不能按Rust惯用将适用上游能力删除。D2基础方法通过不关闭完整L025。

验收链为固定Store/HandleExceptions源码→明确标记的PHP8.5.0运算与错误处理探针→Rust方法/重入/预算/异常→唯一modules/example与生成helper→真实双库持久HTTP及外部driver；适用workspace门禁在本簇改码后实际执行。

## D3：扩展生命周期（契约与验收边界）

- 固定Store.php:701–714的setId只更换ID，合法值为40位ASCII字母数字，非法/null生成随机值；不读取目标记录或清除现有属性。setHandler:854–857只替换并返回驱动，不隐式start。
- 固定StartSession.php:144–149先从配置Cookie名选择ID；130–136向需要的handler注入请求，再start。普通handler不需要请求，Store.php:864–879仅识别CookieSessionHandler。
- 固定CookieSessionHandler.php:87–129从以ID为名的第二枚Cookie读带expires的data，写入/删除经本请求Cookie队列；注入不能是假回调，也不能把可变请求保存在应用级单例store。
- 当前Rust的Inner.store为不可变Arc，State.existing仅绑定当前ID与原驱动；直接改指针或无条件清existing不足以定义后续受管理写入。
- 保留setter的受信任扩展语义与现有属性，明确目标记录归属、busy/closed/revoked、失败与取消；普通被动撤销保护和开发者显式转移不是同一保证。不以两次load声称跨驱动原子撤销。
- 选定公开API：`set_id(Option<&str>)->Result<&Self>`、`set_handler(Arc<dyn SessionStore>)->Result<Arc<dyn SessionStore>>`返回传入的Arc；只有新后端才成为配置驱动，当前配置/请求实例Arc回传均为no-op且请求实例不得长期保存；`handler()->Result<Arc<dyn SessionStore>>`返回当前实际I/O驱动，改为Result以传播锁错误。`handler_needs_request()->Result<bool>`与async `set_request_on_handler(SessionRequestContext)->Result<&Self>`补齐请求绑定。
- State保留逻辑后端身份、实际请求实例、原已持久Binding和写入意图。setter立即可见，bag/started不变；同绑定不清证明，连续未保存替换保留原origin，切回origin恢复Update。显式新目标只Insert，碰撞报错且不随机改指定ID、不永久标revoked；仅系统生成ID可按现上限随机重试。成功start/save更新实际origin，后续按Update保存。
- 同ID且显式传入合法当前ID时，未持久的SystemRandomCreate升级为ExplicitCreate，不能因同值提前返回而随机改写选择。set_handler传回当前配置Arc或当前请求实例Arc均为同逻辑后端no-op、保留证明，不把请求实例提升为长期配置；这是逻辑身份与请求实例分离的Rust适配。
- 显式转移前对未主动销毁origin作有界活性检查，已观测missing/expired则拒绝并标revoked；这不是跨驱动原子撤销。显式目标选择与migrate的可信复制语义，不等于被动同绑定持久化。存在已加载origin的pending跨绑定start拒绝，避免源/目标认证属性混合；未加载的初始setId→start正常加载。
- 已加载origin且pending跨绑定时，migrate(true)/regenerate(true)/invalidate在任何delete前以session_rebind_pending拒绝，不删除未拥有目标或擅改origin；切回origin或完成授权保存后再执行。此安全增强与上游直接删除当前handler目标的行为分别记录。
- migrate(true)/invalidate成功删除旧ID后留下本请求tombstone，最多1024个；容量在delete前检查，已销毁ID跨store也保守禁止回退，作为公开安全增强。主动销毁origin不因其预期缺失而阻止本次授权迁移。setter/hook拒绝busy/closed/revoked；驱动回调和旧Arc最后释放不持属性锁，I/O取消只释放门控，不承诺撤回外部效果。
- `SessionRequestContext`拥有有界Cookie快照和本请求CookieJar，不持Request/body/Session/Scope，Debug脱敏；公开`new(Value,CookieJar)->Result<Self>`、`cookie(&str)->Option<Value>`（与公开RequestInfo的object/array数字键精确名称读取一致，不展开点路径）、`cookie_jar()->&CookieJar`。复用既有Cookie/Data资源限额，不让外部driver依赖私有构造。
- SessionStore新增默认`needs_request()->bool=false`与`bind_request(context)->Result<Option<Arc<dyn SessionStore>>>=Ok(None)`；普通driver不调用bind，hook忙碌时即时拒绝而不排队等待；需请求的driver必须返回本请求实例且保持同一逻辑backend。注入仅改实际I/O Arc，不清existing/provenance；显式set_handler才改变逻辑身份，需请求且未绑定时拒绝I/O。第三方返回同backend是公开driver契约，框架不能反射证明任意实现。
- middleware在start前完成请求绑定；真实HTTP自定义driver必须实际消费Cookie并经本请求队列输出，证明两个请求不串scope及替换后重新绑定。内置Cookie driver双Cookie、加密视图、expires/write/destroy等完整行为列紧邻D4，D3 hook不冒称其通过。
- 示例扩展由模块自有wrapper和显式fixture Application/Provider选择已知Db/Memory底层并经公开接口装配，正常宿主默认driver不被模块自动覆盖；不冒称可装饰任意已有容器factory。未交付的通用Container extend仍按既有矩阵后继，D3不另造专属注册内核。
- 本簇已完成适用验收；沿source→API→公开consumer/唯一模块/生成→双库真实HTTP→适用完整门禁交付，不新增跨库分布式事务或第二套Session内核。

D3限定子范围已通过最终12项门禁、根真实双库完整target各55项、三类新生成应用4/2/2及不同模型审查；资源收尾已完成，证据见[testing](../memory/testing.md)。完整Cookie driver与Container extend仍保留对应后继，不关闭整个Session/Web范围。

## D4：内置Cookie driver（契约已定，实现中）

- 固定SessionManager用lifetime/expire_on_close构造CookieSessionHandler；配置名Cookie保存ID，第二枚以该ID为名的Cookie保存JSON data/expires。输入取Cookie层解密视图，输出经同请求队列再由外层保护，不新增并行Cookie内核。
- CookieSessionHandler.read仅在有效JSON数组且expires已设、currentTime() <= expires时读取data；expiry包含整秒边界。write嵌入当前秒加lifetime秒数，expire_on_close只改变浏览器Cookie有效期为会话，不取消嵌入期限。异常字段已按固定源码和限定PHP读取表达式探针审计；正常写入格式的对应与异常弱类型差异分别验收，不把所有异常无条件空会话冒称等价。
- 数据Cookie沿CookieJar默认HttpOnly=true、Partitioned=false；ID Cookie则按Session配置传http_only/partitioned。path/domain/secure/same_site共用最终CookieFactory/可信传输，两个Cookie不能简单复制所有Options。固定上游Integration/Session/CookieSessionHandlerTest验证expire_on_close与请求Secure继承。
- Cookie driver没有服务器共享记录，旧受保护双Cookie副本可在嵌入期限内重放；delete只排当前响应forget，Insert/Update不能发现另一客户端的撤销或全局碰撞。必须公开客户端快照保证，不暗加DB/Redis，也不降低默认服务器driver原子Update/Missing契约。
- 普通Auth登录/当前请求登出须沿bound Session实际工作；现Auth authorized_until读取singleton handler、OIDC远程delete后标session_deleted_at的路径必须明确能力边界并安全失败，不能返回假成功或声称跨客户端撤销。公开保证分类与接线见下述契约；这些入口未验收前不关闭相应行为。
- 复用D3逻辑后端/请求实例、取消和tombstone；写入预算、秒边界、双客户端重放限制、加密与历史密钥、真实HTTP/浏览器及生成/双库回归随簇验收，不把D3 hook证据当完整driver通过。

### D4实现契约与明确差异

- 公开`SessionStoreGuarantees::{ServerSide, ClientSideSnapshot}`，`SessionStore::guarantees`默认前者。ServerSide仅表示当前后端的条件写入/删除契约，不代表Memory跨节点共享；Cookie声明客户端快照，本请求观察不能外推全局冲突或撤销。
- `CookieSessionStore::new(lifetime_minutes: u64, expire_on_close: bool)`返回`Result`，校验正分钟与秒数/日期溢出。Provider以`driver="cookie"`选择，复用统一请求绑定；无请求时安全错误，不暗存全局Request。
- 正常第二Cookie为`{data: JSON属性序列化字符串, expires: 整数秒}`。Rust属性表示使用serde JSON，不承诺PHP serialize字节兼容。写入截止为当前秒加lifetime与record截止秒的较小值；读取转换为checked的下一秒排他截止，包含上游到期整秒。显式`set_expiry`在此driver采用秒精度，可能较纳秒期限延长不足一秒，此项为公开精度适配。
- 坏JSON、非object根、缺失/null的expires或已过期值按miss处理；非整数expires、有效未过期对象中缺失/null/非字符串data以`session_cookie_payload_invalid`安全错误拒绝。整数溢出也拒绝。PHP可接受浮点或字符串expires以及部分data弱转换；严格类型错误是安全增强差异，尚不具备这些异常输入的严格1:1等价证据，不将正常双Cookie通过扩大为全部异常语义完成。
- 请求实例只维护有界ID/截止/已排载荷SHA-256摘要及Written或Deleted元数据（至多1024），不另存属性bag；写入与删除先校验/准备，Cookie队列成功后才提交元数据。仅本driver已成功写入的ID，后续读取才解析相同名称/路径的已排队完整输出；其他ID始终读取入站快照，不能把任意预排响应Cookie当入站。自身已排载荷读取/续写核对摘要，外部覆盖安全拒绝，外部forget视miss；不把摘要当属性缓存。外部预排同名同路径数据值使写入返回Conflict，预排forget使写入返回Missing，属于防覆盖的安全增强。取消不后台补写，外部已发生副作用不能声称回滚。
- 沿既有单头64KiB、队列128项及最终保护后预算，不新增全局4KiB硬限制。真实浏览器通常有约4KiB单Cookie实践边界；大于浏览器限制的成功HTTP头构造不代表浏览器可存，浏览器验收使用小payload并单列限制。
- 普通绑定请求的Auth登录/登出沿现Session轮换；无请求远程`SessionStoreHandle.delete`对客户端快照返回`session_store_guarantee_unsupported`，不能标记远程删除成功。需要持续服务器撤销保证的`authorized_until`对Cookie同样明确拒绝，服务器驱动读取实际绑定handler；不因编译Auth feature就拒绝全部Cookie装配。
- Session认证需要独立的完整性保证：`SessionStore::supports_session_authentication(cookie_name)`默认仅ServerSide为true，ClientSideSnapshot为false；第三方显式声明支持时须自行验证载荷来源与完整性。内置Cookie请求实例只接受真实Cookie中间件的私有Encrypted保护事实，公开`SessionRequestContext::new`不能凭原始快照制造此事实。ID名称或任一合法40位数据ID被精确except排除时不支持认证；拒绝任一合法数据ID排除是轮换安全的保守约束。当前except是精确字面量，不将`*`误当glob。
- Auth实际选择Session凭据、默认DB认证直接入口及登录/登出限流或密码验证等副作用之前检查此保证；认证属性的最终save按实际handler及当前名称再次检查，防set_name/set_handler在入口后降级保护，普通非认证属性不因此禁止既有转移；失败返回中文脱敏的500 `session_cookie_protection_required`。JWT独立路径不受影响，普通显式plain Cookie业务仍允许，不以编译auth feature全局拒绝Cookie，也不谎称所有不安全组合启动即失败。真实Cookie Auth正例必须走受保护wire，plain与保护排除的伪造负例须被拒绝；未验收前该安全子项保持开放。
- Cookie/Session机制自身失败响应统一设置`Cache-Control: no-store`与`Pragma: no-cache`，覆盖请求准备、保存/ID Cookie生成及外层Cookie保护/预算失败的响应替换。复用内部响应helper，不按示例路径特判，不复制被替换响应的Set-Cookie、Content-Length或正文，普通成功响应不加此策略。确定性Router回归与真实账户负例分别验收，不把静态缺口冒称某次服务失败的动态根因。
- 核心实现、叶测试/模块/生成调用与不同模型只读审查分离；Cargo、清单/锁、服务及构建资源由单一集成人协调。当前仅contract固定，全部实现与验收仍开放。

## 配置与后继边界

现SessionConfig默认secure=true、ttl_secs=两周未被C04-C称为Laravel默认。本簇整改lifetime为120分钟、nullable secure供实际TLS推导、expire_on_close=false、http_only=true、same_site=lax、partitioned=false并保Cookie最终配置优先级；cookie名派生app.name与SESSION_*环境映射须按固定源同步，不默保旧值冒称完成。数据库默认后端复用现已注册Db；不暗连用户数据库。

Session区段类型与未知字段由既有schema即时检查；类型正确的语义中间值允许被后续boot/overlay修复。语义只在既有validate_at_finalize阶段读取最终冻结配置校验，再解析单例/层，不在with_cookie_config提前拒绝；最终错误保留configuration_error与session.<field>键，容器不把它包装成服务初始化错误。CookieFactory只校验同一最终区段中的Cookie属性，不抢校验Session driver/lifetime，显式with_factory仍独立优先。

driver选择/全驱动生态、GC lottery、previous URL/route、blocking锁、precognitive特殊分支、OldInput/CSRF/redirect为明确后继，矩阵不将本簇扩为完整StartSession实现。

## 来源与证据

固定源码与自造合成case来源清单见[仓内Session参考资料](../../../tests/http/fixtures/laravel-session/provenance.json)。这些38项是source_candidate，不是已执行的PHP Store差分golden；各项Rust对应测试及部分覆盖边界分别记录，默认Cargo不依赖/tmp或PHP运行时。DatabaseSessionHandler与CacheBasedSessionHandler的write固定官方源另已核。实际Rust/双库/网络与生成门禁已按限定子范围执行，结果与失败记录见memory/testing；不将其外推为全部38候选或完整Session等价。

load接收运行时按最终lifetime计算的empty_expiry，专供损坏/空payload匿名恢复，不在driver再硬编码默认TTL；Redis同一Lua读取GETRANGE(max+1)+PTTL区分缺失/空值，TTL/envelope均为截止上界。DB写入envelope/列共同归一微秒；读取以列截止为上界，过期行条件删除后按miss保原合法ID，Update仍有原子expiry谓词防旧属性复活。

Session属性使用JSON值表示；业务手动删除flash元数据尾项后追加按当前可见整数键计算，不保留PHP数组已删除键的隐藏next-free历史。例new=[a,b]删除索引1再flash x：PHP隐藏索引为0/2，JSON表示为连续0/1。正常框架flash生命周期与array_diff/unique的稀疏键保留。该必要表示适配须独立测试，不豁免公开all/only/except顺序。

同步属性锁不跨await；I/O门控以受管busy标记防止保存快照期间同步写入，冲突返回session_operation_in_progress而不阻塞运行线程或丢更改。I/O future取消时RAII清busy，不spawn后台保存；读操作返回当前快照，只有显式await start完成后才保证加载完成。保存前已执行的flash老化与上游一致，不因write错误倒退；handler副作用不回滚。

属性方法同步返回Result，真正store I/O为async；同请求std状态锁不跨await。I/O持有RAII busy标记及快照，同步变更在busy期间安全拒绝，取消只释放busy而不补偿已发出的外部write、不后台保存。默认cookie名称按最终app.name经固定portable-ascii 2.0.2 en轮序及slug推导，加_session；显式cookie字段优先。字符串各阶段20MiB是配置资源增强；父层统一验证最终Cookie/header预算。

RequestInfo::session返回克隆的同Arc Session句柄（非属性副本），便于持有或移入async；缺装配统一安全500 session_layer_missing。

D2基础方法子范围已通过当前12项门禁、真实双库模块HTTP各54项、生成plain4/认证双库各2项与不同模型审查，证据见[testing](../memory/testing.md)。这不关闭上述ID/handler替换生命周期及完整Session范围。

## D5 GC lottery与公开driver契约

固定Laravel `StartSession` 在start成功后、业务前以`config/session.php`默认`[2,100]`抽签，调用实际handler的`gc(lifetime秒)`。公开契约为`SessionStore::gc(i64) -> BoxFuture<Result<u64>>`，默认no-op返回0，保第三方覆盖与真实请求绑定；配置`lottery: [u32;2]`校验分母非零且分子不大于分母，无偏抽签、0与全中确定性执行。

DB采用单条条件DELETE清理已持久化绝对expiry，Memory同锁移除已有期限到期记录并计数，Cookie和Redis与固定上游handler保持no-op/TTL职责。不引入第二存储或后台清扫进程。Laravel DB按last_activity与当前lifetime回溯、Array严格小于；本框架的持久绝对expiry与`<=`授权边界优先，避免配置改变/显式Expiry误删活会话，这是现有授权期限与防复活安全增强的必要差异，不能声称异常边界完全一致。缺少last_activity不能用expiry反推伪造上游行为。

请求级GC使用expiry索引，追加公开`SessionGcMigration`，不修改已具名的旧建表迁移。默认统一Migrator在Session-only及Auth建表之后各登记一次，自定义Migrator显式追加；索引名来自实际物理表名摘要，防前缀/映射冲突并满足双库标识符长度。需真实验证新库及已有表追加路径，不假设跨库通用`CREATE INDEX IF NOT EXISTS`，同名异列不静默通过，升级不删除用户记录。

GC失败走现有prepare错误清Cookie/no-store/关闭路径，业务不执行、不save；取消不后台继续，已完成外部DELETE不回滚。删除和续期竞争依靠数据库条件操作与既有Update防复活；不提供默认分布式请求锁。验收覆盖公开driver、确定性lottery、错误/取消、唯一模块与新生成、真实PG18/MySQL8.4及Redis；本簇限定子范围已通过final4与实际专项，证据及未等价边界见[testing](../memory/testing.md#c04-d5-gc-lottery限定子范围已验收)。

固定来源：framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`的`Middleware/StartSession.php:109–129,169–189`、`config/session.php:117`、`DatabaseSessionHandler.php:299–301`及真实`tests/Integration/Session/DatabaseSessionHandlerTest.php:57–75`；`ArraySessionHandler.php:133–146`实际清扫，`CacheBasedSessionHandler.php:112–115`与`CookieSessionHandler.php:146–149`返回0。此前不存在的StartSession测试路径不充当有效官方测试证据。

D5固定源码审计SHA256（均为上述framework ref，源码核对不等于执行官方PHP测试）：

| 上游路径 | SHA256 |
| --- | --- |
| `src/Illuminate/Session/Middleware/StartSession.php` | `c259cd8954a107a080e4787caec3babc7c6fc710a9ba3e0310533c2662199ac7` |
| `config/session.php` | `12ec2d64eaffa4a9378a7f0d49f4ffd117f798b6b60a6efd3be067e4b01f1f95` |
| `src/Illuminate/Session/DatabaseSessionHandler.php` | `d8a2edbac91c7ab70e705c5d2ace67b5833fb58336d42c47d85a4d59e2116079` |
| `tests/Integration/Session/DatabaseSessionHandlerTest.php` | `60f9219ae224271850523d80ce673b9f478f13207c5e896ada77a08a903a371c` |
| `src/Illuminate/Session/ArraySessionHandler.php` | `e01d016e8470b0d196caa600bc72b1fe51e41be426c3f422cb38f4ba666ee62c` |
| `src/Illuminate/Session/CacheBasedSessionHandler.php` | `f60a7447aeab6857526516514d096aadce05c2ea960c9f082c0802d0337f6308` |
| `src/Illuminate/Session/CookieSessionHandler.php` | `fb4c6fe371bda398abcf62f30128021382357fb8ee7f16c88469c571c1ec4102` |
