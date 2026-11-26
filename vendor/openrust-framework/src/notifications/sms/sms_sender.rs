use std::{collections::BTreeMap, sync::Arc};

use serde_json::Value;

use super::{
    NotificationResult, SmsDriver, SmsMessage, SmsSubmission, SmsTemplateParameters, delivery,
    providers,
};

/// Sends text or templates using one configured SMS profile.
#[derive(Clone)]
pub struct SmsSender {
    pub(super) profile: Arc<Value>,
    pub(super) driver: Arc<dyn SmsDriver>,
}

impl SmsSender {
    pub async fn send(&self, mut message: SmsMessage) -> NotificationResult<Value> {
        message.validate()?;
        if let Some(name) = message.template_name.take()
            && self.profile["driver"] != "log"
        {
            let market = providers::market_key(message.to(), &self.profile)?;
            let template_setting = self.profile["markets"][market]["templates"]
                .get(&name)
                .or_else(|| self.profile["templates"].get(&name))
                .ok_or_else(|| {
                    delivery("The SMS template is not configured for this profile and market.")
                })?;
            if let Some(text) = template_setting.get("text") {
                let template = message.template.as_ref().expect("validated template");
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery("The configured SMS text requires named input."));
                };
                let text = text
                    .as_str()
                    .ok_or_else(|| delivery("The configured SMS text must be a string."))?;
                let content = render_sms_text(text, parameters)?;
                return self
                    .driver
                    .send(SmsMessage::new(message.to, content)?)
                    .await;
            }
            let id = template_setting
                .as_str()
                .or_else(|| template_setting["id"].as_str())
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| delivery("The configured SMS template requires an ID."))?;
            let template = message.template.as_mut().expect("validated template");
            template.id = id.to_owned();
            if let Some(order) = template_setting.get("parameter_order") {
                let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                    return Err(delivery(
                        "The configured SMS template requires named input.",
                    ));
                };
                let order = order
                    .as_array()
                    .ok_or_else(|| delivery("The SMS template parameter order must be a list."))?;
                let ordered = order
                    .iter()
                    .map(|key| {
                        key.as_str()
                            .and_then(|key| parameters.get(key))
                            .cloned()
                            .ok_or_else(|| {
                                delivery("A configured SMS template parameter is missing.")
                            })
                    })
                    .collect::<NotificationResult<Vec<_>>>()?;
                template.parameters = SmsTemplateParameters::Positional(ordered);
            }
        }
        self.driver.send(message).await
    }

    /// Submits an SMS and normalizes the provider acknowledgement.
    pub async fn submit(&self, message: SmsMessage) -> NotificationResult<SmsSubmission> {
        let response = self.send(message).await?;
        let provider = self.profile["driver"]
            .as_str()
            .expect("registered SMS driver");
        Ok(SmsSubmission::from_response(provider, &response))
    }

    pub async fn send_text(
        &self,
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> NotificationResult<SmsSubmission> {
        self.submit(SmsMessage::new(to, content)?).await
    }

    pub async fn send_template<K: Into<String>, V: Into<String>>(
        &self,
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<SmsSubmission> {
        self.submit(SmsMessage::named_template(to, name, parameters)?)
            .await
    }
}

fn render_sms_text(
    mut text: &str,
    parameters: &BTreeMap<String, String>,
) -> NotificationResult<String> {
    let mut content = String::with_capacity(text.len());
    while let Some((literal, tail)) = text.split_once('{') {
        content.push_str(literal);
        let (name, remainder) = tail
            .split_once('}')
            .ok_or_else(|| delivery("The configured SMS text has an unclosed placeholder."))?;
        let value = parameters
            .get(name)
            .ok_or_else(|| delivery("A configured SMS text parameter is missing."))?;
        content.push_str(value);
        text = remainder;
    }
    content.push_str(text);
    Ok(content)
}
