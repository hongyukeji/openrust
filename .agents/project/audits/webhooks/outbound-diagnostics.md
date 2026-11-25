# Webhook 出站诊断

`DeliveryStore::inspect_delivery` 查询投递状态、已用与最大尝试次数、下次可领时间、截止时间及租约。`inspect_message` 使用 `publish` 回执中的内部消息 ID 查询事件类型、schema 版本、登记时间及首次投递 ID。`inspect_attempts` 按固定投递 ID 及递增尝试序号分页，每页 1 至 100 条，返回下一页游标。查询只取指定字段。尝试依投递表关联的范围约束，跨范围或不存在的投递返回空页。消息和投递详情不存在时返回空值。

`webhook-server:inspect` 将相同契约写成单行 JSON。Artisan 由可信运维操作，支持指定数据库和范围。扩展未注册对公网的诊断路由。应用按自身授权规则开放范围后可重用公开服务。输出不含请求或响应正文、URL、密钥、Header、幂等键或完整快照。借鉴 Svix 分开查询消息与尝试、按已授权应用隔离并有界分页的职责。 [Svix 投递诊断说明](https://www.svix.com/webhook-monitoring/)

直接 Builder 提交目前以投递 ID 查询；按任意 Builder 指定的外部消息 UUID 聚合、端点维度列表、批量重放、留存清理和多主机验收仍跟踪。普通 CONNECT 代理仍在 WS-08。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过 Artisan 迁移、配置及路由缓存、真实 HTTP、具名连接的 Artisan 单行 JSON 与独立 Worker。覆盖待处理与成功状态、503 后 204 的尝试顺序、下一页及超大游标、非法限额、空订阅及有投递的发布消息、凭据和正文脱敏、应用权限拒绝及 CLI 跨范围隔离。SQLite 原有直接发送及扇出回归通过，接收路由套件的 136 个真实 HTTP 请求与 83 个独立 Worker 通过。Rust 1.94.0 隔离工作区格式检查、发送包九项合约、三语六组示例、受影响目标及应用夹具严格 Clippy 通过。
