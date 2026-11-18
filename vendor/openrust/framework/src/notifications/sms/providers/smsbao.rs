use serde_json::{Value, json};
use url::Url;

use crate::notifications::{NotificationFuture, NotificationResult, delivery};

use super::super::{SmsDriver, SmsMessage};
use super::{Market, checked_response, request, required, rpc_endpoint};

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
                    .withQueryParameters([
                        ("u".into(), self.username.clone()),
                        ("p".into(), self.api_key.clone()),
                        ("m".into(), mobile),
                        ("c".into(), message.content().into()),
                    ])
                    .get(self.endpoint.as_str())
                    .await,
            )?;
            let code = std::str::from_utf8(response.body())
                .ok()
                .and_then(|body| body.trim().parse::<u32>().ok())
                .ok_or_else(|| delivery("SmsBao returned an invalid submission code."))?;
            if code != 0 {
                return Err(delivery(format!(
                    "SmsBao rejected the submission (code {code})."
                )));
            }
            Ok(json!({"code":code}))
        })
    }
}
