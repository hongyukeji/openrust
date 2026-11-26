use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{NotificationResult, delivery};

/// The fields shared by SMS providers.
#[derive(Deserialize, Serialize)]
pub struct SmsMessage {
    pub(super) to: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) template: Option<SmsTemplate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) template_name: Option<String>,
}

/// A provider-approved template and the values used to fill it.
#[derive(Clone, Deserialize, Serialize)]
pub struct SmsTemplate {
    pub id: String,
    pub parameters: SmsTemplateParameters,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SmsTemplateParameters {
    Named(BTreeMap<String, String>),
    Positional(Vec<String>),
}

impl SmsTemplate {
    pub fn named(id: impl Into<String>, parameters: BTreeMap<String, String>) -> Self {
        Self {
            id: id.into(),
            parameters: SmsTemplateParameters::Named(parameters),
        }
    }

    pub fn positional(id: impl Into<String>, parameters: Vec<String>) -> Self {
        Self {
            id: id.into(),
            parameters: SmsTemplateParameters::Positional(parameters),
        }
    }
}

impl SmsMessage {
    pub fn new(to: impl Into<String>, content: impl Into<String>) -> NotificationResult<Self> {
        let message = Self {
            to: to.into(),
            content: content.into(),
            template: None,
            template_name: None,
        };
        message.validate()?;
        Ok(message)
    }

    pub fn template(to: impl Into<String>, template: SmsTemplate) -> NotificationResult<Self> {
        let message = Self {
            to: to.into(),
            content: String::new(),
            template: Some(template),
            template_name: None,
        };
        message.validate()?;
        Ok(message)
    }

    /// Builds a message using a business template configured on the selected profile.
    pub fn named_template<K: Into<String>, V: Into<String>>(
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<Self> {
        let name = name.into();
        let mut message = Self::template(
            to,
            SmsTemplate::named(
                &name,
                parameters
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            ),
        )?;
        message.template_name = Some(name);
        Ok(message)
    }

    pub fn to(&self) -> &str {
        &self.to
    }

    /// Returns the text body, which is empty for a template message.
    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn approved_template(&self) -> Option<&SmsTemplate> {
        self.template.as_ref()
    }

    pub(in crate::notifications) fn validate(&self) -> NotificationResult<()> {
        international_number(&self.to)?;
        if let Some(name) = &self.template_name
            && (name.trim().is_empty() || self.template.is_none())
        {
            return Err(delivery("The configured SMS template name is invalid."));
        }
        match &self.template {
            Some(template) if !template.id.trim().is_empty() && self.content.is_empty() => Ok(()),
            Some(_) => Err(delivery(
                "The SMS template requires an ID and cannot include a text body.",
            )),
            None if self.content.trim().is_empty() => {
                Err(delivery("The SMS content cannot be empty."))
            }
            None => Ok(()),
        }
    }
}

pub(in crate::notifications) fn international_number(number: &str) -> NotificationResult<&str> {
    let number = number.trim();
    let digits = number.strip_prefix('+').unwrap_or(number);
    if !(7..=15).contains(&digits.len())
        || digits.starts_with('0')
        || !digits.bytes().all(|digit| digit.is_ascii_digit())
    {
        return Err(delivery(
            "The SMS recipient must be an international phone number.",
        ));
    }
    Ok(digits)
}
