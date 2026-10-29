# Session 使用指南

Session 为同一请求提供共享属性与 flash，并在响应发送前持久保存。本文面向使用 `openrust::session` 的应用开发者；业务示例位于唯一的 [`modules/example`](../../modules/example/http/controllers/session.rs)。

## 启用与存储

编译 `session` feature，并在应用 Provider 清单中登记 `SessionServiceProvider`。启用 `auth` 已包含 `session` 编译能力，但仍需要应用明确装配 Provider。认证入口与通用 Session 共用同一 Provider、句柄和存储，不能再登记第二套 Session Provider。

默认 driver 是 `database`。它使用应用已有 `Db` 和逻辑 `sessions` 表，不另建连接池。选择 PostgreSQL 或 MySQL 时启用对应的 `db-postgres` / `db-mysql`，登记 `DatabaseServiceProvider`，配置数据库并运行迁移。下面是已经配置加密密钥的宿主的 Provider 片段；`EncryptionServiceProvider` 需要 `encryption` feature：

```rust
use openrust::core::ProviderBuilder;
use openrust::database::DatabaseServiceProvider;
use openrust::http::{CookieServiceProvider, HttpServiceProvider};
use openrust::security::EncryptionServiceProvider;
use openrust::session::SessionServiceProvider;

fn register(builder: &mut ProviderBuilder) {
    builder
        .provider(DatabaseServiceProvider)
        .provider(HttpServiceProvider)
        .provider(EncryptionServiceProvider)
        .provider(CookieServiceProvider::new())
        .provider(SessionServiceProvider::new());
}
```

