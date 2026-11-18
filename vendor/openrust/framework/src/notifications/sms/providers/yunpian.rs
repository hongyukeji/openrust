use serde_json::Value;
use url::{Url, form_urlencoded};

use crate::notifications::{NotificationFuture, NotificationResult, delivery};

use super::super::{SmsDriver, SmsMessage, SmsTemplateParameters};
use super::{Market, request, required, response, rpc_endpoint};

pub(super) struct Yunpian {
    endpoint: Url,
    market: Market,
    api_key: String,
}

impl Yunpian {
    pub(super) fn new(profile: &Value, market: Market) -> NotificationResult<Self> {
        Ok(Self {
            endpoint: rpc_endpoint(profile, "https://sms.yunpian.com")?,
            market,
            api_key: required(profile, "api_key")?,
        })
    }
}

impl SmsDriver for Yunpian {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let mobile = self.market.provider_number(message.to())?;
            let mut url = self.endpoint.clone();
            let mut fields = vec![
                ("apikey", self.api_key.as_str()),
                ("mobile", mobile.as_str()),
            ];
            let variables;
            if let Some(template) = message.approved_template() {
                if !matches!(self.market, Market::Mainland) {
                    return Err(delivery(
                        "Yunpian international SMS requires the rendered SMS text.",
                    ));
                }
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery("Yunpian templates require named parameters."));
                };
                if template.id.parse::<i64>().ok().is_none_or(|id| id <= 0)
                    || !template.id.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(delivery(
                        "The Yunpian template ID must be a positive integer.",
                    ));
                }
                let mut form = form_urlencoded::Serializer::new(String::new());
                for (key, value) in parameters {
                    if key.is_empty() || key.trim() != key || key.contains('#') || value.is_empty()
                    {
                        return Err(delivery(
                            "Yunpian template names must omit # delimiters and names and values cannot be empty.",
                        ));
                    }
                    form.append_pair(&format!("#{key}#"), value);
                }
                variables = form.finish();
                fields.extend([
                    ("tpl_id", template.id.as_str()),
                    ("tpl_value", variables.as_str()),
                ]);
                url.set_path("/v2/sms/tpl_single_send.json");
            } else {
                fields.push(("text", message.content()));
                url.set_path("/v2/sms/single_send.json");
            }
            let payload = response(request()?.asForm().post(url.as_str(), &fields).await)?;
            match payload["code"].as_i64() {
                Some(0) if payload["sid"].as_u64().is_some_and(|id| id > 0) => Ok(payload),
                Some(code) if code != 0 => Err(delivery(format!(
                    "Yunpian rejected the submission (code {code})."
                ))),
                _ => Err(delivery("Yunpian returned an invalid submission result.")),
            }
        })
    }
}
