# 自定义短信供应商验收

## 契约

以固定 Laravel v13.40.0 的通知 `ChannelManager::extend` 和通用 `Manager::driver` 为依据。OpenRust 通过应用容器注册类型化的 `SmsDriver` 工厂，并以 `Option<&str>` 表达默认或显式选择。`SmsChannel` 通过现有自定义通知渠道注册，应用的派发端和 Worker 共用同一服务提供者。旧版 `v0.x` 的 `vendor/openrust/framework/src/sms.rs` 提供了驱动抽象与国际号码校验的参考。主线复用现有 Vonage 传输，保留 `vonage` 渠道及其既有负载。 [固定通知管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/ChannelManager.php) · [固定通用管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php)

## 实际结果

- 用 Rust 1.94.0 执行 `cargo build --locked --example sms`，构建示例应用（当时位于 `examples/sms/main.rs`）。通过当时的验收入口 `tests/sms_example.py`运行示例二进制的 `artisan sms:demo`、`artisan migrate --force` 和 `artisan queue:work <连接> --once --tries=1`。
- 独立 MySQL 8.4.13 与 Redis 8.10.3 各通过两次直接发送和四次独立 Worker 投递。逐条读取两种存储的实际队列载荷，短信正文、号码和选定的配置名一致。端点和凭据均未入队。Worker 的默认配置特意设为与任务已选配置相反，实际仍投递到任务选中的模拟供应商，并使用 Worker 专用凭据。
- 现有 Vonage 的 `vonage_protocol_and_worker_retries_use_the_notification_route` 定向回归通过，使用真实 MySQL 及 Redis Worker 和本地协议端点。两家示例供应商也只通过本地 HTTP 验证，未进行真实短信投递。
- `cargo +1.94.0 fmt --all --check` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。三语文档使用 `npm --prefix docs run build` 构建，在 `/` 与 `/docs/` 下通过 12 项通用浏览器检查和 6 项新章节检查。文档的 `queue:work redis --stop-when-empty --tries=1` 实际向 `/alpha`、`/beta`、`/alpha` 依次投递并正常退出。
- 已清理本批临时容器、隔离编译和文档产物，数据卷清单一致。项目磁盘可用量净增 3.45 GB，剩余 85.56 GB。原始日志保留于 `/private/tmp/openrust-sms.8ejsv5mh`。

## 内置厂商、模板和市场配置

