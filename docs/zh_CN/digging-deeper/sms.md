# 短信

统一短信是 OpenRust 借鉴 Laravel [文件存储](filesystem.md)机制提供的扩展。`Storage::disk` 按名称选择由已注册驱动支持的磁盘，`Sms::driver` 同样按名称选择由已注册供应商驱动支持的短信配置。多个配置可以复用同一驱动，并分别设置参数。

## 内置供应商

在应用环境中通过 `SMS_DRIVER` 选择默认配置，并给应用和 Worker 配置该供应商的凭据。内置与自定义供应商都使用同一个 `Sms::driver` 入口。

| 配置 | 支持的内容 | 环境变量 |
| --- | --- | --- |
| `vonage`（默认） | 纯文本 | `VONAGE_KEY`、`VONAGE_SECRET`、`VONAGE_SMS_FROM` |
| `twilio` | 纯文本和厂商审核模板，模板使用具名变量 | `SMS_TWILIO_ACCOUNT_SID`、`SMS_TWILIO_API_KEY_SID`、`SMS_TWILIO_API_KEY_SECRET`，以及 `SMS_TWILIO_FROM` 或 `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | 中国内地审核模板，使用具名变量 | `SMS_ALIYUN_ACCESS_KEY_ID`、`SMS_ALIYUN_ACCESS_KEY_SECRET`、`SMS_ALIYUN_SIGN_NAME` |
| `aliyun_international` | 国际及港澳台自由文本，使用 SendMessageToGlobe | `SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID`、`SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET`。可选 `SMS_ALIYUN_INTERNATIONAL_SENDER_ID` 和 `SMS_ALIYUN_INTERNATIONAL_MESSAGE_TYPE`（`OTP`、`NOTIFY`、`MKT`） |
| `tencent` | 厂商审核模板，使用按顺序排列的变量 | `SMS_TENCENT_SECRET_ID`、`SMS_TENCENT_SECRET_KEY`、`SMS_TENCENT_SDK_APP_ID`、按需设置 `SMS_TENCENT_SIGN_NAME`，以及 `SMS_TENCENT_REGION`（默认 `ap-guangzhou`） |

国际及港澳台号码请使用带 `+` 和国家或地区区号的格式。`aliyun` 配置用于中国内地审核模板，接受 11 位内地手机号码并补上 `+86`。`aliyun_international` 配置用于国际及港澳台自由文本，使用独立凭据，默认连接阿里云新加坡端点。腾讯云和 Twilio 可用各自的 `SMS_*_MARKET` 环境变量选择 `mainland` 或 `international`。请为目标市场及配置选择正确的发件人和审核模板。供应商受理和手机实际收信是两个状态。

## 纯文本与模板

用 `SmsMessage::new` 构造纯文本，用 `SmsMessage::template` 传入厂商审核模板。通知可以从 `to_sms` 返回纯文本，或从 `to_sms_template` 返回模板。遇到不支持的消息类型或变量格式时，驱动会在提交前报错。

```rust
use openrust::{
    notifications::sms::{SmsMessage, SmsTemplate},
    support::facades::Sms,
};

let template = SmsTemplate::named("SMS_123", [("code".into(), "123456".into())].into());
Sms::driver(Some("aliyun"))?
    .send(SmsMessage::template("+8613800138000", template)?)
    .await?;

let template = SmsTemplate::positional("1234", vec!["123456".into(), "5".into()]);
Sms::driver(Some("tencent"))?
    .send(SmsMessage::template("+85255550123", template)?)
    .await?;

Sms::driver(Some("aliyun_international"))?
    .send(SmsMessage::new("+85255550123", "Your order is ready.")?)
    .await?;

Sms::driver(Some("twilio"))?
    .send(SmsMessage::new("+12025550123", "Your order is ready.")?)
    .await?;
```

## 注册供应商 {#custom-sms-providers}

通过 `Sms::driver(None)` 选择默认配置，或用 `Sms::driver(Some("backup"))` 显式指定。直接发送时调用 `driver.send(SmsMessage::new(number, text)?).await?`。在应用服务提供者的 `register` 中解析 `SmsManager`，用 `extend` 注册每个驱动。工厂接收选中的配置并返回实现 `SmsDriver` 的驱动。`sms.drivers` 下的配置名称可以与注册的驱动名称不同。

用 `ChannelManager::extend` 将 `SmsChannel::default()` 注册为 `sms`，用 `SmsChannel::using("backup")` 注册固定使用备用配置的渠道。在 `Notification::via` 中选择已注册渠道，由 `to_sms` 返回短信正文，并在 `sms` 路由键下用 `NotificationRoute::Sms` 指定号码。派发进程和 Worker 使用相同的服务提供者和渠道注册。现有 `vonage` 渠道仍可使用。

## 配置

以下配置默认选择 `primary`。令牌通过环境变量传入，应用运行时将其加载到对应配置。

```json
{
  "sms": {
    "default": "primary",
    "drivers": {
      "primary": {"driver": "demo-alpha", "endpoint": "http://127.0.0.1:8767/alpha"},
      "backup": {"driver": "demo-beta", "endpoint": "http://127.0.0.1:8767/beta"}
    }
  }
}
```

## 运行示例

从仓库根目录运行完整示例。先在一个终端启动模拟端点。

```sh
python3 -B examples/sms/mock.py
```

再在另一个终端启动独立 Redis，并执行示例应用的 Artisan 命令。

```sh
cargo build --locked --example sms
docker run -d --rm --name openrust-sms-demo -p 127.0.0.1:16379:6379 \
  --tmpfs /data redis:8 redis-server --save '' --appendonly no

export APP_BASE_PATH="$(mktemp -d)"
export APP_ENV=local CACHE_STORE=array QUEUE_CONNECTION=redis QUEUE_FAILED_DRIVER=null
export REDIS_HOST=127.0.0.1 REDIS_PORT=16379 REDIS_QUEUE=sms-demo
export SMS_ALPHA_URL=http://127.0.0.1:8767/alpha SMS_ALPHA_TOKEN=demo-alpha-token
export SMS_BETA_URL=http://127.0.0.1:8767/beta SMS_BETA_TOKEN=demo-beta-token

SMS_DRIVER=primary target/debug/examples/sms artisan sms:demo +12025550123 "First"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Second"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Third" --profile primary
SMS_DRIVER=backup target/debug/examples/sms artisan queue:work redis --stop-when-empty --tries=1
```

模拟端点会依次打印 `/alpha`、`/beta` 和 `/alpha`。为 `sms:demo` 加上 `--now` 可试用门面的直接发送。队列保存配置名称、号码和正文。Worker 从自己的配置读取端点和凭据。派发后修改 `sms.default`，已入队任务仍使用派发时选中的配置，需保留该配置直到任务完成。

用 Ctrl+C 停止模拟服务，再运行 `docker stop openrust-sms-demo`。模拟端点只接收本地 HTTP 请求，不发送真实短信。
