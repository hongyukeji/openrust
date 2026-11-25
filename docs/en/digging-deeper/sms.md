# SMS

The unified SMS API uses the named-driver design described in [file storage](filesystem.md). Just as `Storage::disk` selects a named disk backed by a registered driver, `Sms::driver` selects a named SMS profile backed by a registered provider driver. Multiple profiles can use the same driver with different settings.

The application supplies the default `log` profile in `config/sms.rs`. If a custom configuration loader replaces the SMS settings, it must provide both `default` and the selected entry under `drivers`; missing settings produce an error.

## Driver Lifecycle {#sms-driver-lifecycle}

`SmsManager` follows named driver pattern. The first resolution of a profile creates its driver; subsequent calls reuse the driver and the profile's template settings within the same application. Different profiles and applications have independent instances. The default profile name is read when `default_driver` or a default send method is called.

Update configuration or register a replacement factory before clearing the resolved drivers:

```rust
Sms::forget_drivers()?;
let sender = Sms::driver("aliyun")?;
```

`Sms::get_default_driver()` returns the configured default profile name without creating a driver. `forget_drivers` affects future resolutions; existing `SmsSender` handles keep their original driver and settings. Failed driver creation can be retried. In long-running workers, rebuild configuration caches when applicable and restart workers after configuration changes.

## Built-in providers

Set `SMS_DRIVER` in the application's environment to select the default profile. Supply that profile's credentials to the application and its Workers. The built-ins use the same `Sms::driver` entry point as custom providers.

| Profile | Supported content | Environment variables |
| --- | --- | --- |
| `log` (default) | Text and templates. Writes an info entry with the last four digits of the number and the message type. | None |
| `vonage` | Text | `SMS_VONAGE_API_KEY`, `SMS_VONAGE_API_SECRET`, `SMS_VONAGE_FROM` |
| `twilio` | Text and approved templates with named variables | `SMS_TWILIO_ACCOUNT_SID`, `SMS_TWILIO_API_KEY_SID`, `SMS_TWILIO_API_KEY_SECRET`, and `SMS_TWILIO_FROM` or `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | Mainland China approved templates and international text via SendMessageToGlobe | `SMS_ALIYUN_ACCESS_KEY_ID`, `SMS_ALIYUN_ACCESS_KEY_SECRET`. Use `SMS_ALIYUN_SIGN_NAME` for mainland templates or optional `SMS_ALIYUN_SENDER_ID` and `SMS_ALIYUN_MESSAGE_TYPE` (`OTP`, `NOTIFY`, `MKT`) for international text. |
| `tencent` | Approved templates with ordered variables | `SMS_TENCENT_SECRET_ID`, `SMS_TENCENT_SECRET_KEY`, `SMS_TENCENT_SDK_APP_ID`, `SMS_TENCENT_SIGN_NAME` if required, and `SMS_TENCENT_REGION` (default `ap-guangzhou`) |
| `smsbao` | Mainland and international text | `SMS_SMSBAO_USERNAME`, `SMS_SMSBAO_API_KEY` |
| `yunpian` | Mainland text or named templates, and international text | `SMS_YUNPIAN_API_KEY` |

## Sending SMS

Use the default profile for routine sends. Select a profile by name when the application needs a particular provider or account.

```rust
use openrust::support::facades::Sms;

Sms::send_text("+85255550123", "Your order has shipped.").await?;

Sms::driver("aliyun")?
    .send_template("+8613800138000", "login_code", [("code", "123456")])
    .await?;
