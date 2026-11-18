# 短信

统一短信是 OpenRust 借鉴 Laravel [文件存储](filesystem.md)机制提供的扩展。`Storage::disk` 按名称选择由已注册驱动支持的磁盘，`Sms::driver` 同样按名称选择由已注册供应商驱动支持的短信配置。多个配置可以复用同一驱动，并分别设置参数。

## 内置供应商

在应用环境中通过 `SMS_DRIVER` 选择默认配置，并给应用和 Worker 配置该供应商的凭据。内置与自定义供应商都使用同一个 `Sms::driver` 入口。

| 配置 | 支持的内容 | 环境变量 |
| --- | --- | --- |
| `log`（默认） | 纯文本和模板。应用日志记录号码尾号和消息类型。 | 无 |
| `vonage` | 纯文本 | `VONAGE_KEY`、`VONAGE_SECRET`、`VONAGE_SMS_FROM` |
| `twilio` | 纯文本和厂商审核模板，模板使用具名变量 | `SMS_TWILIO_ACCOUNT_SID`、`SMS_TWILIO_API_KEY_SID`、`SMS_TWILIO_API_KEY_SECRET`，以及 `SMS_TWILIO_FROM` 或 `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | 中国内地审核模板，以及通过 SendMessageToGlobe 发送国际纯文本 | `SMS_ALIYUN_ACCESS_KEY_ID`, `SMS_ALIYUN_ACCESS_KEY_SECRET`. 内地模板用 `SMS_ALIYUN_SIGN_NAME`。国际文本可设 `SMS_ALIYUN_SENDER_ID` 和 `SMS_ALIYUN_MESSAGE_TYPE`（`OTP`、`NOTIFY`、`MKT`）。 |
| `tencent` | 厂商审核模板，使用按顺序排列的变量 | `SMS_TENCENT_SECRET_ID`、`SMS_TENCENT_SECRET_KEY`、`SMS_TENCENT_SDK_APP_ID`、按需设置 `SMS_TENCENT_SIGN_NAME`，以及 `SMS_TENCENT_REGION`（默认 `ap-guangzhou`） |
| `smsbao` | 内地和国际文本 | `SMS_SMSBAO_USERNAME`, `SMS_SMSBAO_API_KEY` |
| `yunpian` | 内地文本或具名模板，以及国际文本 | `SMS_YUNPIAN_API_KEY` |

## 发送短信

日常发送使用默认配置。业务需要指定某个供应商或账号时，可以直接传入配置名。

```rust
use openrust::support::facades::Sms;

Sms::send_text("+85255550123", "Your order has shipped.").await?;

Sms::driver("aliyun")?
    .send_template("+8613800138000", "login_code", [("code", "123456")])
    .await?;
```

默认的 `log` 驱动记录消息类型和号码末四位。需要真实发送时，将 `SMS_DRIVER` 设为相应供应商或具名配置。

## 配置

使用阿里云时，设定 `SMS_DRIVER=aliyun`，填写 `SMS_ALIYUN_ACCESS_KEY_ID` 和 `SMS_ALIYUN_ACCESS_KEY_SECRET`。内地模板需要 `SMS_ALIYUN_SIGN_NAME`，国际文本按目标市场要求填写 `SMS_ALIYUN_SENDER_ID`。省略 `SMS_ALIYUN_MARKET` 时，同一个配置可以向两个市场发送，驱动根据收件号码选择接口和端点。国际及港澳台号码使用 `+` 和国家或地区区号。

在配置的 `templates` 下将业务模板名映射到供应商模板 ID。腾讯云这类按顺序接收变量的接口，可以用包含 `id` 和 `parameter_order` 的配置，将具名变量转换成厂商要求的顺序。各市场独有的参数写在 `markets.mainland` 或 `markets.international` 下，共用凭据填在配置外层。

下例是现有 `aliyun` 配置的补充字段。仅在使用自定义端点时设置 `SMS_ALIYUN_GLOBAL_ENDPOINT`。如果某个市场使用独立账号，可在相应市场下覆盖两个凭据字段。

```rust
"templates": {
    "login_code": "SMS_123",
},
"markets": {
    "international": {
        "endpoint": get(env, "SMS_ALIYUN_GLOBAL_ENDPOINT", Value::Null)?,
    },
},
```

驱动会在发起请求前校验消息类型。阿里云内地短信需要审核通过的签名和模板，这里接入的国际接口接收纯文本。业务已经持有供应商模板 ID 时，可以通过 `SmsMessage::template` 构造消息并调用驱动的 `send`。

## 通知和 Worker

用 `ChannelManager::extend` 将 `SmsChannel::default()` 注册为 `sms`，也可以通过 `SmsChannel::using("aliyun")` 绑定具名配置。通知可用 `to_sms` 提供正文，或用 `to_sms_template` 提供供应商模板。想用配置中的业务模板名时，实现 `to_sms_message` 并返回 `SmsMessage::named_template`。接收号码使用 `sms` 路由键下的 `NotificationRoute::Sms`。

队列保存选中的配置名和消息。Worker 使用相同的渠道和配置名，从自身配置解析模板并读取凭据。Redis 队列可用 `./artisan queue:work redis` 启动 Worker。配置改名前先消费旧任务，或保留原名称直到旧任务完成。修改配置后，如已启用配置缓存，需重新生成并重启常驻 Worker。

## 注册供应商 {#custom-sms-providers}

在应用服务提供者的 `register` 中解析 `SmsManager`，调用 `extend` 注册工厂。工厂接收选中的配置，返回实现 `SmsDriver` 的驱动。多个具名配置可以复用同一驱动。内置的 `vonage` 通知渠道继续使用原有的 `VONAGE_*` 环境变量。
