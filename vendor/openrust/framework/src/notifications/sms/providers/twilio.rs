use serde_json::Value;
use url::Url;

use crate::notifications::{NotificationFuture, NotificationResult, delivery};

use super::{
    super::{SmsDriver, SmsMessage, SmsTemplateParameters},
    endpoint, rejected, request, required, response, setting, unknown,
};

pub(super) struct Twilio {
    market: super::Market,
    endpoint: Url,
    api_key_sid: String,
    api_key_secret: String,
    from: Option<String>,
    service: Option<String>,
}

impl Twilio {
    pub(super) fn new(profile: &Value, market: super::Market) -> NotificationResult<Self> {
        let account = required(profile, "account_sid")?;
        let mut endpoint = endpoint(profile, "https://api.twilio.com")?;
        endpoint
            .path_segments_mut()
            .map_err(|_| delivery("The Twilio endpoint is invalid."))?
            .pop_if_empty()
            .extend(["2010-04-01", "Accounts", &account, "Messages.json"]);
        let from = setting(profile, "from");
        let service = setting(profile, "messaging_service_sid");
        if from.is_none() && service.is_none() {
            return Err(delivery("Twilio requires from or messaging_service_sid."));
        }
        Ok(Self {
            market,
            endpoint,
            api_key_sid: required(profile, "api_key_sid")?,
            api_key_secret: required(profile, "api_key_secret")?,
            from,
            service,
        })
    }
}

impl SmsDriver for Twilio {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let to = format!("+{}", self.market.number(message.to())?);
            let mut fields = vec![("To", to.as_str())];
            if let Some(from) = &self.from {
                fields.push(("From", from));
            }
            if let Some(service) = &self.service {
                fields.push(("MessagingServiceSid", service));
            }
            let variables;
            if let Some(template) = message.approved_template() {
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery("Twilio templates require named parameters."));
                };
                fields.push(("ContentSid", &template.id));
                if !parameters.is_empty() {
                    variables = serde_json::to_string(parameters).map_err(delivery)?;
                    fields.push(("ContentVariables", &variables));
                }
            } else {
                fields.push(("Body", message.content()));
            }
            let payload = response(
                request()?
                    .asForm()
                    .withBasicAuth(&self.api_key_sid, &self.api_key_secret)
                    .post(self.endpoint.as_str(), &fields)
                    .await,
            )?;
            let id = payload["sid"].as_str().filter(|value| !value.is_empty());
            if matches!(
                payload["status"].as_str(),
                Some("failed" | "undelivered" | "canceled")
            ) {
                let code = payload["error_code"].as_i64().map(|code| code.to_string());
                return Err(rejected(
                    "Twilio did not accept the SMS submission.",
                    code.as_deref(),
                ));
            }
            if id.is_none() {
                return Err(unknown("Twilio did not confirm the SMS submission."));
            }
            Ok(payload)
        })
    }
}
