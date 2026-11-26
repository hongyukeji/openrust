use serde_json::{Value, json};
use url::Url;

use crate::notifications::{NotificationFuture, NotificationResult, delivery};

use super::super::{SmsDriver, SmsMessage};
use super::{Market, checked_response, rejected, request, required, rpc_endpoint, unknown};

pub(super) struct SmsBao {
    endpoint: Url,
    market: Market,
    username: String,
    api_key: String,
}

impl SmsBao {
    pub(super) fn new(profile: &Value, market: Market) -> NotificationResult<Self> {
        let path = if matches!(market, Market::Mainland) {
            "/sms"
        } else {
            "/wsms"
        };
        let mut endpoint = rpc_endpoint(profile, "https://api.smsbao.com")?;
        endpoint.set_path(path);
        Ok(Self {
            endpoint,
            market,
            username: required(profile, "username")?,
            api_key: required(profile, "api_key")?,
        })
    }
}

impl SmsDriver for SmsBao {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            if message.approved_template().is_some() {
                return Err(delivery("SmsBao requires the rendered SMS text."));
            }
            let mobile = self.market.provider_number(message.to())?;
            let response = checked_response(
                request()?
                    .with_query_parameters([
                        ("u".into(), self.username.clone()),
                        ("p".into(), self.api_key.clone()),
                        ("m".into(), mobile),
                        ("c".into(), message.content().into()),
                    ])
                    .get(self.endpoint.as_str())
                    .await,
            )?;
            let body = response
                .body()
                .map_err(|_| unknown("读取短信宝提交结果失败。"))?;
            let code = std::str::from_utf8(&body)
                .ok()
                .and_then(|body| body.trim().parse::<u32>().ok())
                .ok_or_else(|| unknown("SmsBao returned an invalid submission code."))?;
            if code != 0 {
                return Err(rejected(
                    "SmsBao rejected the submission",
                    Some(&code.to_string()),
                ));
            }
            Ok(json!({"code":code}))
        })
    }
}
