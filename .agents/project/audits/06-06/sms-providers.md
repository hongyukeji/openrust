# 自定义短信供应商验收

## 契约

以固定 Laravel v13.40.0 的通知 `ChannelManager::extend` 和通用 `Manager::driver` 为依据。OpenRust 通过应用容器注册类型化的 `SmsDriver` 工厂，并以 `Option<&str>` 表达默认或显式选择。`SmsChannel` 通过现有自定义通知渠道注册，应用的派发端和 Worker 共用同一服务提供者。旧版 `v0.x` 的 `vendor/openrust/framework/src/sms.rs` 提供了驱动抽象与国际号码校验的参考。主线复用现有 Vonage 传输，保留 `vonage` 渠道及其既有负载。 [固定通知管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/ChannelManager.php) · [固定通用管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php)

## 实际结果

- 用 Rust 1.94.0 执行 `cargo build --locked --example sms`，构建[示例应用](../../../../examples/sms/main.rs)。通过[验收入口](../../../../tests/sms_example.py)运行示例二进制的 `artisan sms:demo`、`artisan migrate --force` 和 `artisan queue:work <连接> --once --tries=1`。
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
