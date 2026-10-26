# ADR-042：数据库共享锁与请求幂等

- 状态：Accepted
- 日期：2026-10-26
- 范围：PostgreSQL/MySQL 的跨实例租约与显式请求幂等；不扩展 `Queue::dispatch_unique` 的语义

## 决策

- 提供 `SharedLocks` 与 `RequestIdempotency` 两个显式服务，复用 `Db`、数据库 UTC 时钟及 `DatabaseTableNames`。迁移 `CoordinationMigration` 由应用 migrator 显式注册；服务不会隐式建表，也不依赖 Redis。
- 锁使用有界逻辑 key 的摘要、CSPRNG owner token、数据库到期时间和单调 fencing token。到期后可竞争接管；renew/release 必须匹配当前 owner，旧 lease 不能解锁新 lease。需保护同库写入时，在同一业务事务调用 `assert_current_in_transaction`；TTL 本身不保证暂停的旧持有者已停止，也不保护任意外部副作用。
- 请求幂等按调用方提供的 scope + key 隔离，持久化摘要而非原 key；调用方提供规范化请求指纹。首次/过期键可领取 owner lease；同指纹有效处理中返回 `InProgress`，指纹不同返回 `Conflict`，已完成且保留期内返回有界 JSON `Replay`。保留截止时间有索引，应用维护任务分批清理过期记录。
- 业务写入及成功响应快照必须在同一个 `Db::transaction` 中，通过 `complete_in_transaction` 完成。回调失败时数据库写入与完成快照回滚，处理中 lease 保留至到期后可接管；明确失败且尚未产生副作用时可用匹配 owner `release` 立即放弃。外部供应商/网络副作用不与数据库原子，须结合供应商幂等键或事务队列。
- 响应只保留 HTTP 状态与受大小限制的 JSON body，不自动缓存 header、二进制、流式响应或凭据。应用负责身份/接口 scope、规范化指纹、授权后回放及敏感字段剔除；框架不增加隐式 HTTP middleware。
- Key/scope/指纹/响应均有大小上限，租约与结果保留窗口为 1 毫秒至 24 小时，输入与载荷不进入日志；按过期时间索引进行有界清理。数据库事务/条件写保护保证已声明范围；原生连接逃生口和不协作外部副作用不受自动 fencing。

## 验收

PostgreSQL 18 与 MySQL 8.4 真实共享隔离数据库验证：多 `Db` 实例同时竞争仅一个 owner；活跃锁忙、到期接管、旧 owner renew/release/fenced transaction 失败；同键并发只产生一个幂等 owner；身份 scope 隔离、指纹冲突、处理中与完成回放、完成结果保留/过期接管；业务事务回滚时写入与 replay 快照一并撤销；连接/进程故障后 lease 到期可恢复。对外部副作用及跨系统 exactly-once 不作保证。
