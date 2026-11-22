# 短信

统一短信是 OpenRust 借鉴 Laravel [文件存储](filesystem.md)机制提供的扩展。`Storage::disk` 按名称选择由已注册驱动支持的磁盘，`Sms::driver` 同样按名称选择由已注册供应商驱动支持的短信配置。多个配置可以复用同一驱动，并分别设置参数。

应用在 `config/sms.rs` 中提供默认的 `log` 配置。自定义配置加载器替换短信设置时，必须提供 `default` 及 `drivers` 下对应的具名配置；缺失设置会返回错误。

## 驱动生命周期 {#sms-driver-lifecycle}

`SmsManager` 沿用 Laravel 的具名驱动模式。同一应用首次解析配置时创建驱动，后续调用复用该驱动及对应模板设置；不同配置、不同应用各自独立。调用 `default_driver` 或默认发送方法时读取当前默认配置名。

更新配置或注册替换工厂后，清除已解析驱动，再重新获取：

```rust
Sms::forgetDrivers()?;
let sender = Sms::driver("aliyun")?;
```

`Sms::getDefaultDriver()` 返回当前默认配置名，不创建驱动。`forgetDrivers` 影响之后的解析，已持有的 `SmsSender` 保留原驱动和设置。驱动创建失败后可以重试。常驻 Worker 修改配置后，按需重新生成配置缓存并重启 Worker。

## 内置供应商

在应用环境中通过 `SMS_DRIVER` 选择默认配置，并给应用和 Worker 配置该供应商的凭据。内置与自定义供应商都使用同一个 `Sms::driver` 入口。

| 配置 | 支持的内容 | 环境变量 |
| --- | --- | --- |
| `log`（默认） | 纯文本和模板。应用日志记录号码尾号和消息类型。 | 无 |
| `vonage` | 纯文本 | `SMS_VONAGE_API_KEY`、`SMS_VONAGE_API_SECRET`、`SMS_VONAGE_FROM` |
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

### 开发环境使用 log

省略 `SMS_DRIVER` 或设为 `log`。下面的调用只记录消息类型和号码末四位，无需联系短信供应商。

```dotenv
SMS_DRIVER=log
```

```rust
Sms::send_text("+8613800138000", "本地短信测试").await?;
```

### 阿里云同时发送内地和国际短信

使用阿里云时，设定 `SMS_DRIVER=aliyun`，填写 `SMS_ALIYUN_ACCESS_KEY_ID` 和 `SMS_ALIYUN_ACCESS_KEY_SECRET`。内地模板需要 `SMS_ALIYUN_SIGN_NAME`，国际文本按目标市场要求填写 `SMS_ALIYUN_SENDER_ID`。省略 `SMS_ALIYUN_MARKET` 时，同一个配置可以向两个市场发送，驱动根据收件号码选择接口和端点。国际及港澳台号码使用 `+` 和国家或地区区号。

自动选择市场时，内地号码也要带 `+` 和国家或地区区号。只有明确将配置限制为 `mainland` 时，才接受 11 位内地本地手机号。

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

同一个业务模板名可以在一个市场使用厂商模板 ID，在另一个市场使用正文。在 `markets.international` 的 `templates` 中加入同名映射即可覆盖共用设置。`{code}` 这样的占位符取自调用时传入的具名参数，缺少参数时在发送请求前报错。

```rust
"templates": {
    "login_code": "SMS_123",
},
"markets": {
    "international": {
        "templates": {
            "login_code": {"text": "Your verification code is {code}."},
        },
    },
},
```

### 短信宝单账号发送内地和国际短信

在 `.env` 填写用户名和 API Key。省略 `SMS_SMSBAO_MARKET`，驱动会根据收件号码选择内地或国际接口。

```dotenv
SMS_DRIVER=smsbao
SMS_SMSBAO_USERNAME=your-username
SMS_SMSBAO_API_KEY=your-api-key
```

传入带审核签名的完整正文。发送到中国内地以外的号码时，带上 `+` 和国家或地区区号。

```rust
Sms::send_text("+8613800138000", "【示例商店】您的订单已发货。").await?;
Sms::send_text("+85255550123", "【示例商店】您的订单已发货。").await?;
```

### 云片单账号

在 `.env` 里设置默认配置和 API Key。

```dotenv
SMS_DRIVER=yunpian
SMS_YUNPIAN_API_KEY=your-api-key
```

发送带已审核签名的完整正文，驱动按号码选择内地或国际市场。

```rust
Sms::send_text("+8613800138000", "〖示例商店〗您的订单已发货。").await?;
```

### 腾讯云模板按顺序传参

在 `.env` 里配置腾讯云凭据、应用 ID 和已审核签名。

```dotenv
SMS_DRIVER=tencent
SMS_TENCENT_SECRET_ID=your-secret-id
SMS_TENCENT_SECRET_KEY=your-secret-key
SMS_TENCENT_SDK_APP_ID=your-app-id
SMS_TENCENT_SIGN_NAME="Your Brand"
```

把下面的字段加到 `config/sms.rs` 的 `drivers.tencent`，将 `id` 换成审核通过的模板 ID。

