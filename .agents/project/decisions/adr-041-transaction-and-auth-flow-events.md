# ADR-041：事务与认证流程事件

- 状态：Accepted
- 日期：2026-10-26
- 范围：标准 `DatabasePlugin` 管理的事务、`AuthFlows` 注册和登录扩展

## 决策

- 复用 `EventsPlugin`，不增加独立 Hooks 注册表。`DatabasePlugin::finish` 在所有插件完成 `build` 后将可选 `Events` 绑定至标准 `Db`；自定义数据库插件不会自动获得此行为。
- `Db::transaction` 在业务回调成功后、提交前派发 `TransactionBeforeCommit`。所有 direct/queued listeners 按注册顺序执行；错误触发回滚。队列监听器在同一事务写入，只有提交成功才可被 worker 领取。
- 成功提交后派发 `TransactionCommitted` 的 direct listeners。其错误只记录稳定错误码并继续返回已提交结果；不运行 queued listeners，避免提交后第二次写库造成非原子通知。需要可靠副作用时监听 `TransactionBeforeCommit` 并使用事务队列。
- `Events::dispatch` 是独立于数据库提交的普通派发。直接调用 `DatabaseConnection`、SeaORM connection 或原生驱动 API 不经过 `Db::transaction`，不触发这些生命周期事件。框架不承诺嵌套事务、savepoint 或底层逃生口的覆盖。
- 注册验证和登录验证事件只调用 direct listeners。注册验证、账户写入和 `RegistrationSucceeded` 在同一事务；拒绝/监听失败不留下账户。验证码签发在账户提交后执行；失败时保留未验证账户，调用方可重试申请，不表示邮件已送达。
- 登录失败派发无身份、无原因的 `LoginFailed`。JWT 登录 refresh token 插入与 `LoginSucceeded` 共用事务；成功事件失败即回滚 refresh 记录，不返回凭据。Session 登录复用登录前/失败验证，但不发 `LoginSucceeded`：Session store 保存由 HTTP middleware 在稍后完成，不能虚称与数据库原子提交。
- 注册校验事件暴露规范化邮箱，供业务资格策略读取；邮箱属于个人信息，监听器不得记录。事件不含密码、哈希、验证码、access/refresh token、PAT、Session ID 或原始 HTTP 请求。

## 验收边界

事务提交前拒绝、回滚、成功提交后的 listener 错误及认证流程拒绝/原子写入在 PostgreSQL 18 与 MySQL 8.4 使用真实隔离数据库验证。direct listener 是进程内尽力处理；进程在提交后退出时允许通知丢失，跨节点/可靠副作用必须使用事务队列及幂等任务。
