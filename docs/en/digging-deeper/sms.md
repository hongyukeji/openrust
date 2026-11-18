# SMS

The unified SMS API is an OpenRust extension based on Laravel’s [file storage](filesystem.md) design. Just as `Storage::disk` selects a named disk backed by a registered driver, `Sms::driver` selects a named SMS profile backed by a registered provider driver. Multiple profiles can use the same driver with different settings.

## Built-in providers

Set `SMS_DRIVER` in the application's environment to select the default profile. Supply that profile's credentials to the application and its Workers. The built-ins use the same `Sms::driver` entry point as custom providers.

| Profile | Supported content | Environment variables |
| --- | --- | --- |
| `vonage` (default) | Text | `VONAGE_KEY`, `VONAGE_SECRET`, `VONAGE_SMS_FROM` |
| `twilio` | Text and approved templates with named variables | `SMS_TWILIO_ACCOUNT_SID`, `SMS_TWILIO_API_KEY_SID`, `SMS_TWILIO_API_KEY_SECRET`, and `SMS_TWILIO_FROM` or `SMS_TWILIO_MESSAGING_SERVICE_SID` |
| `aliyun` | Chinese mainland approved templates with named variables | `SMS_ALIYUN_ACCESS_KEY_ID`, `SMS_ALIYUN_ACCESS_KEY_SECRET`, `SMS_ALIYUN_SIGN_NAME` |
| `aliyun_international` | International and Hong Kong, Macao, and Taiwan text via SendMessageToGlobe | `SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_ID`, `SMS_ALIYUN_INTERNATIONAL_ACCESS_KEY_SECRET`. Optional `SMS_ALIYUN_INTERNATIONAL_SENDER_ID` and `SMS_ALIYUN_INTERNATIONAL_MESSAGE_TYPE` (`OTP`, `NOTIFY`, `MKT`) |
| `tencent` | Approved templates with ordered variables | `SMS_TENCENT_SECRET_ID`, `SMS_TENCENT_SECRET_KEY`, `SMS_TENCENT_SDK_APP_ID`, `SMS_TENCENT_SIGN_NAME` if required, and `SMS_TENCENT_REGION` (default `ap-guangzhou`) |

Use the `+country code` form for international numbers, including Hong Kong, Macao, and Taiwan. The `aliyun` profile accepts Chinese mainland numbers for templates, and can add `+86` to an 11-digit local mobile number. Choose `aliyun_international` for free text outside the Chinese mainland. It uses its own credentials and defaults to the Alibaba Cloud Singapore endpoint. Tencent and Twilio profiles can set their respective `SMS_*_MARKET` variable to `mainland` or `international`. Select a sender and an approved template for the destination and profile. Provider acceptance and handset delivery are separate states.

## Text and templates

Use `SmsMessage::new` for text and `SmsMessage::template` for a provider-approved template. Return text from `to_sms` or a template from `to_sms_template` in a notification. The driver rejects unsupported message types and incorrect variable formats before submitting.

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

## Registering providers {#custom-sms-providers}

Resolve the default profile with `Sms::driver(None)` or select one explicitly with `Sms::driver(Some("backup"))`. For a direct send, call `driver.send(SmsMessage::new(number, text)?).await?`. In an application service provider’s `register` hook, resolve `SmsManager` and call `extend` for each driver. The factory receives the selected profile’s configuration and returns a driver that implements `SmsDriver`. Profile names under `sms.drivers` can differ from the registered driver names.

Register `SmsChannel::default()` as `sms` with `ChannelManager::extend`, and use `SmsChannel::using("backup")` for a channel pinned to the backup profile. Select the registered channel in `Notification::via`, return the text from `to_sms`, and route the recipient with `NotificationRoute::Sms` under the `sms` route key. Register the same provider and channels in the dispatching application and the Worker. The existing `vonage` channel remains available.

## Configuration

The following example configuration selects `primary`. Keep tokens in environment variables and load them into the profiles at runtime.

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

## Running the example

Run the complete example from the repository root. Start the mock endpoints in one terminal.

```sh
python3 -B examples/sms/mock.py
```

In another terminal, start an isolated Redis instance and run the example application’s Artisan commands.

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

The mock prints `/alpha`, `/beta`, and `/alpha`, respectively. Add `--now` to `sms:demo` to try the direct facade call. The queue stores the profile name, number, and text. The Worker reads the endpoint and credentials from its own configuration. Changing `sms.default` after dispatch leaves existing jobs on their selected profile, so keep that profile configured until those jobs finish.

Stop the mock with Ctrl+C and run `docker stop openrust-sms-demo`. The mock receives local HTTP requests without sending real SMS.
