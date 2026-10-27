# ADR-044 协议无关验证与调用作用域

- 状态：Accepted
- 日期：2026-10-26

## 背景

HTTP 的 `Validated<T>` 已有嵌套字段路径、统一 `Error::Validation` 与当前 locale 的消息转换。命令和 typed Job 需要复用同一 DTO 校验，而请求关联信息、deadline、取消与语言必须属于一次调用，不能放入共享 `AppState` 或长期任务凭据。

## 决策

- `openrust::validation::{Validate, validate}` 复用现有 `validator` 与 `Error`；HTTP JSON/Query extractor 直接调用它。命令与 Job 可在业务入口显式调用，不新增校验 DSL 或自动数据库唯一性预检。
- `ExecutionContext` 只表示一次 HTTP 请求、命令或队列尝试；task-local 不自动继承至 `tokio::spawn`，子任务需显式持有上下文或所需的取消句柄。
- `ExecutionMetadata` 是唯一可随数据库队列持久化的上下文白名单，只含最多 128 字节安全 ASCII request ID、规范化 `Locale` 与经 `TenantId::new` 校验的可信 tenant ID。身份、权限快照、Cookie、token、任意 header、deadline、取消信号和进程句柄不得序列化。Queue handler 在每次 attempt 恢复 tenant scope，但仍须重新验证业务权限。
- 子调用 deadline 取父 deadline 与请求预算的较早者；取消从父调用向子调用传播，子调用自身 deadline 只取消该子调用，不取消父级或兄弟任务。
- HTTP middleware 建立 request ID、Accept-Language、请求 deadline 和协作取消作用域；非法 request ID 在响应封装前移除并由 request-id middleware 重建。请求超时或处理 future 被丢弃时通知合作子任务。
- `App::run_command` 建立独立的匿名调用作用域，并在应用注册 `I18n` 时使用默认 locale。
- `Queue` facade 从当前作用域读取非敏感 metadata。`QueueDriver` 新增 metadata-aware 派发入口；默认实现只接受空 metadata，非空时返回 `queue_driver_metadata_unsupported`，防止旧 driver 静默丢弃关联信息。
- 数据库队列用版本化 payload envelope 保存 metadata，保留旧的原始 payload 读取；retry 原样保留 envelope，metadata 不参与唯一任务键。`__openrust_execution` 是保留字段，含 metadata 的派发会拒绝应用 payload 使用此字段；历史原始 payload 即使使用该字段，也只在完整匹配已定义的 envelope 结构且 metadata 合法时被视为框架 envelope。
- 每次 worker attempt 新建独立 deadline 与 cancellation，并在 `JobContext` 和 `ExecutionContext` 中共享；`JobRegistry::execute` 与数据库 worker 均在 typed Job handler 外强制 deadline 和取消，任务组子项继承该 attempt 作用域。存在 `I18n` 服务时，任务使用 metadata locale 或应用默认 locale。worker span 分别记录 job ID、稳定名称、attempt 与 request ID，不记录 payload。
- 当前 request ID 是日志关联值，不宣称 W3C trace parent 或 OpenTelemetry span link。gRPC 接入 ADR-040 G2 时必须重新验证 metadata、deadline、取消、认证扩展和 span 关联契约。

## 影响

`JobContext::new` 接受统一 `Cancellation`，并公开 metadata 与剩余预算读取。HTTP、命令与 Job 复用同一 `validate` 入口；任务取消是合作语义，已提交的外部副作用不可撤销。数据库 envelope 和自定义 driver 的 metadata-aware 行为必须由 PostgreSQL、MySQL 与独立 consumer 测试覆盖。