assert_eq!(receipt["accepted"], true);
```

The default `log` driver records message type and the last four digits in application logs. Set `SMS_DRIVER` to a provider or named profile for live delivery.

## Configuration

### Log for local development

Omit `SMS_DRIVER` or set it to `log`. The example records the message type and the last four digits without contacting an SMS provider.

```dotenv
SMS_DRIVER=log
```

```rust
Sms::send_text("+8613800138000", "Local SMS test").await?;
```

### Alibaba Cloud across two markets

To use Alibaba Cloud, set `SMS_DRIVER=aliyun` and provide `SMS_ALIYUN_ACCESS_KEY_ID` and `SMS_ALIYUN_ACCESS_KEY_SECRET`. Configure `SMS_ALIYUN_SIGN_NAME` for mainland templates and `SMS_ALIYUN_SENDER_ID` if required for international text. Leave `SMS_ALIYUN_MARKET` unset to use both markets from one profile. The driver selects the appropriate API and endpoint from the recipient number. Use `+` and the calling code for international numbers, including Hong Kong, Macao, and Taiwan.

Use a leading `+` and country calling code for automatic market selection, including for mainland numbers. A profile explicitly restricted to `mainland` also accepts an 11-digit mainland mobile number.

Add business template names under `templates` on a profile. A value can be a provider template ID or an object with `id` and `parameter_order` for positional APIs such as Tencent Cloud. Put settings specific to one market under `markets.mainland` or `markets.international`. They override only the listed settings, so shared credentials stay at the profile level.

The example adds these entries to the existing `aliyun` profile. Set `SMS_ALIYUN_GLOBAL_ENDPOINT` only when using a custom endpoint. Override both credential fields inside the market if that account has its own key pair.

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

The selected driver checks the message type before submitting. Alibaba Cloud mainland SMS needs an approved signature and template, while the international route described here accepts text. Use `SmsMessage::template` with the lower-level `send` method when you already have the provider template ID.

One business template can use a provider template ID in one market and text in another. Add a `templates` entry under `markets.international` to override the common mapping. Placeholders such as `{code}` use the named parameters passed by the application. A missing parameter aborts the send before contacting the provider.

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

### SmsBao with one account for both markets

Set the username and API key in `.env`. Leave `SMS_SMSBAO_MARKET` unset so the driver chooses the mainland or international endpoint from the number.

```dotenv
SMS_DRIVER=smsbao
SMS_SMSBAO_USERNAME=your-username
SMS_SMSBAO_API_KEY=your-api-key
```

Pass the complete rendered text with the approved signature. Use the international calling code for destinations outside the Chinese mainland.

```rust
Sms::send_text("+8613800138000", "【Example Shop】Your order has shipped.").await?;
Sms::send_text("+85255550123", "【Example Shop】Your order has shipped.").await?;
```

### Yunpian with one account

Set the default profile and API key in `.env`.

```dotenv
SMS_DRIVER=yunpian
SMS_YUNPIAN_API_KEY=your-api-key
```

Send the full text with your approved signature. The driver selects the market from the recipient number.

```rust
Sms::send_text("+8613800138000", "〖Your Brand〗Your order has shipped.").await?;
```

### Tencent Cloud with ordered template variables

Set the default profile, credentials, and app ID in `.env`. Use your approved signature.

```dotenv
SMS_DRIVER=tencent
SMS_TENCENT_SECRET_ID=your-secret-id
SMS_TENCENT_SECRET_KEY=your-secret-key
SMS_TENCENT_SDK_APP_ID=your-app-id
SMS_TENCENT_SIGN_NAME="Your Brand"
```

Add this fragment to `drivers.tencent` in `config/sms.rs`. Set `id` to your approved template ID.

```rust
"templates": {
    "login_code": {
        "id": "123456",
        "parameter_order": ["code", "minutes"],
    },
},
```

Pass named variables in the application. OpenRust sends them to Tencent Cloud in the configured order.

```rust
Sms::send_template(
    "+8613800138000",
    "login_code",
    [("minutes", "5"), ("code", "246810")],
)
.await?;
```

### Twilio with text and templates

Add the account SID, API key, and sender number to `.env`. You can use `SMS_TWILIO_MESSAGING_SERVICE_SID` in place of the sender number.

```dotenv
SMS_DRIVER=twilio
SMS_TWILIO_ACCOUNT_SID=ACyour_account_sid
SMS_TWILIO_API_KEY_SID=SKyour_api_key_sid
SMS_TWILIO_API_KEY_SECRET=your-api-key-secret
SMS_TWILIO_FROM=+12025550100
```

Add this fragment to `drivers.twilio` in `config/sms.rs`. Replace the example ID with your Twilio Content SID.

```rust
"templates": {
    "order_shipped": "HXyour_content_sid",
},
```

Both calls use the same Twilio profile. Use the variable keys defined in your Twilio template.

```rust
let sms = Sms::driver("twilio")?;
sms.send_text("+12025550123", "Your order has shipped.").await?;
sms.send_template("+12025550123", "order_shipped", [("1", "A1024")])
    .await?;
