# 06-06 邮件与通知

依赖 03-07 视图、06-03 队列，Worker 验收沿用 06-04。对照固定 [Laravel Mail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md)、[Notifications](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md)、[Mailer 测试](https://github.com/laravel/framework/blob/v13.40.0/tests/Mail/MailMailerTest.php) 和 [NotificationSender](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Notifications/NotificationSender.php)。参考 [Goravel 邮件契约](https://github.com/goravel/framework/blob/v1.18.0/contracts/mail/mail.go)。提取 `v0.x` 的 Lettre 异步 SMTP 配置与凭据校验、MIME 及附件验收和每渠道独立入队的思路。视图使用当前 Blade，语言复用当前 Translator。

MailManager 解析默认或命名邮件器并在应用内缓存。SMTP、log 和 array 共用 MIME 组装。密送只进入投递信封。认证 SMTP 要求 TLS。通知由 ChannelManager 选择邮件或数据库渠道，支持显式立即发送、语言作用域、取消和投递事件。数据库通知保存关联键、数据及已读状态。Artisan 生成编译型通知迁移，应用登记后运行。

Rust 队列载荷显式保存邮件定义或通知渠道载荷及语言，Worker 重新解析传输和视图。通知可登记命名 guard，在 Worker 执行时以当前状态决定投递，`NotificationSending` 仍可取消投递。队列采用至少一次投递语义。工作器失败记录沿用队列通用错误，SMTP 直发可见脱敏的阶段或状态码。Markdown 主题、测试 Fake 和部署环境验收范围见下方记录。

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

## 邮件投递事件、扩展传输和通知队列中间件

- 对照固定 [Mailer](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailer.php)、[MailManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/MailManager.php) 和 [SendQueuedNotifications](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/SendQueuedNotifications.php)。旧版 `MailDriver` 和 `Notifier` 只有传输及队列入口，现行实现在保留 Rust trait 边界的同时复用应用事件派发器和 Worker 中间件链。
- 按应用注册传输工厂，由命名邮件器配置选择。对实际投递派发 `MessageSending` 和 `MessageSent`。监听器否决时 `send` 返回 `None`，现有 `sendNow` 返回契约使用 `MailError::Cancelled` 表达取消。队列遇到取消会正常确认任务。
- 通知可按收件人和渠道选择 Worker 限制和已登记的中间件链。Rust 在队列中保留编译型链的名称；Worker 沿用常规任务中间件管道。没有指定中间件的既有通知继续使用原有任务名称。
- 真实回环 HTTP 和独立 Artisan 发送命令验证命名自定义传输、投递前后顺序及取消时不调用传输。HTTP 提交三条数据库队列通知，核对序列化重试上限。独立 Worker 验证实时 guard 和普通队列 `Skip` 中间件，只有允许且未被中间件跳过的通知写入 MySQL。

## 06-06 整单元验收

| 范围 | 整单元证据 |
| --- | --- |
| 邮件器、传输和 MIME | SMTP 真实投递至 Mailpit，实际 Sendmail 进程及供应商回环协议覆盖信封、密送、附件、认证、回执和故障切换 |
| 模板和发送生命周期 | Blade 与纯 Rust Markdown 邮件经过实际 Artisan 和 SMTP，HTTP 核对 Mailpit 收件内容、双 MIME、主题内联与转义，实际 HTTP 和 Artisan 验证发送前后事件 |
| 通知 | 按需邮件、数据库通知、本地化、广播、失败与跳过通过 HTTP、MySQL 8.4 和独立 Artisan Worker 验证 |
| 队列 | MySQL 8.4 与 Redis 覆盖事务提交及回滚、Worker 队列拆分、重试、终态失败、动态 guard 和中间件 |
| Fake | HTTP 路由和 Artisan 子进程验证邮件发送与入队断言、通知收件人及渠道断言、应用隔离和无实际副作用 |

最终批次按提交 `48af9dfe0` 的隔离副本验收。Rust 1.94.0 的 `mail_notifications` 整组 14 项通过，2 项为单独启动的夹具。独立服务为 MySQL 8.4.13、Redis 8.10.3 和 Mailpit 1.33.1。工作区全目标严格 Clippy、本批格式、差异空白检查和三语构建通过。浏览器检查在 `/` 和 `/docs/` 共 12 项通过。全仓格式检查仍只有既有 Sanctum 测试断言换行差异。

按项目交付计划的核心范围闭合 06-06。受信 SMTP 证书、认证及真实邮箱，以及真实供应商账户投递留待有目标环境和凭据时验证。纯 Rust Markdown 使用 Blade 与内置 CommonMark 解析，Laravel 的 PHP CommonMark 扩展、邮件专属组件及 `vendor:publish` 邮件资源发布属于后续适配。

## Markdown 邮件布局和主题发布

- 对照固定 Laravel v13.40.0 的 `MailServiceProvider` 与 Markdown 组件自定义章节。OpenRust 沿用编译型供应商发布，由 `vendor:publish --tag=laravel-mail` 向 `resources/views/vendor/mail` 发布 HTML 布局、纯文本布局和默认主题。旧版 `src/mail/template.rs` 与 `tests/server/mail/templates.rs` 使用 Minijinja 预登记及限长渲染，本批继续沿用现行 Rust Blade、CommonMark 与 CSS 内联。邮件专用的 message、button、panel、table 等 Blade 组件单独跟踪。 [固定 Laravel 发布](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/MailServiceProvider.php#L39) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#customizing-the-components)
- 从 `9bc0d6a11` 的干净归档验证普通发布保留编辑，`--force` 恢复三个框架资源。应用内声明的相对 `mail.markdown.paths` 从应用根解析并按顺序优先读取，随后尝试标准发布目录和内置资源。邮件正文先通过既有 Markdown HTML 和 URL 处理，再放进独立布局的 `slot`。显式 `text_view` 完整替代生成文本。实测无效 UTF-8 的发布文本布局仍可被显式文本视图绕过。
- 真实 Artisan 发送两个 Markdown 邮件，经 Mailpit 1.33.1 的实际 HTTP API 验证 HTML 页眉、CSS 颜色、文本布局或显式文本、表格、HTML 转义与危险链接处理。MySQL 8.4.13、Redis 8.10.3 与 Mailpit 的邮件通知套件 14 项通过、2 项专用夹具忽略。发布器回归 10 项通过。根应用实跑三语共同示例，独立二进制在无源码目录从另一个工作目录发布相同的三个文件。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。临时服务和隔离构建、站点、发布产物已回收。提交前磁盘可用 98.38 GB（91.62 GiB），周额度剩余 35%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-publish.NHa52FfZ`。

## Markdown 邮件的四类 Blade 组件

- 对照固定 Laravel v13.40.0 的 Markdown HTML 和纯文本模板。OpenRust 接通 `x-mail::message`、`x-mail::button`、`x-mail::panel`、`x-mail::table`。消息按应用名称、URL 和 `subcopy` 插槽生成邮件内容。按钮支持可绑定 URL、颜色和对齐方式，纯文本含按钮标签及 URL。面板保留 Markdown 正文，表格在 HTML 中显示，纯文本沿用上游的空模板。布局和主题沿用 Rust 版 HTML／CSS，页脚使用 UTC 年份。v0.x 的 Minijinja 邮件未发现可复用的 Blade 组件，保留编译型装配。 [固定 HTML 模板](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/resources/views/html) · [固定纯文本模板](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/resources/views/text)
- Markdown 渲染时按输出格式在本次 Blade 渲染上下文解析组件。组件标记只在本次渲染中被替换。普通 Markdown 的 HTML 转义和链接过滤继续生效，按钮 URL 也使用同一链接规则。配置目录及标准发布目录沿用原优先级，`--tag=laravel-mail` 现在发布 11 个文件，既有覆盖以及独立 `text_view` 的优先级继续通过。
- 基于 `2f55c9802` 的隔离快照，用真实 Artisan、SMTP 和 Mailpit 1.33.1 的 HTTP API 核对默认头尾、命名插槽、绑定链接的转义、危险方案过滤、HTML 表格和纯文本省略、自定义面板、已发布文件保留及强制重置。MySQL 8.4.13、Redis 8.10.3 与 Mailpit 的邮件通知套件 14 项通过、2 项专用夹具忽略。视图和发布回归 15 项通过。实际根应用和无源码独立二进制发布同样的 11 个资源。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。单独调用的 `header`、`footer`、`subcopy` 等邮件组件、页脚本地化及上游布局样式继续跟踪。临时服务和隔离构建、站点、发布产物已回收。提交前磁盘可用 98.34 GB（91.58 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-components.zEOOhdVM`。

## Markdown 邮件页眉、页脚与补充说明

- 对照固定 Laravel v13.40.0 的 Markdown 消息与两种输出模板，接通独立的 `x-mail::header`、`x-mail::footer`、`x-mail::subcopy`。内置 `message` 和独立调用共用发布模板。默认页脚用应用时区计算年份，读取邮件语言的 JSON 翻译，缺失时回退到英语原句。旧版 Minijinja 邮件模板和回归未发现对应的 Blade 组件。OpenRust 保留编译型发布、CommonMark 的 HTML 转义与 URL 过滤、现有布局样式及应用标题。 [固定 Laravel 消息模板](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/resources/views/html/message.blade.php) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#customizing-the-components)
- 基于 `66e175a90` 的干净归档叠加本批验收。真实 Artisan 通过 SMTP 向 Mailpit 1.33.1 投递，HTTP API 校验 HTML 和纯文本中的独立组件、命名插槽、页首链接、共享模板覆盖、法语和英语回退。在 UTC 已进入 2030 年而应用时区仍为 2029 年的冻结时刻，两个 MIME 版本均显示 2029 年。源码和无源码独立二进制发布相同的 17 个文件，覆盖发布与独立 `text_view` 回归通过。
- 按项目 `python3 -B tests/mysql_acceptance.py` 临时实例入口复验 MySQL 8.4.13。与 Redis 8.10.3 和 Mailpit 一起运行的邮件通知套件 14 项通过、2 项专用夹具忽略。视图及发布回归 15 项通过。Rust 1.94.0 全仓格式、严格 Clippy、三语站点与 18 项浏览器检查通过。Laravel 默认表格布局、主题样式及独立 `x-mail::layout` 继续跟踪。临时服务和隔离构建产物已回收。提交前磁盘可用 98.20 GB（91.46 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-slots.ZWXREHY1`。

## Markdown 邮件显式布局与响应式主题

- 对照固定 Laravel v13.40.0 的 `mail::layout` 和主题。显式布局接收 `head`、`header`、`subcopy` 和 `footer` 槽，输出一次文档。OpenRust 以一次渲染作用域内的标记识别显式根布局，边界要求它包住整个视图。HTML 内置布局使用应用标题、邮件语言、响应式 570 px 正文和页脚。文本沿用分段输出，省略 HTML 文档头。旧版 Minijinja 没有同类 Blade 邮件布局。 [固定 HTML 布局](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/resources/views/html/layout.blade.php) · [固定主题](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/resources/views/html/themes/default.css)
- 基于 `5c6553caa` 的干净归档验收。实际 Artisan 通过 SMTP 投递到 Mailpit 1.33.1，真实 HTTP 校验标题和语言、转义的 head 属性、正文和纯文本槽位顺序、默认按钮绿色与 600／500 px 媒体规则。Chrome 实测同一封投递邮件在 800 px 和 420 px 窗口下的正文、页脚和按钮宽度。窄屏按钮修复边框撑开后未超出操作区。布局外有正文时 Artisan 返回 1。旧的布局覆盖、强制发布、显式纯文本及 17 个独立发布资源继续通过。
- 项目 `python3 -B tests/mysql_acceptance.py` 临时实例与真实 Redis 8.10.3、Mailpit 运行邮件通知套件 14 项通过、2 项专用夹具忽略。视图及发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。内置主题保留 Rust CSS 内联与应用标题，具体 HTML 属性、CSS 声明和各邮件客户端视觉结果有差异或待验证。临时产物已回收，磁盘可用 98.09 GB（91.36 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-layout.92aDRJVG`。

## Markdown 邮件扩展与安全编码

- 对照固定 Laravel v13.40.0 的 `Markdown::converter` 和 `withSecuredEncoding`。邮件继续默认启用 CommonMark 与表格，Rust 通过 `mail.markdown.extensions` 按应用配置四种内置解析选项。安全编码默认关闭，可通过 `mail.markdown.secured_encoding` 启用。转义表达式使用本次渲染内部标记，解析完成后还原到文本或显式链接目标，链接过滤仍在还原后执行。HTML 文档头还原时执行 HTML 转义。v0.x 的邮件模板使用 Minijinja，未发现这两个契约的可复用实现。 [固定 Laravel Markdown 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Markdown.php#L70) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#writing-markdown-messages)
- 基于 `1150acaa0` 的干净归档验收。同进程两个应用经 Artisan 控制台内核和 SMTP 发送，Mailpit 1.33.1 的 HTTP API 核对默认和启用配置的 HTML、纯文本、脚注、任务列表、删除线、标点及动态 URL。安全编码启用后，变量里的 Markdown 链接和强调语法在正文与代码中按字面显示。HTML 文档头保留属性值，危险链接方案被过滤。未知扩展名时 Artisan 返回 1。
- 项目 `python3 -B tests/mysql_acceptance.py` 的 MySQL 8.4.13、真实 Redis 8.10.3 与 Mailpit 的邮件通知套件 15 项通过、2 项夹具忽略。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已复验 800／420 px 投递邮件与 17 项独立发布资源。OpenRust 的扩展使用 Rust 内置名称，安全编码涵盖生成的 HTML 和纯文本。Laravel 的 PHP 扩展类及静态切换接口继续单列。临时产物已回收，磁盘可用 98.04 GB（91.30 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-encoding.dtkHJoWA`。

## 单封 Markdown 邮件主题

- 对照固定 Laravel v13.40.0 的 `Mailable::markdownRenderer` 和自定义 CSS 文档。OpenRust 在 `Content::theme` 表达单封 Markdown 邮件的主题，只在本次 HTML 合成中解析 CSS。空值或缺字段用应用主题。队列沿用已有 `Content` 串行化，只存主题名，Worker 在自己的应用目录解析样式。缺字段的既有载荷可正常解码。v0.x 使用 Minijinja 的邮件模板，未发现单封 Markdown 主题契约的可复用代码。 [固定 Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailable.php#L415-L425) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#customizing-the-css)
- 基于 `c3db642d5` 的干净归档验收。在同一个邮件器上直接发送覆盖主题、未指定和空字符串三封邮件。缺失的主题返回合成错误。独立 Artisan Worker 各消费一个来自 MySQL 8.4.13 和 Redis 8.10.3 的主题邮件，队列清空且失败表保持空。五封经 SMTP 投递的邮件通过 Mailpit 1.33.1 的 HTTP API 验证 CSS 和显式文本。三个覆盖主题和两个默认主题的颜色各自可辨。
- 项目 `python3 -B tests/mysql_acceptance.py` 环境下邮件通知套件 16 项通过、2 项夹具忽略。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已复验 800／420 px 投递邮件与 17 项独立发布资源。临时产物已回收，磁盘可用 97.90 GB（91.18 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-theme.LCKkUOgj`。

## 邮件渲染与浏览器预览

- 对照固定 Laravel v13.40.0 的 `Mailable::render` 和浏览器预览。OpenRust 的 `Mailable::render().await` 返回优先选中的 HTML 或纯文本正文，`toResponse().await` 构造带相应媒体类型的响应。预览与实际投递共用正文、视图和主题渲染，具有单封邮件的语言作用域。预览只读取邮件内容，绕过信封、入队、事件及传输。旧版 Minijinja 的 `render_email` 直接组装 MIME，现有 Blade 与 Markdown 合成具备可复用的正文渲染。 [固定 Laravel 渲染](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailer.php#L225-L242) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#previewing-mailables-in-the-browser)
- 基于 `74a47e80e` 的干净归档验收。真实 TCP HTTP 验证无可用传输时的 Markdown HTML、法语页脚、单封主题颜色、Blade 转义、纯文本媒体类型和不存在视图的 500。测试同时验证直接取得正文、语言在调用后恢复，以及 HTML 预览绕过缺失的纯文本备用视图。测试信封方法一旦执行便失败，用于验证预览路径的边界。
- 项目 `python3 -B tests/mysql_acceptance.py` 的 MySQL 8.4.13、真实 Redis 8.10.3 与 Mailpit 1.33.1 下邮件通知套件 17 项通过、2 项夹具忽略。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已复验 800／420 px 投递邮件与 17 项独立发布资源。Rust 路由使用显式的异步响应方法。Laravel 预览中的 CID 内嵌附件转换待 MIME 内嵌附件支持后补齐。临时产物已回收，磁盘可用 97.55 GB（90.85 GiB），周额度剩余 34%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-preview.Ty8qFGMA`。

## 邮件 MIME 内嵌图片

- 对照固定 Laravel v13.40.0 的 `Message::embed`、`embedData` 和 `Mailer::replaceEmbeddedAttachments`。Rust 的 `Mailable::inline_attachments` 声明可串行化的内嵌附件，`cid()` 提供模板引用。发送时，带 HTML 的内嵌图片进入 `multipart/related`，文本位于 `multipart/alternative`。浏览器预览将匹配的图片 `src` 换成 Base64 数据 URL。旧版 Minijinja 邮件未发现可复用的内嵌附件。 [固定 Laravel 内嵌附件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Message.php#L389-L403) · [固定 Laravel 预览转换](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailer.php#L245-L269)
- 基于 `a51f1995c` 的干净归档验收。直接发送及 MySQL 8.4.13、Redis 8.10.3 的独立 Artisan Worker 通过 SMTP 投递至 Mailpit 1.33.1。HTTP 核对原始 MIME 中的混合、相关和替代结构、Content-ID、普通附件及图片 MIME 类型。Chrome 在 Mailpit 预览中成功解码五张 PNG。应用 TCP HTTP 核对预览数据 URL、纯文本路径、未匹配 CID 和非 src 属性。
- 项目 `python3 -B tests/mysql_acceptance.py` 环境下邮件通知套件 17 项通过、2 项夹具忽略。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。已复验 800／420 px 投递邮件与 17 项独立发布资源。结构化供应商 API 使用明确的传输错误；已有原始 MIME 接口可承载此结构。临时产物已回收，磁盘可用 104.17 GB（97.01 GiB），周额度剩余 33%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-inline.Hsxf34HF`。

## 供应商 API 的内嵌图片字段

- 对照固定 Laravel v13.40.0 的 Cloudflare 内嵌附件用例及供应商协议。Postmark 发送带 `cid:` 的 `ContentID`，Resend 和 Cloudflare 发送无前缀的 `content_id`，Cloudflare 同时标注 `inline` disposition。Mailgun 表单通过 CID 命名 `inline` 文件分段，引用原样保留。原始 MIME 使用上一批的附件名称和 MIME 结构。旧版邮件模块未发现对应的供应商内嵌字段。 [固定 Laravel Cloudflare 用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Mail/MailCloudflareTransportTest.php#L154) · [Postmark](https://postmarkapp.com/developer/user-guide/send-email-with-api#inline-image-attachments) · [Resend](https://resend.com/changelog/embed-images-using-cid) · [Mailgun](https://documentation.mailgun.com/docs/mailgun/user-manual/sending-messages/send-attachments)
- 基于 `410e5467e` 的干净归档验收。向本地 TCP HTTP 协议夹具发送四家供应商请求，Cloudflare 和 Mailgun 从独立 Artisan 进入。核对普通附件和内嵌图片并存，两种 JSON CID 前缀格式、Cloudflare disposition、Mailgun 分段名称和二进制原文，以及 HTML 的 CID。供应商账号实投仍待目标环境。
- 项目 `python3 -B tests/mysql_acceptance.py` 提供 MySQL 8.4.13，联同 Redis 8.10.3、Mailpit 1.33.1 复验邮件通知套件 17 项通过、2 项夹具忽略。Chrome 成功解码五封 SMTP 邮件的内嵌 PNG。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项文档浏览器检查通过。独立二进制发布 17 项资源。临时产物已回收，磁盘可用 104.33 GB（97.17 GiB），周额度剩余 33%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-provider-inline.PtioNhdd`。

## Artisan 生成 Markdown 邮件与 Blade 视图

- 对照固定 Laravel v13.40.0 的 `MailMakeCommand` 和 Generating Markdown Mailables。Artisan 生成普通邮件、Blade 或 Markdown 类型，接受显式或默认视图名，Markdown 决定两视图选项并用时的模板类型。`--test` 复用 Rust 配套测试生成器。Rust 使用静态模块，生成后声明模块再构建。默认普通邮件生成可渲染的文本。三个源码 stub 可通过 `stub:publish` 发布和覆盖。旧版 `generate mailer` 使用 Minijinja 的语言目录与登记代码，当前保留 Laravel 的命令职责及源码／模板配套。 [固定 Laravel 生成器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/MailMakeCommand.php) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#generating-markdown-mailables)
- 基于 `8a0f61466` 的干净归档验收。实际 Artisan 校验三种模式、嵌套命名、选择优先级、已有源码和模板处理、配套测试、stub 覆盖、路径穿越和符号链接。独立发布二进制实跑文档的三个命令，重新编译生成的代码并通过 HTTP 内核渲染 Blade 和 Markdown 预览。生成器回归 5 项、生成产物编译／预览 2 项通过。
- 项目 `python3 -B tests/mysql_acceptance.py` 提供 MySQL 8.4.13，联同 Redis 8.10.3、Mailpit 1.33.1 复验邮件通知套件 17 项通过、2 项夹具忽略。视图和发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项文档浏览器检查通过。独立二进制发布 17 项邮件资源。Rust 配套测试使用 `--test`，PHP 测试框架和 Laravel 缺参交互提示留作差异。临时产物已回收，磁盘可用 104.31 GB（97.15 GiB），周额度剩余 33%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-mail-generator.h4nmlZIR`。

## Artisan 生成通知与 Markdown 邮件模板

- 对照固定 Laravel v13.40.0 的 `NotificationMakeCommand` 和 Generating Notifications。普通及 Markdown 生成器输出 Rust `Notification` 实现并选择邮件渠道，支持显式及默认视图名、嵌套 Rust 模块、可发布源码 stub 和 `--test`。明确沿用 Laravel 的通知模板覆盖契约，源码生成成功后写入指定 Markdown 模板。邮件生成器继续保留已有模板。两者共用命名、路径校验、互斥写入及失败回滚，旧版未发现 Laravel 式的通知生成器。 [固定 Laravel 生成器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/NotificationMakeCommand.php) · [固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#generating-notifications)
- 基于 `479a1e941` 的干净归档验收。实际 Artisan 核对普通、Markdown、显式和默认视图、已存在源码、强制覆盖、已编辑模板、stub 提取及路径错误。独立发布二进制运行三语文档示例，重新编译生成的两个通知和配套测试。经 HTTP 内核触发两个生成通知，由真实 SMTP 邮件渠道投递，Mailpit HTTP 核对纯文本和 Markdown 双正文。生成器回归 7 项、生成产物编译／投递 2 项通过。
- 项目 `python3 -B tests/mysql_acceptance.py` 提供 MySQL 8.4.13，联同 Redis 8.10.3、Mailpit 1.33.1 复验邮件通知套件 17 项通过、2 项夹具忽略。视图及发布回归 15 项，全仓格式、严格 Clippy、三语站点和 18 项文档浏览器检查通过。独立二进制发布 17 项邮件资源。Rust 使用静态模块声明与 Rust 特性测试。Laravel 缺参交互提示仍单列。临时产物已回收，磁盘可用 104.27 GB（97.11 GiB），周额度剩余 33%。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-notification-generator.v8OcP1hr`。

## Artisan 邮件与通知生成器的缺参交互

- 对照固定 Laravel v13.40.0 的 `PromptsForMissingInput` 与两个生成器的补问钩子。缺少类型名且能交互时先询问名称，再按命令询问模板。已有选项时只问名称，已有名称时直接生成。旧版的 `generate mailer` 要求用户输入完整参数，可参考其 `--no-interaction` 与非终端检测方式。 [固定 Laravel 交互](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Concerns/PromptsForMissingInput.php#L36) · [邮件生成器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/MailMakeCommand.php#L211) · [通知生成器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/NotificationMakeCommand.php#L144)
- Rust 用 Clap 校验原始选项，终端模式为缺失的必填参数临时放开解析，再按完整命令定义校验用户输入。三语提示使用现有 Artisan 本地化机制。选择采用编号和整行输入，重复提示会要求重新输入无效选项或名称。程序调用、非终端和 `--no-interaction` 保留常规必填参数错误；终端 EOF 取消，不产生源码和模板。
- 基于 `1f0d9e810` 隔离快照，`python3 -B tests/message_generator_terminal.py <main>` 验证三种邮件视图、通知默认和自定义视图名、拒绝生成视图、显式选项、语言、参数异常、帮助和 EOF。真实 `./artisan inspire` 及 `mail_generator`、`resource_generator`、`artisan_entry` 三个回归目标通过。交互验收使用 macOS 的伪终端，Windows 终端未复验。
- Rust 1.94.0 全仓格式、全工作区严格 Clippy、三语站点及两个部署路径的 12 项通用和 12 项正文浏览器检查通过。邮件投递路径沿用既有验收。核心账本维持 66/66。 [邮件文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#generating-markdown-mailables) · [通知文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#generating-notifications)
- 本批隔离编译和文档产物已回收，项目卷可用空间净增 3.48 GB（3.24 GiB），剩余 104.65 GB（97.46 GiB）。提交前周额度剩余 33%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-message-interactive.xyS2DSVQ`。

## 邮件附件的路径、磁盘与字节来源

- 对照固定 Laravel v13.40.0 的 `Attachment::fromPath`、`fromStorage`、`fromStorageDisk`、`fromData`、`as` 和 `withMime`。Rust 用可序列化的 `AttachmentSource` 描述待解析附件，通过 `Mailable::attachment_sources()` 提供，并兼容现有已物化的 `attachments()` 与旧载荷。Rust 用 `r#as` 表达保留字，`fromData` 接收已生成的字节并随任务保存。复核 `openrust-v0.x` `6d1a81924` 邮件模块，旧版只有附件 MIME 组合覆盖，沿用当前的组合及队列机制。 [Laravel Attachment](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Attachment.php) · [Laravel Attachments 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#attachments)
- 文件和存储在组合阶段读取，队列 Worker 可读取入队后生成的文件。S3 复用一次 GET 返回的内容与 MIME 元数据，本地文件从内容和路径推断类型。显式命名和 MIME 覆盖应用在同一个传输组合流程中。生成字节无覆盖时使用 `application/octet-stream`。
- 在隔离基线 `df828c915` 上，用真实 HTTP、SMTP 和 Mailpit HTTP 核对文件名、MIME、二进制内容和原有附件。磁盘文件缺失返回 HTTP 500，未投递部分邮件。MySQL 与 Redis 的独立 Artisan Worker 验证入队时无需访问文件，数据库 Worker 首次读取失败后恢复文件并重试成功。通过实际 AWS SDK 向本地 S3 协议夹具读取对象并验证服务端 MIME。未连接托管 S3。
- `mail_notifications` 18 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。`./artisan inspire`、Rust 1.94.0 格式及全工作区严格 Clippy、三语附件示例编译和运行、站点构建以及两种部署路径的 12 项通用和 6 项附件说明浏览器检查通过。核心账本维持 66/66。URL、上传文件、默认云磁盘与附件断言接口后续逐项对照。
- 临时服务及本批隔离编译和文档产物已回收。项目卷可用空间净增 3.65 GB（3.40 GiB），剩余 102.28 GB（95.26 GiB）。提交前周额度剩余 33%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-attachments.rFf3tElu`。

## 上传文件、默认云磁盘及邮件附件断言

- 按固定 Laravel v13.40.0 实现 `Attachment::fromUploadedFile()`、`fromCloudStorage()` 和邮件附件断言。上传使用客户端 basename，优先使用检测到的 MIME，随后回退到客户端声明的 MIME。Rust 使用已缓冲的上传字节构建可序列化来源，跨请求和 Artisan 进程投递。云附件在构造时选择 `filesystems.cloud`，缺省为 `s3`，Worker 发送时访问该磁盘。复核旧版 `6d1a81924` 的上传模型及邮件模块，未发现同契约的附件构造或断言，沿用当前公开的 `UploadedFile` 和存储机制。 [Laravel Attachment](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Attachment.php#L85) · [Laravel FilesystemManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php#L440)
- `Mailable` 支持路径／来源检查、原始字节、默认磁盘、指定磁盘和无普通附件断言。通过来源、名称和 MIME 比较检查声明，默认磁盘附件的显式 basename 与默认名称等价，兼容旧的物化附件。测试时可在文件尚未写入的情况下断言文件和磁盘来源。 [Laravel Mailable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailable.php#L1446) · [Laravel Mail 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#testing)
- 在隔离基线 `3fc92e01c` 上，以真实 multipart POST 发送 PNG，核对服务端 MIME 优先及文件名去除目录。另一个 POST 将无法辨识的二进制上传入队，由 Redis Artisan Worker 投递，验证客户端声明 MIME 及原始字节。MySQL 与 Redis Worker 同时复验云磁盘配置、入队后文件读取和 MySQL Worker 失败恢复。使用实际 AWS SDK 请求本地 S3 协议夹具，未连接托管 S3。
- `mail_notifications` 18 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。实际 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy、三语断言示例执行、站点构建及两个部署路径的 12 项通用和 6 项附件说明浏览器检查通过。核心账本维持 66/66。URL 来源和 `Attachable` 契约后续对照。
- 临时服务及本批隔离编译和文档产物已回收。项目卷可用空间净增 3.67 GB（3.42 GiB），清理后可用 96.39 GB（89.77 GiB）。提交前周额度剩余 32%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-upload-cloud.SjilBxNX`。

## URL 附件与可附加对象

- 对照固定 Laravel v13.40.0 的 `Attachment::fromUrl()`、`Attachable::toMailAttachment()` 和 Attachments／Attachable Objects 两节。URL 限于绝对 HTTP 和 HTTPS。Rust 通过应用范围的 HTTP 客户端在发送阶段下载，沿用请求超时和重定向机制。默认文件名来自原 URL 路径，下载响应可指定 MIME，名称和类型可覆盖。旧版 `6d1a81924` 邮件模块没有对应的 URL 或可附加对象实现，复用当前附件组合与 HTTP 客户端。 [Laravel Attachment](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Attachment.php#L63) · [Laravel Attachable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Mail/Attachable.php) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#attachable-objects)
- Laravel 在动态附件数组中调用对象转换方法。Rust 用 `Attachable::toMailAttachment()` 显式返回已定型、可序列化的 `AttachmentSource`，领域对象留在应用侧。入队及附件断言保留来源，直到实际投递才发起请求。
- 在等价基线 `761e641b8` 上，用真实 HTTP 端点验证相对重定向、带查询参数的 URL、文件名解码、响应 MIME 和原始字节。独立 MySQL 和 Redis Worker 从任务载荷重新下载，失败响应不会产生邮件。MySQL Worker 遇到 HTTP 503 后保留尝试记录，远端恢复后重试成功。Mailpit HTTP 核对即时与队列投递。HTTPS URL 通过构造验证，HTTPS 下载本轮未单独实测。
- `mail_notifications` 18 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。实际 `./artisan inspire`、Rust 1.94.0 格式及全工作区严格 Clippy、三语示例执行、站点构建及两种部署路径的 12 项通用和 6 项邮件文档浏览器检查通过。核心账本维持 66/66。后续对照邮件 Envelope 的 Tags and Metadata。
- 临时服务及本批隔离编译和文档产物已回收。项目卷可用空间净增 3.67 GB（3.42 GiB），清理后可用 99.16 GB（92.35 GiB）。提交前周额度剩余 32%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-url-attachable.YghUeRKQ`。

## 邮件标签和元数据

- 对照固定 Laravel v13.40.0 的 Tags and Metadata、`Envelope` 装配与标签断言。Rust 在现有 `Envelope` 增加 `tags`、`metadata` 及相应断言，字段缺省兼容旧队列载荷。复核旧版 `6d1a81924`，未发现对应的公开契约和传输映射。采用现有 Rust 信封、MIME 组合及队列序列化流程。 [Laravel Tags and Metadata](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#tags-and-metadata) · [Laravel Mailable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailable.php#L515)
- SMTP 和原始 MIME 输出可重复的 `X-Tag` 和 `X-Metadata-*`。Mailgun 结构化请求使用 `o:tag`／`v:*`。Postmark 在发送前拒绝多个标签并发送 `Tag` 和 `Metadata`。SES v1 和 v2 把信封元数据追加到配置的 `Tags`／`EmailTags`。Resend 发送原生名称值对，元数据作为邮件头。Cloudflare 的邮件头映射保留最后一个标签。 [Postmark](https://github.com/symfony/symfony/blob/v8.0.0/src/Symfony/Component/Mailer/Bridge/Postmark/Transport/PostmarkApiTransport.php) · [Mailgun](https://github.com/symfony/symfony/blob/v8.0.0/src/Symfony/Component/Mailer/Bridge/Mailgun/Transport/MailgunApiTransport.php) · [SES 用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Mail/MailSesTransportTest.php) · [SES v2 用例](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Mail/MailSesV2TransportTest.php)
- 在隔离基线 `12faa39f4` 上，用真实 HTTP 和 SMTP 核对两个标签、元数据以及邮件正文。MySQL 与 Redis 的独立 Artisan Worker 序列化并投递同样的标记。供应商采用本地 HTTP 协议夹具，核对 Mailgun 原始 MIME／结构化请求、Postmark、Resend、Cloudflare、SES v1 和 v2。覆盖多标签被拒和头部非法控制字符，未向第三方账户投递。
- `mail_notifications` 18 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。实际 `./artisan inspire`、Rust 1.94.0 全仓格式与严格 Clippy、三语示例执行及旧信封 JSON 往返、站点构建和 18 项浏览器检查通过。核心账本维持 66/66。下一批核对邮件消息定制回调。
- 临时服务和本批隔离编译及文档产物已回收，项目卷可用空间净增 3.67 GB（3.42 GiB），剩余 98.98 GB（92.18 GiB）。提交前周额度剩余 32%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-metadata.NqZjw7B4`。

## 邮件消息定制回调

- 对照固定 Laravel v13.40.0 的 `Envelope::using` 和 `Mailable::runCallbacks()`。OpenRust 在已渲染正文上按声明顺序执行应用注册的回调，随后读取和组装声明的附件，再进入投递事件。回调可调整信封、邮件头和正文，也可添加附件。MIME 和供应商 API 由同一份草稿组装。复核旧版 `6d1a81924` 邮件模块，未发现同契约回调，复用当前 MailManager、组装和队列流程。 [Laravel Mailable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailable.php#L211) · [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#customizing-the-symfony-message)
- Rust 以注册名和 JSON 数据表达可跨进程保存的回调声明。应用在服务提供者启动时注册，Worker 在每个投递尝试中执行。旧信封缺省为空。预览保留原始正文。重复注册、跨应用未注册、回调失败和非法邮件头通过原有错误路径处理。
- 在隔离基线 `7cb928e9b` 上，通过真实 HTTP 与 SMTP、Postmark 本地协议夹具、真实 MySQL 8.4.13 和 Redis 8.10.3 的独立 Artisan Worker 核对执行顺序、头部、正文和追加附件。数据库 Worker 先因回调失败，再因 URL 附件服务失败而重排，恢复后投递成功。回调失败时未下载附件。双应用隔离及预览回归通过。初版在 Debug Worker 复现栈溢出，将附件解析 future 移到堆后用原栈大小复验成功。Postmark 本轮未使用真实账户。
- `mail_notifications` 19 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。实际 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy、三语示例执行及旧信封 JSON 往返、三语站点与 18 项浏览器检查通过。核心账本维持 66/66。下一批核对邮件内容的测试断言。
- 临时服务及本批隔离编译和文档产物已回收。项目卷可用空间净增 3.86 GB（3.59 GiB），清理后可用 97.01 GB（90.35 GiB）。提交前周额度剩余 31%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-customize.tqga7539`。

## 邮件正文与地址断言

- 对照固定 Laravel v13.40.0 的 Testing Mailable Content、`Mailable` 和 `SeeInOrder`。主题及各地址断言同步读取声明的 `Envelope`，可精确匹配显示名。HTML 与纯文本正文分别支持包含、缺失及顺序匹配。Rust 异步渲染正文，HTML 断言显式接收转义开关。顺序匹配识别数字和命名实体，使用已在锁文件中的 `web_atoms` HTML5 实体表。复核旧版 `6d1a81924`，邮件模块未提供对应断言。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md#testing-mailable-content) · [Laravel Mailable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Mail/Mailable.php) · [Laravel SeeInOrder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Testing/Constraints/SeeInOrder.php)
- 复用当前应用渲染器、语言上下文、Markdown 双正文和内嵌资源预览。只有纯文本的邮件也可经 HTML 断言读取其正文。断言不会读取普通附件、执行消息定制回调、传输或排队。
- 基于隔离基线 `f7f207b59`，真实 HTTP 核对地址、显示名、主题、HTML 逃逸、原始标记、实体解码、非重叠顺序、语言切换及恢复、Markdown 纯文本和纯文本回退。包含逆序、重复匹配数量不足及预期内容缺失的失败路径，数组传输保持空。旧发送路径在 Mailpit SMTP 和 MySQL／Redis 独立 Artisan Worker 上通过复验。
- `mail_notifications` 20 项通过、2 项外部专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式及严格 Clippy、`./artisan inspire`、三语相同示例、VitePress 构建以及根路径和 `/docs/` 的 12 项通用及 6 项正文浏览器检查通过。核心账本维持 66/66。下一批对照 Notifications 的邮件正文和操作按钮构建接口。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 3.86 GB（3.59 GiB），清理后可用 87.36 GB（81.36 GiB）。提交前周额度剩余 30%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-mail-assertions.1UheRv6N`。

## 通知邮件的默认格式与操作按钮

- 对照固定 Laravel v13.40.0 的 Formatting Mail Messages、`SimpleMessage`、`MailChannel` 和默认邮件视图。接通 `MailMessage` 的主题、问候语、结束语、条件正文、操作按钮、消息级别及 mailer 选择。正文按调用顺序分到按钮前后，邮件渠道在缺省时由通知类型标识生成主题，Rust 通过 `Notification::kind()` 获取标识。旧版 `6d1a81924` 未提供同契约构建器。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#formatting-mail-messages) · [SimpleMessage](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Messages/SimpleMessage.php) · [MailChannel](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Channels/MailChannel.php)
- Rust 正文行使用字符串，复用 `Content.with` 传递构建参数，`MailMessage` 的字段与显式 HTML、文本或视图调用兼容。内置 `notifications::email` 通过同一 Markdown 管线生成 HTML 和纯文本，命名插槽直接位于 Blade 组件下。默认问候语、结束语和链接说明使用通知语言。默认模板可通过现有 `vendor:publish --tag=laravel-notifications` 发布并覆盖。
- 在隔离基线 `4309e6711` 上通过真实 HTTP 验证换行合并、条件正文、按钮前后顺序、错误和成功颜色、缺省主题、HTML 转义、翻译和可复制链接。实际 Artisan 发布默认模板后，应用视图覆盖生效。真实 Mailpit SMTP、MySQL 8.4.13 与 Redis 8.10.3 独立 Artisan Worker 复验通知发送及队列正文，核对收件的 HTML 和纯文本。
- `mail_notifications` 21 项通过、2 项专用夹具忽略，`filesystem_local` 9 项和 `view_finder` 5 项通过。Rust 1.94.0 全仓格式、严格 Clippy、`./artisan inspire`、三语相同示例、VitePress 构建以及根路径和 `/docs/` 下 18 项浏览器检查通过。核心账本维持 66/66。后续核对通知邮件的发件人、收件人、附件和消息定制契约。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 4.06 GB（3.78 GiB），清理后可用 92.57 GB（86.21 GiB）。提交前周额度剩余 30%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-notification-message.BQ0hNTWV`。

## 通知邮件的地址、附件与消息定制

- 对照固定 Laravel v13.40.0 通知邮件发件人、收件人、附件、标记和消息回调。旧版 `6d1a81924` 的 `EmailDelivery` 只保存发件人、收件人、主题及正文，沿用主线的可序列化 `PreparedMail`、`Envelope`、附件来源和应用级定制机制。普通通知依 `route_notification_for` 获取收件人，自定义 mailable 可自带信封。 [Laravel 通知](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#customizing-the-sender) · [MailMessage](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Messages/MailMessage.php) · [MailChannel](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Channels/MailChannel.php)
- Rust 用 `Address` 表示可带显示名的邮箱。收件信息可重复追加。附件通过类型化 `AttachmentSource` 声明，旧的已物化附件字段仍可使用。完整结构体字面量需补充 `attachment_sources`，使用构建器或 `..Default::default()` 的调用无需额外字段。用 `withMessage` 声明应用注册的回调名和 JSON 数据，投递端按声明顺序修改邮件草稿。双正文默认结束语使用 Markdown 硬换行，避免纯文本和 HTML 转义输出出现字面量 `<br>`。
- 在隔离基线 `3edc1ca42` 上，经实际 HTTP 和 Mailpit SMTP 核对显示名、抄送、密送、回复地址、原始路径及两种磁盘附件、内存附件、标记和回调。删除文件后即时请求返回 500，仍可入队。独立 MySQL Artisan Worker 的首轮尝试保留任务，恢复文件后重试成功，独立 Redis Worker 同样使用恢复后的附件。三封邮件均检查 MIME 和回调追加的正文及附件，失败表为空。
- `mail_notifications` 22 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式与严格 Clippy、`./artisan inspire`、三语示例和站点构建、双路径 18 项浏览器检查通过。核心账本维持 66/66。后续核对通知邮件的模板构建、预览和优先级。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 3.67 GB（3.42 GiB），清理后可用 92.52 GB（86.16 GiB）。提交前周额度剩余 30%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-notification-envelope.bAog7JqB`。

## 通知邮件的模板、预览与优先级

- 对照固定 Laravel v13.40.0 的通知预览与 Markdown 通知，以及 `MailMessage` 的视图选择、主题和优先级。旧版 `6d1a81924` 使用本地化邮件模板目录，未提供对应通知构建器。复用主线的 `Content`、邮件预览和 MIME 头部。 [Laravel 通知](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#previewing-mail-notifications) · [Laravel MailMessage](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/Messages/MailMessage.php) · [Symfony MIME 邮件优先级](https://github.com/symfony/mime/blob/v8.0.0/Email.php#L219)
- Rust 以两个构建调用组合命名 HTML 和纯文本视图，后提供的数据按键覆盖已有值。显式 `markdown` 选择双正文，`template` 仅影响 Markdown，`theme` 复用已有 Markdown 样式表。`Mailable::render` 和 `toResponse` 负责只读预览。`priority(i32)` 将值限制在 1–5 并写入 `X-Priority`，不影响队列处理顺序。
- 在隔离基线 `a01c48adb` 上，经真实 HTTP 核对 HTML、纯文本、Markdown、显式视图优先、主题、错误状态和无投递预览。真实 Mailpit SMTP 及 MySQL 8.4.13、Redis 8.10.3 的独立 Artisan Worker 分别验证 1、3、5 的 MIME 优先级。用实际 Artisan 生成 Markdown 通知和 Blade 视图，并编译及执行生成源码与三语相同示例。
- `mail_notifications` 23 项通过、2 项专用夹具忽略，`filesystem_local` 9 项和 `mail_generator` 6 项通过。Rust 1.94.0 全仓格式、严格 Clippy、实际 `./artisan inspire`、生成器 POSIX 交互、三语构建及双路径 18 项浏览器检查通过。核心账本维持 66/66。下一批对照通知渠道的短信、Slack 能力和固定文档中的验收边界。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 4.14 GB（3.86 GiB），清理后可用 92.55 GB（86.20 GiB）。提交前周额度剩余 30%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-notification-template.mwqMAkZu`。

## Vonage 短信通知渠道

- 对照固定 Laravel v13.40.0 的 SMS Notifications 及 Laravel Vonage v3.3.4。复用旧版 `6d1a81924` 的国际号码校验和供应商错误脱敏思路。旧版实际短信驱动为 Twilio，本批按 Vonage SMS 表单和 Basic Auth 实现。 [固定 Laravel SMS](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#sms-notifications) · [Vonage 渠道 v3.3.4](https://github.com/laravel/vonage-notification-channel/blob/v3.3.4/src/Channels/VonageSmsChannel.php) · [Vonage SMS API](https://developer.vonage.com/en/api/sms)
- `NotificationRoute::Vonage` 是新增的类型化路由。`to_vonage` 与现有 Rust 通知契约命名一致。消息支持 Unicode、发件人、业务标识、回执 URL。Rust 的 `usingClient` 使用应用配置名称表达可序列化的客户端选择，队列仅保存输入和客户端名称。Worker 从自身配置读取密钥，每次尝试使用一次 HTTP 请求并拒绝重定向。
- 在隔离基线 `48677efdb` 上，通过真实 HTTP 及本地 Vonage 协议服务验证默认／指定客户端凭据、表单、普通／Unicode、缺失／无效路由、非成功业务状态、无结果响应和重定向。真实 MySQL 8.4.13 与 Redis 8.10.3 的独立 Artisan Worker 使用入队后轮换的凭据。MySQL 首次尝试保留任务，重试成功，失败表为空。Mailpit 运行既有邮件回归。运营商送达及真实 Vonage 账户验证待授权测试号码和账户。
- `mail_notifications` 24 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式及严格 Clippy、实际 Artisan `config:show vonage` 和 `inspire`、三语相同示例、站点构建及双路径 18 项浏览器检查通过。核心账本维持 66/66。下一批核对 Slack Bot API 与 Block Kit，使用现有 HTTP 和队列验收入口。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 3.94 GB（3.67 GiB），清理后可用 90.32 GB（84.12 GiB）。提交前周额度剩余 29%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-vonage-notifications.Kf3i1hLI`。

## Slack Bot API 通知与 Block Kit

- 对照固定 Laravel v13.40.0 的 Slack Notifications 及 Laravel Slack v3.8.0。旧版 `6d1a81924` 的框架无 Slack 通知，复用主线的 HTTP 客户端、通知路由、独立 Worker 和加密服务。`to_slack` 与其他 Rust 通知契约一致。 [固定 Laravel Slack](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#slack-notifications) · [SlackChannel v3.8.0](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Slack/SlackChannel.php) · [Slack Bot API](https://docs.slack.dev/reference/methods/chat.postMessage/)
- 实现 Bearer JSON `chat.postMessage`、显式路由／消息／配置默认频道的优先级、Bot 标识和线程选项，以及原始 Block Kit、消息和字段构建、按钮与确认框。JSON 导入追加区块，Rust 用 `toArray` 和 `toBlockKitBuilderUrl` 返回可检查值。应用可通过原始区块或组件 JSON 处理高级选择器参数。外部工作区调用由 `SlackRoute::make` 传入令牌，队列以 `APP_KEY` 加密该值。默认令牌在 Worker 投递时从配置读取。
- 在隔离基线 `58f5e6e41` 上，通过真实 HTTP 和本地 Slack 协议服务验证三种频道来源、Block Kit、确认框、缺失结果、业务拒绝与重定向。MySQL 8.4.13 实测外部工作区令牌入队为密文，独立 Artisan Worker 解密投递并在供应商拒绝后重试成功。Redis 8.10.3 独立 Worker 使用轮换后的默认令牌。Slack 安装、OAuth 授权、实际工作区送达和应用交互回调由获得授权的 Slack 应用验证。
- `mail_notifications` 25 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式及严格 Clippy、实际 Artisan `config:show services.slack.notifications` 和 `inspire`、三语相同示例、站点构建及双路径 18 项浏览器检查通过。核心账本维持 66/66。下一批核对固定 Slack 包的旧 Webhook 路由以及 Block Kit 的剩余可观察边界。
- 本批隔离编译与文档产物已回收。清理前后项目卷可用空间净增 3.95 GB（3.68 GiB），清理后可用 90.28 GB（84.08 GiB）。提交前周额度剩余 29%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-slack-notifications.6070Nedk`。

## Slack 传入 Webhook 路由与队列投递 {#slack-传入-webhook-路由与队列投递}

- 核对固定 Laravel v13.40.0 的 Slack 文档与 Laravel Slack v3.8.0 路由。HTTP(S) 字符串路由选择 Webhook，`SlackRoute::make` 表达 Bot API 的工作区路由。同一 `to_slack` 和 `SlackMessage` 可直接发送 Block Kit，不需单独定义通知类。旧版 `openrust-v0.x` 无 Slack 对应实现，本批复用现有通知、HTTP 和加密服务。 [固定 Laravel Slack](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md#slack-notifications) · [固定路由](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/SlackNotificationRouterChannel.php) · [固定 Webhook 渠道](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Channels/SlackWebhookChannel.php)
- Webhook 直接 POST JSON，使用 URL 凭据，不使用 Bot OAuth token。Worker 从用 `APP_KEY` 保护的载荷还原 URL。沿用 HTTP 客户端的供应商安全边界，接受 HTTPS 与测试用回环 HTTP，拒绝链接内的用户凭据、查询、片段和重定向，错误信息隐藏完整 URL 和供应商响应正文。Bot API 的旧版队列载荷仍可读取。 [Slack 传入 Webhook 协议](https://docs.slack.dev/messaging/sending-messages-using-incoming-webhooks/)
- 隔离基线 `f20dc7ce0` 使用真实 HTTP 和本地协议服务验收 Block Kit、无 Bearer header、URL 校验、HTTP 403、307 与错误脱敏。检查 MySQL 8.4.13 和 Redis 8.10.3 的实际队列载荷只有加密 URL。用独立 Artisan Worker 投递两种存储中的任务，MySQL 首次被拒绝后重试成功，失败表为空。此批以本地协议服务验证，真实工作区的送达仍需授权的 Slack 应用。
- `mail_notifications` 26 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式、严格 Clippy、`./artisan inspire`、三语同源示例编译执行、站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。核心账本维持 66/66。后续核对 Laravel 旧式 Slack 附件消息类型与剩余 Block Kit 构建器。
- 本批隔离编译与文档产物已回收，数据卷清单一致。清理前后项目卷可用空间净增 3.95 GB（3.68 GiB），清理后可用 90.20 GB（84.00 GiB）。提交前周额度剩余 29%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-slack-webhook.YQrzg27n`。

## Slack 旧式附件 Webhook 投递 {#slack-旧式附件-webhook-投递}

- 固定 Laravel Slack v3.8.0 的 Webhook 渠道可投递现代 `Slack\SlackMessage` 与旧式 `Messages\SlackMessage`。核对旧版 `openrust-v0.x`，无对应实现，本批复用现有 Webhook 传输及加密队列。Rust 保留两个类型的命名空间，用 `to_legacy_slack` 表达旧式消息返回类型。Webhook 优先使用该消息，未提供时使用 `to_slack`。分离的 `fieldWith` 表达 PHP 使用同一方法名接受字段闭包的写法。Rust 接收日期时间或显式相对秒数。 [固定 SlackMessage](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Messages/SlackMessage.php) · [固定 SlackAttachment](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Messages/SlackAttachment.php) · [固定 Webhook 渠道](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Channels/SlackWebhookChannel.php)
- 支持消息级别颜色、附件颜色覆盖、命名与自定义字段、Markdown、图片、作者、时间戳、页脚和带样式的链接动作。消息与附件的可选外层字段按固定上游过滤，保留嵌套字段的布尔值和动作的默认样式。传输沿用 Rust 的统一 Webhook 配置，Laravel 独有的任意 Guzzle 请求参数和 PHP 日期区间对象未移植。
- 基于隔离快照 `b2fe0fc52` 使用真实 HTTP 与本地服务比对完整 JSON。验证 Block Kit 路由回退、旧式消息只从 Webhook 投递、HTTP 错误与重定向。MySQL 和 Redis 队列含附件原始字段及加密目标 URL，独立 Artisan Worker 在供应商 403 后重试成功，失败表为空。真实 Slack 工作区显示效果待授权的供应商验收。
- `mail_notifications` 26 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式、严格 Clippy、`./artisan inspire`、三语同源示例编译执行、站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。核心账本维持 66/66。下一批核对余下 Block Kit 构建器及约束。
- 本批隔离编译与文档产物已回收，数据卷清单一致。清理前后项目卷可用空间净增 3.94 GB（3.67 GiB），清理后可用 90.13 GB（83.94 GiB）。提交前周额度剩余 29%，第二窗口无读数。日志保留于 `/private/tmp/openrust-slack-legacy.uRRILLkP`。

## Slack Block Kit 选择器与文本约束 {#slack-block-kit-选择器与文本约束}

- 对照固定 Slack v3.8.0 的 `StaticSelectElement`、`UsersSelectElement`、`ButtonElement` 和文本单测。旧版 `openrust-v0.x` 无对应 Slack 实现，复用主线动作区块、段落附属组件、通知序列化和独立 Worker。两种选择器支持显式 ID、占位文字、初始选项／用户及保留 `false` 的焦点配置。静态选项按原始键覆写并保序，供应商收到过滤后的规范小写值。 [固定选择器](https://github.com/laravel/slack-notification-channel/tree/v3.8.0/src/Slack/BlockKit/Elements/Selects) · [固定选择器测试](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/tests/Slack/Unit/Elements/Selects/StaticSelectElementTest.php)
- `tryActionsBlock` 传播未知初始选项。Rust 的选择器入口把文本参数用于占位文字，固定 PHP 构建器需要显式调用 `placeholder`。默认选择器 ID 使用带上游前缀的 UUID。按钮生成的 ID 对齐 Laravel 的连字符 slug，显式 ID 保持原值。类型化文本按固定字节上限截断并保留 UTF-8 字符边界。空文本和过长的动作标识及按钮参数在生成载荷时返回错误。纯文本只发送 `emoji`，Markdown 只发送 `verbatim`。 [固定按钮测试](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/tests/Slack/Unit/Elements/ButtonElementTest.php) · [固定文本对象](https://github.com/laravel/slack-notification-channel/blob/v3.8.0/src/Slack/BlockKit/Composites/TextObject.php)
- 基于隔离快照 `05305abd1`，真实 HTTP 验证两个选择器的 JSON、按钮与确认框及未知初始选项在发送前失败。MySQL 和 Redis 的独立 Artisan Worker 验证选择器随队列载荷投递，MySQL 在供应商拒绝后重试。文档示例另外通过本地 Webhook 协议服务。实际 Slack 回调和展示由授权工作区验收。
- `mail_notifications` 27 项通过、2 项专用夹具忽略，`filesystem_local` 9 项通过。Rust 1.94.0 全仓格式、严格 Clippy、`./artisan inspire`、三语同源示例编译执行、站点构建及根路径和 `/docs/` 的 18 项浏览器检查通过。核心账本维持 66/66。下一批核对标题、分隔线和图片区块的回调配置及约束。
- 本批隔离编译与文档产物已回收，数据卷清单一致。清理前后项目卷可用空间净增 3.94 GB（3.67 GiB），清理后可用 90.10 GB（83.92 GiB）。提交前周额度剩余 29%，第二窗口无读数。日志保留于 `/private/tmp/openrust-slack-block-kit.SFgo5GfV`。
