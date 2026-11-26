use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;
use url::Url;

use crate::{
    http::client::aliyun_rpc::AliyunRpcSigner,
    notifications::{NotificationFuture, NotificationResult, delivery},
};

use super::{
    super::{SmsDriver, SmsMessage, SmsTemplateParameters},
    rejected, request, required, response, rpc_endpoint, setting, unknown,
};

pub(super) struct Aliyun {
    client: Client,
    sign_name: String,
}

impl Aliyun {
    pub(super) fn new(profile: &Value) -> NotificationResult<Self> {
        Ok(Self {
            client: Client::new(profile, "https://dysmsapi.aliyuncs.com")?,
            sign_name: required(profile, "sign_name")?,
        })
    }
}

impl SmsDriver for Aliyun {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            let Some(template) = message.approved_template() else {
                return Err(delivery("Alibaba Cloud SMS requires an approved template."));
            };
            let SmsTemplateParameters::Named(parameters) = &template.parameters else {
                return Err(delivery(
                    "Alibaba Cloud SMS templates require named parameters.",
                ));
            };
            let number = super::Market::Mainland.number(message.to())?;
            let phone = &number[2..];
            let variables = serde_json::to_string(parameters).map_err(delivery)?;
            let payload = self
                .client
                .send(
                    "SendSms",
                    "2017-05-25",
                    [
                        ("PhoneNumbers", phone),
                        ("SignName", self.sign_name.as_str()),
                        ("TemplateCode", template.id.as_str()),
                        ("TemplateParam", variables.as_str()),
                    ],
                )
                .await?;
            if payload["Code"].as_str().is_some_and(|code| code != "OK") {
                return Err(rejected(
                    "Alibaba Cloud SMS did not accept the submission.",
                    payload["Code"].as_str(),
                ));
            }
            if payload["Code"] != "OK" || payload["BizId"].as_str().is_none_or(str::is_empty) {
                return Err(unknown(
                    "Alibaba Cloud SMS did not accept or confirm the submission.",
                ));
            }
            Ok(payload)
        })
    }
}

pub(super) struct AliyunInternational {
    client: Client,
    sender_id: Option<String>,
    message_type: Option<String>,
}

impl AliyunInternational {
    pub(super) fn new(profile: &Value) -> NotificationResult<Self> {
        let sender_id = setting(profile, "sender_id");
        if let Some(sender) = &sender_id {
            let max = if sender.bytes().all(|byte| byte.is_ascii_digit()) {
                15
            } else {
                11
            };
            if sender.len() > max || !sender.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                return Err(delivery(
                    "The Alibaba Cloud international SMS sender ID is invalid.",
                ));
            }
        }
        let message_type = setting(profile, "message_type");
        if let Some(value) = message_type.as_deref()
            && !matches!(value, "OTP" | "NOTIFY" | "MKT")
        {
            return Err(delivery(
                "The Alibaba Cloud international SMS type must be OTP, NOTIFY, or MKT.",
            ));
        }
        Ok(Self {
            client: Client::new(profile, "https://dysmsapi.ap-southeast-1.aliyuncs.com")?,
            sender_id,
            message_type,
        })
    }
}

impl SmsDriver for AliyunInternational {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            message.validate()?;
            if message.approved_template().is_some() {
                return Err(delivery(
                    "Alibaba Cloud international SMS requires a text body.",
                ));
            }
            let to = super::Market::International.number(message.to())?;
            let mut parameters = vec![("Message", message.content()), ("To", to.as_str())];
            if let Some(sender) = &self.sender_id {
                parameters.push(("From", sender));
            }
            if let Some(kind) = &self.message_type {
                parameters.push(("Type", kind));
            }
            let payload = self
                .client
                .send("SendMessageToGlobe", "2018-05-01", parameters)
                .await?;
            if payload["ResponseCode"]
                .as_str()
                .is_some_and(|code| code != "OK")
            {
                return Err(rejected(
                    "Alibaba Cloud international SMS did not accept the submission.",
                    payload["ResponseCode"].as_str(),
                ));
            }
            if payload["ResponseCode"] != "OK"
                || payload["MessageId"].as_str().is_none_or(str::is_empty)
            {
                return Err(unknown(
                    "Alibaba Cloud international SMS did not accept or confirm the submission.",
                ));
            }
            Ok(payload)
        })
    }
}

struct Client {
    endpoint: Url,
    access_key_id: String,
    access_key_secret: String,
}

impl Client {
    fn new(profile: &Value, default_endpoint: &str) -> NotificationResult<Self> {
        Ok(Self {
            endpoint: rpc_endpoint(profile, default_endpoint)?,
            access_key_id: required(profile, "access_key_id")?,
            access_key_secret: required(profile, "access_key_secret")?,
        })
    }

    async fn send<'a>(
        &self,
        action: &str,
        version: &str,
        parameters: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> NotificationResult<Value> {
        const ESCAPE: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let mut fields = parameters.into_iter().collect::<Vec<_>>();
        fields.sort_unstable_by_key(|(key, _)| *key);
        let query = fields
            .into_iter()
            .map(|(name, value)| format!("{name}={}", utf8_percent_encode(value, ESCAPE)))
            .collect::<Vec<_>>()
            .join("&");
        let mut url = self.endpoint.clone();
        url.set_query(Some(&query));
        let headers = AliyunRpcSigner {
            access_key_id: &self.access_key_id,
            access_key_secret: self.access_key_secret.as_bytes(),
            security_token: None,
        }
        .sign_post(&url, action, version, None, b"")
        .map_err(|_| delivery("The SMS profile contains an invalid request header."))?;
        response(
            request()?
                .with_headers(headers)
                .send(http::Method::POST, url.as_str())
                .await,
        )
    }
}
