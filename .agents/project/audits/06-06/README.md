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

## Mail Fake 与 Notification Fake

- 骨架归 `mail::{MailFake, RecordedMail}` 和 `notifications::{NotificationFake, NotificationTarget, RecordedNotification}`。Fake 状态由当前应用的管理器持有。沿用现有 Mail、Notification Facade、命名邮件器和 Dispatcher 契约。
- 对照固定 [MailFake](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/MailFake.php) 和 [NotificationFake](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Testing/Fakes/NotificationFake.php)。沿用旧版 `6d1a81924` 的 `src/mail/mod.rs` 中应用服务内记录、按收件人检查的思路。旧版的 `src/notifications.rs` 使用渠道替身，未提供按通知和收件人断言的 Fake。
- Rust 的断言读取显式记录。邮件记录提供信封、邮件器、语言、发送结果或队列选项。通知记录提供通知标识、目标、渠道、语言与应用通过 `fake_data` 选择保存的数据。邮件立即发送路径仍组装 MIME，以满足既有 `SentMessage` 返回契约。Fake 不调用真实传输、渠道或队列。应用可为无数据库路由的收件人实现 `notification_key`。
- 实际 Artisan 子进程核对按类型和条件断言已发、已排队和出站数量、Fake 启用前取得的邮件器、模型与按需通知、渠道过滤和语言。实际回环 HTTP 核对应用内已拦截通知，第二个应用仍可通过 array 邮件器正常投递。三语站点构建和两种部署路径的 12 项浏览器检查通过。
- 在提交 `c570cf91d` 的隔离副本中叠加本批改动，Rust 1.94.0 定向回归和工作区全目标严格 Clippy 通过。共享工作树中的并行日志改动曾阻止编译，隔离验收保留其现场。

## Markdown 邮件与独立主题

- `mail::Markdown` 使用 Blade 命名视图、CommonMark 表格及纯 Rust CSS 内联。通过 `Content::markdown` 选择模板，邮件由同一份内容生成 HTML 和纯文本，可指定独立文本视图。主题沿用 `mail.markdown.theme` 和 `paths`，配置缺省时使用内置主题。旧版 `6d1a81924` 的 `src/mail/template.rs` 和 `tests/server/mail/templates.rs` 使用独立文本与 HTML 模板，复用其双版本、插值转义和 SMTP 验收思路。
- 对照固定 [Markdown renderer](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Markdown.php)。Rust 使用同一解析树生成文本和 HTML，原始 HTML 转义，链接方案过滤，CSS 内联不下载远程样式。PHP CommonMark 扩展和 Laravel 邮件专属组件属于额外适配。
- 实际 Artisan 通过 SMTP 向 Mailpit 1.33.1 发送主题邮件。通过记录的 HTTP API 检查纯文本、HTML 标题和表格、自定义主题颜色、变量转义和链接过滤。
- 以独立 MySQL 8.4.13 运行数据库通知、数据库队列、重试及失败记录用例。迁移既有用例时核对 MySQL 无符号重试次数的返回类型。整组 12 项通过，两个夹具有各自入口。三语站点构建及 12 项浏览器检查通过。
- Rust 1.94.0 工作区全目标严格 Clippy 通过。

## 队列通知投递判定

- 对照固定 [NotificationSender](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/NotificationSender.php) 的 Worker 时点判定。Rust 编译型 Provider 登记命名 async guard，队列只保存 guard 名称和值，不序列化闭包或 trait 对象。无 guard 的既有载荷仍按原策略兼容，立即发送继续使用 `should_send`。旧版通知任务已有 Worker 查应用服务的惯例，沿用同样的服务装配。
- 真实 HTTP 请求将两条数据库通知提交到独立 MySQL 8.4.13。入队后交换两项实时状态，由独立 Artisan `queue:work` 判定，只有 Worker 执行时允许的通知落库，拒绝项没有失败记录。验证 guard 在收件人语言作用域执行。
- 隔离副本以 `e3df2a317` 为基线，整组 13 项通过、2 个夹具标注忽略，Rust 1.94.0 工作区全目标严格 Clippy 通过。三语文档构建与 12 项浏览器检查通过。共享工作树并行日志改动的递归泛型实例化错误由隔离验证绕开。
