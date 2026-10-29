# Session 当前事实

> C04-D1限定子范围已完成适用集成验收，提交以Git为准。契约见 [ADR-082](../decisions/adr-082-session-store-and-flash.md)，唯一执行队列见 [todo](../todo.md)。

## 源码与公开边界

- `src/session.rs` 导出 `Session`、`Expiry`、`SessionId`、`SessionRecord`、`SessionStore`、写入模式/结果以及数据库、显式内存和可选 Redis store；`session` feature 与 `session-redis` 的实际依赖以 Cargo.toml 为准。
- `Session` 在同请求内共享状态。属性方法同步返回 Result，真正 store I/O 的 start/save/migrate/regenerate/invalidate 为 async；同步锁不跨 await。I/O 期间修改明确拒绝，取消释放 busy，不后台补保存，也不承诺撤销已发生的外部写入。
- ID 为大小写敏感的40位 ASCII 字母数字，构造即有；有效外部 ID 未命中记录时保留。启动补缺失或 null 的 token，非 null 的假值保留。ID/记录 Debug 脱敏；显式字符串与序列化仍可能暴露凭据，不能写入日志。
- 属性读取采用完整字面键优先的 Arr 语义，缺失与 null 分开。flash/now/reflash/keep 使用同一属性树，每次 save 推进老化。JSON 表示不能保留 PHP 数组删除尾项后的隐藏 next-free 历史，具体差异见 ADR。
- 已加载记录仅允许原子 Update；记录删除、过期或重复 start 发现撤销时拒绝以旧认证数据重建。该策略是保留的 OpenRust 安全增强，不冒称 Laravel 缓存 Session 默认 PUT 语义。普通同 ID 并发仍可能最后写入覆盖，不提供默认分布式锁。
- 持久键与 envelope 以原始 ID 字节的 SHA-256 绑定，不额外持久化原始 ID。受管理读取/编码有大小与深度边界；自定义 driver 须遵守公开契约，框架仍校验返回记录。数据库独立到期列是授权截止上界。

## 当前证据与缺口

- Session-only定向基础测试18项通过（0失败/0忽略，390过滤，含名称推导）；公开生命周期测试10项通过，包含认证记录删除后重复 start/save 拒绝；进程内正式Router HTTP测试4项通过（均0失败/0忽略/0过滤），覆盖5xx/未修改保存、取消和缺失装配。它们是对应源码快照的定向证据，不能替代修改后重验、workspace 门禁或真实数据库运行。
- 公开 Store 真实PG18/MySQL8.4回归各通过1项命名fixture（0失败/0忽略/0过滤、sourceguard无漂移、两侧隔离锚库已核不存在），覆盖冲突/撤销、文本顺序/NUL/精确预算及授权截止上界；MySQL重复同记录更新也实际通过。这一专项不替代完整Auth/模块双库HTTP流程。
- Redis8共享Store专项1项命名fixture已实际通过（无失败/忽略/过滤、源码无漂移），覆盖独立连接共享、命名空间隔离、真实TTL、防复活、顺序/NUL及预算。仅使用自有随机命名空间并清理自有ID；不是集群或完整Auth Redis链验收。
- 当前统一实现的PG18/MySQL8.4完整Auth database target各8项及Redis8完整Auth runtime10项已实际通过，均无失败/忽略/过滤、源码无漂移；两侧数据库隔离锚库已核清理。配置最终覆盖8项、公开生命周期10项与进程内HTTP4项已定向重验通过。根PG18/MySQL8.4完整模块HTTP各53项及生成plain4/认证双库各2项另已实际通过。浏览器原生短TTL双库各4、Keycloak真实主流程各1、模块OIDC各8、新Session及默认入口TLS与最近修复后的完整12stage门禁均已实际通过；精确忽略项、候选映射与清理事实见testing，不继承旧tower证据。
- 完整 Old Input、CSRF、redirect、GC、blocking、Session cache 与其余 driver 生态按后继矩阵交付；本文件不将基础状态机视为整个 Session 能力完成。