- 保留 Laravel 的通知渠道注册以及文件存储式具名驱动。对照 EasySms 的自定义网关与 SMS4J 的具名厂商配置。参考 OpenSpring 的纯文本与厂商模板区分，并沿用 `openrust-v0.x` 的 Twilio 请求字段和凭据脱敏思路。Rust 用 `SmsTemplate::named` 和 `SmsTemplate::positional` 明确参数形态。 [EasySms](https://github.com/overtrue/easy-sms) · [SMS4J](https://github.com/dromara/SMS4J)
- 新增阿里云 SendSms（ACS3 签名，具名模板变量）、腾讯云 SendSms（TC3 签名，位置模板变量）、Twilio（Basic Auth，纯文本或具名模板变量）。保留 Vonage 文本通知渠道。配置选择中国内地或国际／港澳台市场，可用具名配置分别指定账号和签名。运营商送达、生产账号权限、审核模板和各市场的发送限制待实测。阿里云国际自由文本 API 见下节。 [阿里云签名](https://www.alibabacloud.com/help/en/sdk/product-overview/v3-request-structure-and-signature) · [腾讯云 SendSms](https://cloud.tencent.com/document/product/382/55981) · [Twilio Messages](https://www.twilio.com/docs/messaging/api/message-resource)
- `cargo +1.94.0 test --locked --offline -p openrust-framework --test sms_providers` 通过。回环 HTTP 服务校验签名、发送字段、Unicode、市场及消息类型限制、返回结果和模板队列编解码。现有纯文本负载格式一致。根应用配置测试与隔离快照的全仓格式、严格 Clippy 通过。
- 真实 MySQL 8.4.13 与 Redis 8.10.3 复跑旧示例，两个后端各通过 2 次直接发送和 4 次独立 Artisan Worker 投递，并检查凭据不进队列。现有 Vonage 定向 Worker 回归通过。新增厂商的 HTTP 请求指向本地协议端点。
- 三语短信页面构建通过，根路径和 `/docs/` 下通过 12 项通用及 6 项短信页面浏览器检查。本批隔离产物和容器已回收，数据卷清单相同。项目卷可用空间净增 3.94 GB，剩余 87.35 GB。原始日志保留于 `/private/tmp/openrust-sms-builtins.pbCjYW2Z`。

## 阿里云国际与港澳台自由文本

- `aliyun` 固定走内地模板 SendSms（2017-05-25），`aliyun_international` 固定走国际自由文本 SendMessageToGlobe（2018-05-01），默认新加坡端点，两组账号分别配置。两条路径共用 ACS3 请求签名和 HTTP 边界。对照官方 SDK 的 HTTPS POST、根路径和查询参数，国际发送读取 `ResponseCode` 与 `MessageId`。 [国际 API](https://help.aliyun.com/en/sms/developer-reference/api-dysmsapi-2018-05-01-sendmessagetoglobe) · [官方 Python SDK](https://github.com/aliyun/alibabacloud-python-sdk/blob/master/dysmsapi-20180501/alibabacloud_dysmsapi20180501/client.py#L424-L456)
- 用独立 MySQL 8.4.13 与 Redis 8.10.3 执行 `python3 -B tests/mysql_acceptance.py -- python3 -B tests/sms_example.py <sms-example-binary>`。两个后端各通过 2 笔阿里云真实 Artisan Worker 投递。派发端使用不同的凭据和端点，实际队列仅含原消息和选定配置名。Worker 默认值反向切换后仍按任务配置投递，本地接收端按账户分别重算签名。直接发送覆盖香港、澳门、台湾及美国号码、跨市场拒绝、内容类型拒绝、业务拒绝和缺失消息 ID。真实厂商账号、审核配置和运营商送达留待获得测试账号后验收。
- 双供应商示例与现有 Vonage 定向 Worker 回归、框架协议及配置定向测试通过。Rust 1.94.0 全仓格式和严格 Clippy 通过。三语页面已更新，通过三语构建、12 项通用浏览器检查与 6 项阿里云页面检查。
- 已回收本批隔离编译、站点及容器资源，数据卷清单一致。项目卷可用空间净增 3.74 GB，剩余 84.49 GB。原始日志保留于 `/private/tmp/openrust-sms-international.WA138c5J`。

## 短信宝与云片网络

- `smsbao` 和 `smsbao_international` 分别接入 HTTPS GET `/sms` 和 `/wsms`，提交完整短信正文并按明文数字结果判定。复核 `openrust-v0.x` 的短信驱动，旧版无对应厂商适配。参考 OpenSpring 短信宝实现，将用户名和 API 凭据放在具名配置中，业务错误仅保留数字码。 [短信宝内地接口](https://www.smsbao.com/docs/developer/domestic-sms-api) · [短信宝国际接口](https://user.smsbao.com/docs/developer/international-sms-api/)
- `yunpian` 支持内地正文匹配和指定模板，`yunpian_international` 支持国际正文匹配。指定模板按云片协议为具名变量加分隔符并分别编码，再按表单发送。保留返回的 64 位 `sid`。 [云片内地单发](https://global.yunpian.com/official/document/sms/zh_CN/domestic_single_send) · [云片指定模板](https://global.yunpian.com/official/document/sms/zh_CN/domestic_tpl_single_send) · [云片国际单发](https://global.yunpian.com/official/document/sms/zh_CN/intl_single_send)
- 使用 `cargo +1.94.0 build --locked --offline --example sms` 和 `python3 -B tests/mysql_acceptance.py -- python3 -B tests/sms_example.py <sms-example-binary>`。独立 MySQL 8.4.13、Redis 8.10.3 各通过 4 笔新增配置的真实 Artisan Worker 投递。本地协议端点核对 Unicode、加号和模板双层编码、号码与市场限制、厂商拒绝和拒绝重定向。检查实际队列只含具名配置和消息，端点及认证信息由 Worker 自身配置读取。阿里云、Vonage 和已有自定义供应商回归通过。
- 隔离快照格式、全目标严格 Clippy、三语构建、12 项通用及 6 项短信页面浏览器检查通过。已清理本批容器和编译、文档产物，数据卷清单一致。项目卷可用空间净增 3.68 GB，清理后剩余 84.52 GB。原始日志保留于 `/private/tmp/openrust-sms-cn-providers.Ljqv0bNy`。真实厂商账号、签名、模板与手机收信待验收。

## 默认与具名短信配置

- 采用文件存储的具名配置模式。根应用保留 `config/sms.rs`、`SMS_DRIVER`、默认 Vonage 和原有三个 `VONAGE_*` 环境变量。`.env.example` 只有默认配置示例。三语文档提供云片单账号、附加 `orders_cn` 配置与显式选择示例。 [Laravel 文件存储管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php)
- 隔离 MySQL 8.4.13、Redis 8.10.3 分别验证单账号默认投递、两个配置共用云片驱动时的默认与显式选择、每个后端 2 笔真实独立 Artisan Worker 投递。派发端和 Worker 使用不同密钥及端点，任务只保存选定的配置名称和消息。切换 Worker 的默认配置后仍按任务持有的名称投递。阿里云、短信宝、云片、Vonage 和自定义驱动的已有回归通过。供应商协议在本地 HTTP 端点验证。
- 执行 `cargo +1.94.0 build --locked --offline --example sms`、`python3 -B tests/mysql_acceptance.py -- python3 -B tests/sms_example.py <sms-example-binary>`，并通过根应用配置回归、全仓格式、严格 Clippy、三语构建及 18 项浏览器检查。临时容器和编译产物已清理，数据卷清单相同。项目卷可用空间净增 3.69 GB，剩余 83.43 GB。日志位于 `/private/tmp/openrust-sms-config.iiMVlqPO`。

## 单一供应商名称与默认日志驱动

- 阿里云、短信宝、云片各保留一个内置驱动。用各自的 `SMS_*_MARKET` 选择市场，单账号只需一组凭据。同时使用多个市场或账号时，可为同一个驱动增加独立的具名配置。现有队列继续按派发时保存的名称解析配置，切换名称前须处理完待办任务或在 Worker 保留原名称。
- 默认 `log` 驱动通过应用日志记录号码尾号和消息类型，省略完整号码、正文、模板 ID 和参数。Vonage 可继续通过原有渠道或显式选择对应短信驱动发送。
- 根配置测试、框架 `sms_providers` 测试、三语 VitePress 构建通过。独立 MySQL 8.4.13 和 Redis 8.10.3 的真实 Artisan Worker 验收通过。回环 HTTP 端点核对三家供应商的内地与国际路径、独立 Worker 凭据和队列负载。默认 `log` 的直发与队列模板均写入脱敏日志，队列在 Worker 默认配置改成 `aliyun` 后仍使用 `log`。真实厂商投递及手机收信待验收。
- 本批独立容器、编译与文档产物已回收，数据卷清单一致。项目卷可用空间净增 3.07 GB，清理后剩余 81.08 GB。原始日志保留于 `/private/tmp/openrust-sms-aliyun-config.futGSBF7`。

## 按号码选择市场与简化发送接口

- 内置供应商默认按号码选择市场，保留显式市场限制。共用凭据填在配置外层，各市场可覆盖端点、凭据、发送身份和模板。覆盖只在对应市场生效。
- `Sms::driver("aliyun")` 返回具名发送入口。默认配置可用 `Sms::send_text`、`Sms::send_template` 或 `Sms::default_driver`。业务模板从选定市场和配置读取，队列保留模板名和参数，Worker 从自己的配置解析。既有厂商模板 ID 和通知负载继续可用。
- 原 `examples/` 的验收程序已迁入 [Worker 测试目录](../../../../tests/support/sms/worker_app.rs)。[验收脚本](../../../../tests/sms_worker.py)通过独立 MySQL 8.4.13 和 Redis 8.10.3 验证同一个阿里云配置的内地模板、国际纯文本、共享凭据和按市场覆盖凭据，以及具名模板入队和 Worker 消费。短信宝、云片两市场及默认日志驱动回归通过。本地 HTTP 校验实际请求，真实厂商投递待验收。
- Rust 1.94.0 的根配置测试、`sms_providers` 测试、全仓格式和严格 Clippy 通过。三语 VitePress 构建通过。临时容器、编译和站点产物已清理，数据卷清单一致。项目卷可用空间净增 3.59 GB，剩余 82.51 GB。日志保留于 `/private/tmp/openrust-sms-unified.UycW0r1v`。

## 市场路由、提交结果与队列保护

- 自动市场要求收件号码带国家或地区区号。明确限制为内地时仍可传内地本地号码。业务模板可按市场选择厂商模板 ID 或渲染正文，缺少变量时在发起请求前停止。
- HTTP 请求事件和记录器中的短信请求快照移除查询串、正文及认证头。短信宝实际请求继续按厂商协议发送。通知可选择加密队列载荷及派发条件，Worker 兼容旧有未加密任务。选用加密时，派发端和 Worker 使用同一应用密钥。
- 短信 Worker 只为明确允许重试的错误继续执行。HTTP 429 及 Vonage 明确限流可按任务策略重试。响应丢失、混合受理结果和永久失败进入失败记录。成功提交记录先写日志，后续通知监听器失败单独记录。
- 发送便捷方法和通知事件提供类型化提交结果。应用在验证厂商回执或主动查询后，可按供应商与消息 ID 更新送达状态。Vonage 统一发送入口按 GSM 字符表选编码。云片内地文档使用其签名括号。 [Vonage SMS API](https://developer.vonage.com/en/api/sms) · [云片签名与模板](https://global.yunpian.com/official/document/sms/zh_CN/clk9wf)
- 真实厂商账号、实际运营商送达及应用的回执验签入口待发布环境验证。Worker 在厂商受理后被强制终止且队列还未确认时，队列重投仍可能产生重复发送。跨进程崩溃的去重需要可持久化的发送记录及厂商查询能力。
- 隔离 MySQL 8.4.13、Redis 8.10.3 和真实 Artisan Worker 验证裸号码拒绝、按市场切换厂商模板和正文、短信宝 HTTP 请求脱敏、加密队列及失败记录。模拟厂商断开响应后确认只有一次提交。模拟已受理后的通知监听器失败，日志先记录受理且 Worker 不重发。HTTP 429 和 Vonage 状态码 1 均可按策略重试。
- 16 项定向测试通过，涵盖 HTTP 客户端、厂商协议、类型化回执更新、Vonage GSM 字符和旧渠道 Worker。隔离快照的 Rust 1.94.0 格式、全仓严格 Clippy、三语站点构建及 12 项浏览器检查通过。清理编译及文档产物和临时容器后，数据卷清单一致，项目卷可用空间净增 3.99 GB，剩余 101.51 GB。原始日志位于 `/private/tmp/openrust-sms-hardening.XHwTYDlK`。

## 签名回执与受理后 Worker 崩溃的本机模拟

- Twilio Python SDK 9.13.0 在本机 HTTP 模拟接收端校验表单签名。正确签名的匹配回执通过框架接口将状态由 `accepted` 更新为 `delivered`。篡改签名数据返回 403，错误消息 ID 和冲突终态返回 409。重复回执仍返回 204，现有状态一致。验签入口只在隔离夹具中。真实供应商回执和手机收信待实测。 [Twilio 官方验签说明](https://www.twilio.com/docs/usage/security#validating-requests)
- MySQL 8.4.13 和 Redis 8.10.3 上运行真实 Artisan Worker。在模拟厂商返回受理且应用写出受理日志后，由通知监听器阻塞确认，再发送 SIGKILL。保留窗口到期后，新 Worker 对相同任务再次调用厂商。两个后端各捕获两次提交，确认仍存在受理后、队列确认前的重复发送窗口。
- 临时容器、构建、SDK 和隔离源码副本已清理，数据卷清单一致。磁盘可用量净增 2.58 GB，剩余 93.33 GB。原始结果、探针和夹具补丁保存在 `/private/tmp/openrust-sms-simulation.o52xPJ3x`。