不需要 Cookie 加密的明确使用场景可以选择 `CookieServiceProvider::plain()`；它不提供 Cookie 机密性或完整性保护，不能当作 Laravel 默认 Web 组。密钥配置见[安全能力](../security/README.md)，请求 Cookie 规则见 [HTTP 指南](../http/README.md#cookie-读取队列与响应)。

### 迁移只选一种入口

- 不使用认证迁移时，在应用 Migrator 中登记公开 `SessionMigration`。
- 已登记 `AuthMigration` 时，它内部已经复用 `SessionMigration`，不要再次登记独立 `SessionMigration`。重复迁移版本的回滚可能删除另一版本仍依赖的表。

迁移使用应用的表名前缀与完整名称映射。表保存身份摘要、有界记录及微秒精度截止时间；不要把会话 ID 当作普通日志字段。

### 显式内存与自定义 store

内存 store 适用于测试或明确接受进程内状态的应用；不会在数据库缺失、连接失败或 driver 名称不支持时自动启用：

```rust
use openrust::config::SessionConfig;
use openrust::session::{MemorySessionStore, SessionServiceProvider};

let provider = SessionServiceProvider::with_store(MemorySessionStore::default())
    .with_cookie_config(SessionConfig {
        driver: Some("array".into()),
        secure: Some(false),
        ..SessionConfig::default()
    });
```

也可将最终配置的 `driver` 明确设为 `array`。内存 store 不共享到其他进程，进程重启后状态丢失；单实例最多保留 1024 条记录，容量不足会报错，不悄悄逐出仍有效的会话。

公开扩展入口是 `SessionStore`，通过 `SessionServiceProvider::with_store(store)` 安装，优先于配置中的 driver 选择。实现者需要遵守[存储契约](store.rs)：

| 方法 | 要求 |
| --- | --- |
| `load(id, max_bytes, empty_expiry)` | 在复制或解码完整载荷前限制字节；返回指定身份的记录。`empty_expiry` 供损坏载荷的空属性回退使用。 |
| `write(record, mode, max_bytes)` | 原子区分 `Insert` 与 `Update`；返回 `Written`、`Conflict` 或 `Missing`。 |
| `delete(id)` | 重复删除成功；不能因此重建记录。 |

自定义实现不得把 `Update` 的 `Missing` 改为插入。错误不得包含原始属性、ID 或服务凭据。框架仍会校验自定义 store 返回的身份、属性和整体记录上限。可选 Redis 实现需要 `session-redis` 并通过公开 `RedisSessionStore::connect` 和 `with_store` 明确安装；仅写一个 driver 名称不会自动建立服务连接。

## 最终配置与 Cookie

`SessionServiceProvider::new()` 读取最终 `session` 区段；`with_cookie_config(SessionConfig)` 在该 Provider 的 boot 中覆盖此区段，更晚的配置覆盖仍按正常顺序生效。

```toml
[session]
driver = "database"
path = "/"
http_only = true
same_site = "lax"
lifetime = 120
expire_on_close = false
partitioned = false
max_bytes = 20971520
```

| 字段 | 默认与行为 |
| --- | --- |
| `driver` | `Some("database")`。Rust 配置中的 `None` 明确禁用请求 Session 层，不降级内存。 |
| `cookie` | `None`：按最终 `app.name` 计算 `Str::slug(name, '_') + "_session"`；显式 `Some(name)` 优先。 |
| `path` / `domain` | `/` / `None`；`domain=None` 不发送 Domain，使用浏览器的 host-only Cookie。 |
| `secure` | `None`：按可信请求传输推导；`Some(true/false)` 是显式设置。 |
| `http_only` / `same_site` | `true` / `Some("lax")`；`same_site=None` 明确省略属性，省略配置键则仍使用默认 Lax。 |
| `lifetime` | 120 分钟，无活动期限；每次保存更新截止，必须为正且可安全换算。 |
| `expire_on_close` | `false`；设为 true 时发送浏览器会话 Cookie，持久记录仍受 `lifetime` 限制。 |
| `partitioned` | `false`；启用时发送 Partitioned 标记，应用还需正确设置 Secure 并满足浏览器要求。 |
| `max_bytes` | 完整持久记录默认 20 MiB，包含属性与记录元数据；只能调低，不能超过硬上限。 |

TOML 没有 null 字面量。需要把可选字段明确设为 `None` 时，可以使用 `SessionConfig` 或支持 null 的配置源；不要用空字符串代替。

默认名称使用固定英文转写规则，涵盖重音、希腊及西里尔等映射；不支持的字符按源规则删除。空 slug 的结果是 `_session`，不会换成品牌常量。推导使用本项目明确固定的 portable-ascii 2.0.2 映射；其版本来源与许可见[实现说明](name.rs)。名称推导对输入、中间与输出字符串分别限制为 20 MiB；最终 Cookie 头仍统一检查 64 KiB 上限，超限不会截断名称。

`secure=None` 不相信任意客户端伪造的 `X-Forwarded-Proto`。TLS 终止代理必须按应用可信代理配置接入；直连 TLS 与受信代理才参与安全传输判断。`Secure` 只约束 Cookie 发送条件，不加密 Cookie；反之 Cookie 加密也不代替 TLS。

Session、认证与 Cookie 加密是独立能力。CLI 的 `--auth` 不要求同时使用 `--encryption`；启用加密的正式 Cookie Provider 使用当前密钥保护响应，并可按配置使用历史密钥读取旧 Cookie。显式 plain 模式下 Session Cookie 仍可能是原始 ID，不应将它称为受保护值。业务代码通过 Session API 读属性，不自行解密请求 Cookie。

## 请求共享句柄与属性

受管理请求进入处理器前已经 `start().await`，处理器正常返回后自动 `save().await`。属性方法同步执行，不需要 `.await`；存储 I/O、ID 迁移等方法是异步的。普通处理器不要重复启动或保存，因为每次保存都会推进 flash 年龄。

```rust
use openrust::http::{Json, Request};
use openrust::session::Session;
use serde_json::{Value, json};

async fn store(session: Session, request: Request) -> openrust::Result<Json<Value>> {
    let preference = request.string("preference")?.into_string();
    session.put("example.preference", preference)?;
    session.flash("example.notice", "已保存")?;
    Ok(Json(json!({"saved": true})))
}
```

正文提取器 `Request` 放在参数最后。正式 Request 默认执行 trim 与空字符串转 null；Session 自己的 `put`、`flash`、`now` 不再做这类规范化，会保留传入字符串的空白与空串。示例模块对自身偏好另有限定类型与 256 UTF-8 字节业务限制，不能将它误认为全局 Session 限制。

`Session` 提取器、`RequestInfo::session()`、`Request::session()` 与 `Inject<Session>` 指向同一请求的属性状态。`RequestInfo::session()` 返回可克隆的 `Session` 值，不是借用引用；克隆不会创建另一个会话。没有安装或启用 Session 层时，`has_session()` 为 false，访问 Session 返回 `session_layer_missing`，语义 HTTP 状态为 500；不会隐式创建内存会话。实际响应状态遵循应用统一 HTTP 状态策略。

### null、缺失与键路径

```rust
use serde_json::{Value, json};
use openrust::session::Session;

fn inspect(session: &Session) -> openrust::Result<()> {
    session.put("example.null", Value::Null)?;
    let existing_null = session.get("example.null")?;       // Some(Null)
    let missing = session.get("example.absent")?;          // None
    let kept_null = session.get_or("example.null", json!("fallback"))?; // Null
    let typed = session.get_typed::<String>("example.preference")?;
    Ok(())
}
```

| 操作 | 语义 |
| --- | --- |
| `get` / `get_or` / `get_or_else` | 完整顶层字面键优先，其次 dot 路径；只有缺失才使用默认值，null 不使用默认。惰性默认回调不持有属性锁。 |
| `exists(&[...])` | 所有键存在，包括 null；空列表为 true。 |
| `has(&[...])` / `has_any(&[...])` | 所有键 / 至少一个键存在且非 null；false、0、空串仍算有值。 |
| `put` / `put_many` / `replace` | dot 路径写入；批量按顺序执行并原子替换。`replace` 是合并写入，不清空其他键。 |
| `pull` / `pull_or` | 返回原值并删除；null 与缺失保持区分。 |
| `forget` / `flush` | 删除指定字面键或 dot 路径 / 清空全部属性（包含 token 和 flash 元数据）。 |
| `only` / `except` | 选择顶层字面键 / 从独立快照按删除规则排除；不修改当前属性。 |

这里不展开 `*` 通配符。属性使用 JSON/serde 表示；`all()` 和读出的值是独立快照，空属性根表示为 `[]`。参数或树超限时，批量变更保持原子性。不要直接管理框架 `_flash` 元数据：手动删除尾项后再追加按当前可见整数键计算，不保存 PHP 数组隐藏的 next-free 历史；普通 flash 与稀疏删除语义见[差异说明](../../.agents/project/decisions/adr-082-session-store-and-flash.md)。

## flash 的保存时序

| 请求 | 操作与可读结果 |
| --- | --- |
| A | `flash("example.notice", "已保存")` 后立即可读；A 保存后为下一请求保留。 |
| B | 可读该提示；若不调用 `keep` 或 `reflash`，B 保存时删除。 |
| C | 提示已经缺失。 |

`now` 也立即可读，但本次保存就删除。`keep(&["example.notice"])` 只延长指定键；空列表不等于保留全部。`reflash()` 延长所有旧 flash，也会影响其他业务的提示，业务模块应优先保留自己的键。

每次显式 `save().await` 都老化一次，同一请求保存两次可能提前删除 flash；`save` 不是幂等操作。未修改的请求和处理器返回 500 的请求也会保存。Provider 自动保存时，不要为了“确保保存”在处理器末尾重复调用 `save`。

## ID、token 与撤销

构造 `Session::new(id, store, expiry)` 时 ID 已立即可见；ID 为大小写敏感的 40 位 ASCII 字母数字。合法 ID 没有持久记录时保留此 ID，属性重新开始。`start` 为缺失或 null 的 `_token` 生成同样长度与字符集的随机 token；已有 false、0、空串等非 null 值按 Store 规则保留。token 的存在不代表请求已经通过 CSRF 验证。

| 操作 | ID / token / 属性 |
| --- | --- |
| `migrate(destroy).await` | 更换 ID，保留 token 与属性；destroy=true 时先撤销旧记录。 |
| `regenerate(destroy).await` | 更换 ID 并再生 token，保留其他属性。 |
| `invalidate().await` | 清空属性、撤销旧 ID 并换 ID；不立即再生 token。需要立即使用时显式 `regenerate_token()`，或由后续 `start` 生成。 |
| `SessionStoreHandle::delete(id).await` | 通过同一个 store 撤销指定 ID，不写出凭据。 |

读取过的记录被删除或过期后，迟到保存必须得到 `Missing`，不能变成一次新插入来恢复旧认证属性；当前句柄进入撤销状态，后续保存或换 ID 也会拒绝。新记录的 Insert 碰撞最多尝试三次，然后安全失败。这些是防止并发撤销后复活的安全增强，不是普通请求的完整冲突检测。

数据库截止列或 Redis 剩余 TTL 是持久授权上界，记录 envelope 不能把截止延长到其之后。数据库保存将精度规范为微秒；认证的持久检查复用同一存储。ID/token 是凭据，不放进响应、URL 或日志。

独立构造的 Session 可以使用 `Expiry::OnSessionEnd`、`Expiry::OnInactivity(time::Duration)` 或 `Expiry::AtDateTime(time::OffsetDateTime)`；未提供策略时使用 120 分钟截止。受管理请求默认按最终 `lifetime` 设置无活动期限。`set_expiry` / `expiry_date` 管理持久截止，不自动改写 Provider 的 Cookie 期限配置，`OnSessionEnd` 也不等于修改 `expire_on_close`。

## 失败、取消与并发边界

- 响应发出前保存失败，会替换原业务响应并清空该响应的 Cookie 队列，不发送先前的业务或 Session Set-Cookie。业务已经提交的数据库事务、发送的外部消息等副作用不会因此回滚；应用应按自身业务事务与幂等机制设计。
- handler 被取消时不后台补保存；请求结束或取消会关闭句柄，泄露到请求之外的克隆不能继续修改或保存。已取得的快照不能作为未来请求的授权依据。
- 同一句柄进行 I/O 时同步变更返回 `session_operation_in_progress`；取消 I/O 会回收门控，不承诺外部 store 已执行的写入自动回滚。
- 普通同 ID 并发请求仍可能后写覆盖先写（last writer wins），包括各自的 flash 快照。当前不提供请求阻塞锁、属性级合并或 compare-and-swap 版本控制。

常见错误包括 `session_layer_missing`、`session_closed`、`session_operation_in_progress`、`session_store_write_conflict`（均为语义 500）与 `session_limits_exceeded`（413）。属性限制为最多 1024 项、64 层、20 MiB 键预算，框架元数据也计入；完整记录另受 `max_bytes` 约束，包含值的实际编码字节。资源错误不截断、不吞为匿名成功。

## 示例与当前范围

[示例处理器](../../modules/example/http/controllers/session.rs) 与[模块 HTTP 回归](../../modules/example/tests/session_flows.rs) 使用 `/api/example/session` 的 GET/POST，仅读写示例自己的偏好和提示；支持固定 put/forget/flash/now/keep 操作，不接收任意 Session 键。公开 API 的边界分别见[生命周期测试](../../tests/http/session.rs)、[受管理 HTTP 测试](../../tests/http/session_http.rs)与[数据库存储测试](../../tests/http/session_store.rs)。这些入口对应不同验证层次，不能互相替代。

这里提供 Session 属性、flash 与当前存储/请求生命周期，不代表完整 Laravel Web 组。完整 OldInput、通用 CSRF 中间件、redirect 自动闪存、Session 响应缓存策略、请求阻塞、定时 GC 与全部 Session driver 生态仍需各自能力接入。当前认证业务已有的 CSRF 与来源校验不等于通用 Web CSRF 中间件。记录到期不保证马上物理清除，也不应把内存 store 当作生产持久层。
