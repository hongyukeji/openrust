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
