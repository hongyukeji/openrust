# 审计

`openrust::audit` 通过可替换的 `AuditSink` 记录显式的安全与业务事件。内置的 `DatabaseAuditSink` 会将记录写入应用主数据库。请将 `DatabaseAuditMigration` 加入应用 migrator；迁移不会自动执行。

事件包含可选 actor、action、object、结果、时间戳、可选 request ID 和可信来源 IP，以及调用方显式提供的 details。actor 与 object 应使用稳定且不含秘密的 ID；不要用电子邮件地址、凭据或用户输入的自由文本作为标识符。`AuditPolicy` 默认拒绝 details，并通过白名单限制可记录的标量字段。敏感 detail 名称会以 `[REDACTED]` 保存。Event 和 record 的 `Debug` 输出会隐藏标识符与 detail 值。不要将密码、token、验证码、原始请求/响应、SQL 或任意错误文本作为 details 传入。

```rust,no_run
use openrust::audit::{Audit, AuditEvent, AuditOutcome, AuditSubject};

async fn record_change(audit: &Audit, actor_id: i64, account_id: i64) -> openrust::Result<()> {
    audit.record(AuditEvent::new(
        "account.updated",
        AuditSubject::new("account", account_id.to_string())?,
        AuditOutcome::Succeeded,
    )?.actor(AuditSubject::new("user", actor_id.to_string())?)).await
}
```

对于关键数据库变更，先准备完成脱敏的 record，再使用业务变更所用的同一个 `DatabaseTransaction` 调用 `DatabaseAuditSink::record_in_transaction`。外层事务负责提交；任一写入失败时都应传播错误，使两者一并回滚。拒绝、失败或已回滚操作的记录必须在失败事务结束后写入。提交后发生的 sink 错误不会撤销业务变更。

自定义 sink 可实现公开的 `AuditSink` trait，并通过 `AuditPlugin::with_sink` 注册。启用 `test-util` feature 后，测试可使用 `openrust::audit::testing::FakeAuditSink`。Sink 故障会向上传播；框架不提供回退、自动重试、去重或 exactly-once 保证。审计查询授权与保留策略由应用负责。`purge_before` 最多从主库删除指定批量的记录。框架不承诺存储具备防篡改能力，也不保证满足特定法律合规要求。
