# Pail 认证解析边界

## 2026-11-20 最新状态

V04 已按[显式请求日志入口](pail-auth-timing.md)完成 Rust 适配并验收：Log／LogManager／Logger 提供可 await 的 logWithRequest，内置 HTTP 异常报告已接入；同步代码继续捕获已解析身份。下方为修复前的契约分析，不再表示此适配仍未实现；任意同步回调无法自动等待异步 Guard 的语言约束仍然成立。

## 原始复核结果

固定 Pail `fdb73f5eacf03db576c710d5a00101ba185f2254` 的 Handler 在同步 `MessageLogged` 监听中调用 `Auth::id()` 和 `Auth::user()`，可以当场驱动 PHP guard 完成认证。当前 Rust 的 `Log::listen` 返回同步 `LogResult<()>`；`AuthManager::user`／`Guard::user` 返回异步 future，并独占借用 `&mut Request`。自定义 RequestGuard 可包含任意异步 I/O，不能假定它能在一次 poll 内完成。来源对象只持有只读属性快照，不持有可重新借用的请求。 [固定 Handler](https://github.com/laravel/pail/blob/fdb73f5eacf03db576c710d5a00101ba185f2254/src/Handler.php)

具体入口：`vendor/openrust-framework/src/log/facade.rs`、`log/events/mod.rs`、`auth/auth_manager.rs`、`auth/request_guard.rs`、`contracts/auth.rs` 和 `log/context/origin.rs`。根 `main.rs` 使用 Tokio current_thread，同步监听不能以阻塞等待方式安全驱动同一个请求的异步认证。

## 未采用的替代行为

- 在 HTTP 入口提前认证，会把查询、失败和 guard 副作用移到第一次日志之前，并且改变无日志请求的行为。全局入口也可能早于 Session、中间件选择 guard 和请求属性初始化。
- 在响应结束后回填身份，会把后续登录、退出或 guard 切换后的状态用于先前日志；认证失败也会移到另一个时机。
- 复制请求解析 guard 无法共享原 guard 状态、会话与副作用；以原始指针绕过独占借用不能作为正式接口。

上述方式均不能证明固定 Laravel 调用时序等价，不作为认证差异的闭合实现。

## 修复前的后续范围

修复前，认证自动解析是 **未闭合** 的契约设计项。若要完整保留调用时序，需为日志观察提供可以显式 await、且可合法访问当前请求的入口，并一并核对异常报告、同步容器回调和既有同步日志消费者。应作为独立功能簇设计和验收，不能仅新增一个 Pail 中间件就标记完成。

此前已验收行为继续保留：同步捕获当前 guard 已解析的身份，guard 切换和登出后的快照可更新，未解析时为 null。Pail 消息解析、筛选、打印和会话命令不依赖自动认证解析，可按依赖顺序继续推进；当时 Pail 整体保持未闭合；最新实现、公开入口和剩余平台范围以上方专项验收为准。
