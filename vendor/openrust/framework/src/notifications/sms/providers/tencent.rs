use serde_json::{Value, json};
use url::{Position, Url};

use crate::notifications::{NotificationFuture, NotificationResult, delivery};

use super::{
    super::{SmsDriver, SmsMessage, SmsTemplateParameters},
    hex, now, rejected, request, required, response, rpc_endpoint, setting, sha256, sign,
    signed_headers, unknown,
};

pub(super) struct Tencent {
    market: super::Market,
    endpoint: Url,
    secret_id: String,
    secret_key: String,
    app_id: String,
    sign_name: Option<String>,
    region: String,
}

impl Tencent {
    pub(super) fn new(profile: &Value, market: super::Market) -> NotificationResult<Self> {
        Ok(Self {
            market,
            endpoint: rpc_endpoint(profile, "https://sms.tencentcloudapi.com")?,
            secret_id: required(profile, "secret_id")?,
            secret_key: required(profile, "secret_key")?,
            app_id: required(profile, "sdk_app_id")?,
            sign_name: setting(profile, "sign_name"),
            region: required(profile, "region")?,
        })
    }
}

impl SmsDriver for Tencent {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let Some(template) = message.approved_template() else {
                return Err(delivery("Tencent Cloud SMS requires an approved template."));
            };
            let SmsTemplateParameters::Positional(parameters) = &template.parameters else {
                return Err(delivery(
                    "Tencent Cloud SMS templates require positional parameters.",
                ));
            };
            let mut body = json!({
                "PhoneNumberSet": [format!("+{}", self.market.number(message.to())?)],
                "SmsSdkAppId": self.app_id,
                "TemplateId": template.id,
                "TemplateParamSet": parameters,
            });
            if let Some(name) = &self.sign_name {
                body["SignName"] = json!(name);
            }
            let body = serde_json::to_string(&body).map_err(delivery)?;
            let now = now();
            let timestamp = crate::support::timestamp_seconds(now).to_string();
            let date = now.strftime("%Y-%m-%d").to_string();
            let host = &self.endpoint[Position::BeforeHost..Position::AfterPort];
            const CONTENT_TYPE: &str = "application/json; charset=utf-8";
            let signed = "content-type;host";
            let canonical = format!(
                "POST\n/\n\ncontent-type:{CONTENT_TYPE}\nhost:{host}\n\n{signed}\n{}",
                sha256(body.as_bytes())
            );
            let scope = format!("{date}/sms/tc3_request");
            let to_sign = format!(
                "TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
                sha256(canonical.as_bytes())
            );
            let key = sign(format!("TC3{}", self.secret_key).as_bytes(), &date);
            let service = sign(key.as_ref(), "sms");
            let signing = sign(service.as_ref(), "tc3_request");
            let signature = hex(sign(signing.as_ref(), &to_sign).as_ref());
            let authorization = format!(
                "TC3-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
                self.secret_id
            );
            let headers = [
                ("host", host),
                ("x-tc-action", "SendSms"),
                ("x-tc-version", "2021-01-11"),
                ("x-tc-timestamp", timestamp.as_str()),
                ("x-tc-region", self.region.as_str()),
            ];
            let payload = response(
                request()?
                    .with_headers(signed_headers(&headers, &authorization)?)
                    .with_body(body, http::HeaderValue::from_static(CONTENT_TYPE))
                    .post(self.endpoint.as_str(), &())
                    .await,
            )?;
            let status = &payload["Response"]["SendStatusSet"];
            if let Some(code) = payload["Response"]["Error"]["Code"].as_str() {
                return Err(rejected(
                    "Tencent Cloud SMS did not accept the submission.",
                    Some(code),
                ));
            }
            if let Some(code) = status
                .get(0)
                .and_then(|entry| entry["Code"].as_str())
                .filter(|code| *code != "Ok")
            {
                return Err(rejected(
                    "Tencent Cloud SMS did not accept the submission.",
                    Some(code),
                ));
            }
            if status.as_array().is_none_or(|entries| entries.len() != 1)
                || status[0]["Code"] != "Ok"
                || status[0]["SerialNo"].as_str().is_none_or(str::is_empty)
            {
                return Err(unknown("Tencent Cloud SMS did not confirm the submission."));
            }
            Ok(payload)
        })
    }
}