assert_eq!(receipt["accepted"], true);
```

### Vonage text messages

Set `SMS_DRIVER=vonage` and supply the Vonage credentials and sender identity. Each named profile stores its own `api_key`, `api_secret`, `from`, and optional `endpoint` in `config/sms.rs`. Unicode encoding is selected automatically. Set `client_reference` and `status_callback` on the profile when needed.

```dotenv
SMS_DRIVER=vonage
SMS_VONAGE_API_KEY=your-key
SMS_VONAGE_API_SECRET=your-secret
SMS_VONAGE_FROM=YourBrand
```

```rust
Sms::send_text("+12025550123", "Your order has shipped.").await?;
```

## Notifications and Workers

The `sms` notification channel is registered by default. Use `ChannelManager::extend` with `SmsChannel::using("aliyun")` when a channel should use a fixed profile. Implement `to_sms` and return a `SmsMessage` for text, an approved provider template, or a configured business template. The channel passes the number from `NotificationRoute::Sms` under the `sms` route key to this method.

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

The queue stores the selected profile name and message. Register the same channel and profile in the Worker, which reads credentials and resolves the template from its own configuration. Run it with `./artisan queue:work redis` when using Redis. A rename requires draining jobs that reference the old name or keeping that name until they finish. Rebuild cached configuration and restart Workers after changing settings.

For messages containing verification codes or other sensitive data, override `Notification::should_encrypt` to return `true` for the SMS channel. The dispatcher and Worker must share a valid application encryption key. Both the queued and failed job payloads then contain ciphertext. The Worker still reads provider credentials from its own configuration.

## Submission and delivery

`send_text`, `send_template`, and `SmsSender::submit` return an `SmsSubmission` with its provider, submission status, and available provider message IDs. `Logged` is a local log entry and `Accepted` means the provider accepted the submission. Store the result and use `record_delivery` with the provider and message ID after verifying an official callback or querying the provider. The lower-level `send` still returns the provider response.

An explicit HTTP 429 may retry within the job policy. When the network or provider response leaves acceptance unknown, the Worker stops automatic retries. Match the submission against provider records before resending. A `NotificationSent` listener error after an accepted SMS is logged independently.

### Recovery after a Worker crash

Run `./artisan migrate --force` before starting production Workers. Queued SMS uses the `sms_deliveries` table on the application’s default SQL connection, including when Redis holds the queue. All Workers must use the same database for these records.

The Worker claims the queue job UUID before it calls the provider and stores the submission result before firing `NotificationSent`. A replacement Worker acknowledges an already accepted job without sending the SMS again. If the prior attempt is still marked `submitting` or has an unknown outcome, the job is marked failed for review. Check with the provider before creating a replacement job.

Keep the delivery rows while their jobs can still be redelivered or retried. Before upgrading, stop old Workers and reconcile any jobs interrupted during a send. The table records attempts made after the upgrade.

## Registering providers {#custom-sms-providers}

In a service provider’s `register` hook, resolve `SmsManager` and call `extend` to register a driver factory. The factory receives the selected profile and returns a type that implements `SmsDriver`. Multiple named profiles can use the same driver.


The following complete text-only driver uses the shared HTTP client and registers an `example` factory. Place it in an application provider module and add `ExampleSmsProvider` to `bootstrap/providers.rs`. Replace the endpoint protocol and receipt validation for your provider; this example does not send supplier-approved templates.

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    notifications::{
        NotificationError, NotificationFuture, NotificationResult,
        sms::{SmsDriver, SmsManager, SmsMessage},
    },
    support::{ProviderResult, ServiceProvider, facades::Http},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub struct ExampleSmsProvider;
impl ServiceProvider for ExampleSmsProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<SmsManager>()?
                .extend("example", |profile| {
                    let endpoint = profile["endpoint"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| failure("短信端点未配置"))?;
                    Ok(Arc::new(ExampleSms {
                        endpoint: endpoint.into(),
                    }))
                });
            Ok(())
        })
    }
}

struct ExampleSms {
    endpoint: String,
}
impl SmsDriver for ExampleSms {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            if message.approved_template().is_some() {
                return Err(failure("示例驱动仅支持纯文本"));
            }
            let response = Http::accept_json()
                .map_err(|_| failure("HTTP 客户端不可用"))?
                .without_redirecting()
                .timeout(Duration::from_secs(5))
                .max_response_bytes(16 * 1024)
                .post(
                    &self.endpoint,
                    &json!({"to":message.to(), "text":message.content()}),
                )
                .await
                .map_err(|_| failure("短信供应商连接失败"))?;
            if !response.successful() {
                return Err(failure("短信供应商未接受请求"));
            }
            let receipt: Value = response.json().map_err(|_| failure("短信回执无效"))?;
            if receipt["accepted"] != true {
                return Err(failure("短信供应商拒绝请求"));
            }
            Ok(receipt)
        })
    }
}
fn failure(message: &'static str) -> NotificationError {
    NotificationError::Delivery {
        source: message.into(),
    }
}
```

Add a named profile to `drivers` in `config/sms.rs`, for example `"example_primary": {"driver":"example", "endpoint":"https://sms.example/api/messages"}`. Keep secrets in environment-backed configuration. Send with the named profile:

```rust
use openrust::{notifications::sms::SmsMessage, support::facades::Sms};

let receipt = Sms::driver("example_primary")?
    .send(SmsMessage::new("+12025550123", "Hello")?)
    .await?;
assert_eq!(receipt["accepted"], true);
```

The same factory may serve multiple profiles. Each profile keeps its own configuration snapshot. Replacing a factory affects newly resolved senders; call `Sms::forget_drivers()` to rebuild cached senders. Existing `SmsSender` handles keep their snapshots. Register the provider in each HTTP, Artisan and Worker process: queued notifications serialize the profile name and message, never closures, endpoint credentials or a sender object. In a cluster, deploy the same provider and named configuration on each worker; use the existing shared database/queue delivery records. A successful test endpoint receipt does not prove carrier delivery.
