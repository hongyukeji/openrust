# 06-06 邮件与通知

依赖 03-07 视图、06-03 队列，Worker 验收沿用 06-04。对照固定 [Laravel Mail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md)、[Notifications](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md)、[Mailer 测试](https://github.com/laravel/framework/blob/v13.40.0/tests/Mail/MailMailerTest.php) 和 [NotificationSender](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Notifications/NotificationSender.php)。参考 [Goravel 邮件契约](https://github.com/goravel/framework/blob/v1.18.0/contracts/mail/mail.go)。提取 `v0.x` 的 Lettre 异步 SMTP 配置与凭据校验、MIME 及附件验收和每渠道独立入队的思路。视图使用当前 Blade，语言复用当前 Translator。

MailManager 解析默认或命名邮件器并在应用内缓存。SMTP、log 和 array 共用 MIME 组装。密送只进入投递信封。认证 SMTP 要求 TLS。通知由 ChannelManager 选择邮件或数据库渠道，支持显式立即发送、语言作用域、取消和投递事件。数据库通知保存关联键、数据及已读状态。Artisan 生成编译型通知迁移，应用登记后运行。

Rust 队列载荷显式保存邮件定义或通知渠道载荷及语言，Worker 重新解析传输和视图。自定义通知内容及 `should_send` 在分派时求值，Worker 的 `NotificationSending` 仍可取消投递。队列采用至少一次投递语义。工作器失败记录沿用队列通用错误，SMTP 直发可见脱敏的阶段或状态码。独立 Markdown 主题、供应商与组合传输，以及 Laravel 测试 Fake 的断言 API 仍待对应扩展。实际 TLS 证书与认证服务未纳入本批验收。

## 验收

- Rust 1.94.0，`cargo test --workspace --locked --offline` 483 项通过，3 个子进程入口由集成测试启动。格式与 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。
- SQLite 和 MySQL 8.4.13 验证多态文本键、JSON 及事务。真实 Redis 8.10.3 验证按渠道分队列、回滚丢弃与提交后分派。独立 `queue:work` 进程消化数据库与 Redis 队列，失败测试分两次运行 Worker 验证重试和终态失败记录。
- Mailpit 1.33.1 实际接收邮件与通知。HTTP 查询 Mailpit 验证收件人、本地化主题、纯文本、转义 HTML 和附件。数组邮件器验证两套应用隔离，通知监听器验证取消，数据库验证已读与未读状态。
- 实际 `make:notifications-table` 生成源码。三语 Rust 示例与生成的迁移共同编译，在 SQLite 上执行迁移、邮件发送和数据库通知。三语站点构建、12 项通用浏览器检查和 6 项邮件到通知导航检查通过。

下一单元 06-07 密码重置与邮箱验证。依赖 04-07、05-08、06-01 和本单元。
