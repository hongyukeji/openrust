# 06-06 邮件与通知

依赖 03-07 视图、06-03 队列，Worker 验收沿用 06-04。对照固定 [Laravel Mail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md)、[Notifications](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md)、[Mailer 测试](https://github.com/laravel/framework/blob/v13.40.0/tests/Mail/MailMailerTest.php) 和 [NotificationSender](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Notifications/NotificationSender.php)。参考 [Goravel 邮件契约](https://github.com/goravel/framework/blob/v1.18.0/contracts/mail/mail.go)。提取 `v0.x` 的 Lettre 异步 SMTP 配置与凭据校验、MIME 及附件验收和每渠道独立入队的思路。视图使用当前 Blade，语言复用当前 Translator。

MailManager 解析默认或命名邮件器并在应用内缓存。SMTP、log 和 array 共用 MIME 组装。密送只进入投递信封。认证 SMTP 要求 TLS。通知由 ChannelManager 选择邮件或数据库渠道，支持显式立即发送、语言作用域、取消和投递事件。数据库通知保存关联键、数据及已读状态。Artisan 生成编译型通知迁移，应用登记后运行。

Rust 队列载荷显式保存邮件定义或通知渠道载荷及语言，Worker 重新解析传输和视图。自定义通知内容及 `should_send` 在分派时求值，Worker 的 `NotificationSending` 仍可取消投递。队列采用至少一次投递语义。工作器失败记录沿用队列通用错误，SMTP 直发可见脱敏的阶段或状态码。独立 Markdown 主题及 Laravel 测试 Fake 的断言 API 仍单独跟踪。生产 SMTP 和真实供应商投递的状态见下方专项验收。

## 验收

- Rust 1.94.0，`cargo test --workspace --locked --offline` 483 项通过，3 个子进程入口由集成测试启动。格式与 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。
- SQLite 和 MySQL 8.4.13 验证多态文本键、JSON 及事务。真实 Redis 8.10.3 验证按渠道分队列、回滚丢弃与提交后分派。独立 `queue:work` 进程消化数据库与 Redis 队列，失败测试分两次运行 Worker 验证重试和终态失败记录。
- Mailpit 1.33.1 实际接收邮件与通知。HTTP 查询 Mailpit 验证收件人、本地化主题、纯文本、转义 HTML 和附件。数组邮件器验证两套应用隔离，通知监听器验证取消，数据库验证已读与未读状态。
- 实际 `make:notifications-table` 生成源码。三语 Rust 示例与生成的迁移共同编译，在 SQLite 上执行迁移、邮件发送和数据库通知。三语站点构建、12 项通用浏览器检查和 6 项邮件到通知导航检查通过。

下一单元 06-07 密码重置与邮箱验证。依赖 04-07、05-08、06-01 和本单元。

## Sendmail、组合传输与供应商 API

对照固定 Laravel [MailManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/MailManager.php) 和 [邮件传输](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Transport)。沿用当前 MIME、视图与脱敏错误，复用现有 AWS SDK 凭据链及异步 HTTP。Sendmail 通过程序及独立参数执行，支持 `-bs` 和 `-t`。SES 发送原始 MIME 与独立密送信封，Postmark 和 Resend 从同一封已渲染邮件构建 JSON。组合邮件器记录暂时失败的传输，故障转移保留可用邮件器，轮询依次分派。配置循环在解析时报错。

- 回环协议夹具验证 SES SigV4、临时凭据、MIME 和密送，核对 Postmark 与 Resend 的认证头、消息流、附件、密送及回执。实测组合切换、冷却期跳过、轮询顺序和脱敏的 401。
- 独立子进程验证 Sendmail SMTP 会话、命令路径带空格、两种模式的信封与 MIME。
- Mailpit 1.33.1 独立容器收取一封真实 SMTP 邮件，API 核对收件人及主题。带认证信息但无 STARTTLS，以及 STARTTLS 对端使用不受信的自签证书，均被框架拒绝，没有额外投递。
- 真实 Artisan 调度夹具复验 `schedule:run` 与 `schedule:work`，覆盖到期执行、任务错误后继续及 SIGTERM 等待当前任务。三语站点构建和十二项浏览器检查通过。
- 真实 SES、Postmark、Resend 投递、生产 SMTP 受信证书＋认证＋目标邮箱收信，以及目标服务器部署调度，待有指定环境和测试账号后验收。协议夹具使用测试凭据和回环端点。

以已提交主线 `cd17b297e` 创建隔离验收副本。Rust 1.94.0 全目标严格 Clippy 和本批文件格式检查通过。全仓格式检查只报告既有 Sanctum 断言换行。三语邮件和 HTTP Client 页面在 `/` 与 `/docs/` 下新增章节检查共十二项通过。

## SES v2、Mailgun 和 Cloudflare

- 骨架沿用 `mail::transport` 和 `MailManager`，在 `config/mail.rs` 登记 `ses-v2`、`mailgun` 和 `cloudflare`，服务凭据由 `config/services.rs` 装配。本批沿用现有 Artisan 命令契约，测试夹具单独登记发送命令。
- 按固定 [MailManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/MailManager.php)、[SesV2Transport](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Transport/SesV2Transport.php) 及 [CloudflareTransport](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Transport/CloudflareTransport.php) 实现。SES v2 复用 AWS 凭据链并使用 AWS SDK Rust 客户端。Mailgun 和 Cloudflare 复用 reqwest 和现有 MIME，Mailgun 的 SMTP 方案复用 Lettre。旧版 `6d1a81924` 的 `src/mail/mod.rs` 与 `tests/server/mail/templates.rs` 中无这三项传输，复用现行传输和附件处理，沿用旧版 SMTP 失败脱敏的验收原则。
- Rust 1.94.0 的两项定向集成测试通过。实际 Artisan 子进程通过回环 HTTP 完成四次请求，核对 SES v2 SigV4、会话令牌、MIME、密送和选项，Mailgun MIME 与字段两种方式、认证和回执，以及 Cloudflare 的认证、地址和附件。原有 SES、Postmark、Resend 和组合传输的回环回归通过。Mailgun SMTP 复用已有 SMTP 实现，未单独连真实 Mailgun SMTP。
- 工作区全目标严格 Clippy、本批文件格式、三语站点构建及 `/`、`/docs/` 的 12 项浏览器检查通过。全仓格式检查报告既有 Sanctum 测试断言换行差异。本批未连接真实供应商，生产投递待指定凭据及收件端后验收。

## 按需通知与广播渠道

- 骨架归 `notifications::{AnonymousNotifiable, NotificationRoute}`、`notifications::channels::BroadcastChannel`、`notifications::messages::BroadcastMessage`。沿用现有 `queue:work` 命令和事件派发器。旧版 `6d1a81924` 的 `src/notifications.rs` 和 `tests/server/queue/notifications.rs` 覆盖按渠道拆分与 Worker，未提供按需收件人或广播通知。沿用现行分渠道队列载荷和 Worker。
- 对照固定 [AnonymousNotifiable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/AnonymousNotifiable.php)、[BroadcastChannel](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Channels/BroadcastChannel.php) 和 [BroadcastNotificationCreated](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Events/BroadcastNotificationCreated.php)。Rust 以枚举保存渠道地址、以 trait 方法覆盖广播类型和载荷。模型默认用数据库路由的类型与主键构成私有频道名。队列含显式事件数据和选定连接，不序列化任意 Rust trait 对象。
- 实际回环 HTTP 经框架 Kernel 发送按需通知，用内存邮件器核对收件人，用 MySQL 8.4.13 核对按需收件人的数据库渠道被跳过。独立 Artisan `queue:work` 消化指定广播队列，在 log broadcaster 核对私有频道、事件名、通知类型与 ID。
- 独立 MySQL 8.4.13、Redis 8.10.3 和 Mailpit 1.33.1 回归通过，验证事务提交及回滚边界。真实 `queue:work` 处理两封邮件和一条数据库通知，Mailpit HTTP API 核对两封邮件的收件人和主题。工作区全目标严格 Clippy、三语站点构建及 12 项浏览器检查通过。