```rust
"templates": {
    "login_code": {
        "id": "123456",
        "parameter_order": ["code", "minutes"],
    },
},
```

业务代码传入具名参数即可。即使调用时先写 `minutes`，OpenRust 也会按配置先发送 `code`。

```rust
Sms::send_template(
    "+8613800138000",
    "login_code",
    [("minutes", "5"), ("code", "246810")],
)
.await?;
```

### Twilio 同时发送文本和模板

在 `.env` 里填写账号 SID、API Key 和发送号码。使用 Messaging Service 时，可用 `SMS_TWILIO_MESSAGING_SERVICE_SID` 指定服务。

```dotenv
SMS_DRIVER=twilio
SMS_TWILIO_ACCOUNT_SID=ACyour_account_sid
SMS_TWILIO_API_KEY_SID=SKyour_api_key_sid
SMS_TWILIO_API_KEY_SECRET=your-api-key-secret
SMS_TWILIO_FROM=+12025550100
```

把下面的字段加到 `config/sms.rs` 的 `drivers.twilio`，将示例 ID 换成你的 Twilio Content SID。

```rust
"templates": {
    "order_shipped": "HXyour_content_sid",
},
```

两次调用都使用同一个 Twilio 配置。模板变量的键按实际模板填写。

```rust
let sms = Sms::driver("twilio")?;
sms.send_text("+12025550123", "您的订单已发货。").await?;
sms.send_template("+12025550123", "order_shipped", [("1", "A1024")])
    .await?;
```

### Vonage 发送纯文本

设置 `SMS_DRIVER=vonage` 并填写 Vonage 凭据及发送身份。每个具名配置在 `config/sms.rs` 中独立设置 `api_key`、`api_secret`、`from` 和可选的 `endpoint`。驱动自动选择 Unicode 编码。需要业务跟踪标识或送达回执地址时，在配置中设置 `client_reference` 和 `status_callback`。

```dotenv
SMS_DRIVER=vonage
SMS_VONAGE_API_KEY=your-key
SMS_VONAGE_API_SECRET=your-secret
SMS_VONAGE_FROM=YourBrand
```

```rust
Sms::send_text("+12025550123", "您的订单已发货。").await?;
```

## 通知和 Worker

`sms` 通知渠道已默认注册。需要固定具名配置时，可用 `ChannelManager::extend` 注册 `SmsChannel::using("aliyun")`。实现 `to_sms` 并返回 `SmsMessage`，统一提供纯文本、供应商审核模板或配置中的业务模板。渠道从 `sms` 路由键下的 `NotificationRoute::Sms` 获取号码，并传给该方法。

```rust
impl Notification for LoginCode {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["sms".into()])
    }

    fn to_sms(
        &self,
        _: &dyn Notifiable,
        to: String,
    ) -> NotificationResult<Option<SmsMessage>> {
        SmsMessage::named_template(to, "login_code", [("code", self.code.as_str())])
            .map(Some)
    }
}
```

队列保存选中的配置名和消息。Worker 使用相同的渠道和配置名，从自身配置解析模板并读取凭据。Redis 队列可用 `./artisan queue:work redis` 启动 Worker。配置改名前先消费旧任务，或保留原名称直到旧任务完成。修改配置后，如已启用配置缓存，需重新生成并重启常驻 Worker。

发送验证码等敏感内容时，可以覆写 `Notification::should_encrypt`，为短信渠道返回 `true`。派发端和 Worker 须使用同一个有效的应用加密密钥。队列和失败任务保存密文，厂商凭据仍由 Worker 从自身配置读取。

## 提交结果和送达

`send_text`、`send_template` 和 `SmsSender::submit` 返回 `SmsSubmission`，包含供应商、提交状态和厂商消息 ID（如有）。`Logged` 表示本地已记录，`Accepted` 表示厂商已受理。业务可以保存该结果，在验证厂商回执或主动查询后，用供应商和消息 ID 调用 `record_delivery` 更新送达状态。底层 `send` 仍返回厂商原始响应。

明确的 HTTP 429 可以按任务策略重试。网络中断或响应异常导致受理结果未知时，Worker 停止自动重试。补发前先与厂商记录核对。短信已提交后，`NotificationSent` 监听器的错误会单独记录。

### Worker 崩溃后恢复

在启动生产 Worker 前运行 `./artisan migrate --force`。排队短信使用应用默认 SQL 连接上的 `sms_deliveries` 表，即使队列本身使用 Redis。所有 Worker 须连接同一个记录库。

Worker 在调用厂商前按队列任务 UUID 登记，并在触发 `NotificationSent` 之前保存提交结果。接手的 Worker 遇到已受理的任务会直接确认队列。如果上一次仍是 `submitting` 或结果未知，任务会进入失败记录供核对。先查厂商记录，再决定是否创建新任务补发。

队列仍可能重投或重试时，保留对应发送记录。升级前先停旧 Worker，核对发送中途被中断的历史任务。记录表覆盖升级后执行的发送尝试。

## 注册供应商 {#custom-sms-providers}

在应用服务提供者的 `register` 中解析 `SmsManager`，调用 `extend` 注册工厂。工厂接收选中的配置，返回实现 `SmsDriver` 的驱动。多个具名配置可以复用同一驱动。
