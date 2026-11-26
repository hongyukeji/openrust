use serde_json::{Value, json};
use url::Url;

use crate::{
    http::client::tencent_rpc::{CONTENT_TYPE, TencentRpcOperation, TencentRpcSigner},
    notifications::{NotificationFuture, NotificationResult, delivery},
};

use super::{
    super::{SmsDriver, SmsMessage, SmsTemplateParameters},
    rejected, request, required, response, rpc_endpoint, setting, unknown,
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
            let headers = TencentRpcSigner {
                secret_id: &self.secret_id,
                secret_key: self.secret_key.as_bytes(),
                security_token: None,
            }
            .sign_post(
                &self.endpoint,
                TencentRpcOperation {
                    service: "sms",
                    action: "SendSms",
                    version: "2021-01-11",
                    region: &self.region,
                },
                body.as_bytes(),
            )
            .map_err(|_| delivery("The SMS profile contains an invalid request header."))?;
            let payload = response(
                request()?
                    .with_headers(headers)
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
