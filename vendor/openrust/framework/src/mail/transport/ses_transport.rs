use aws_config::BehaviorVersion;
use aws_sdk_ses::{
    Client,
    config::{Credentials, Region},
    primitives::Blob,
    types::{MessageTag, RawMessage},
};
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::mail::{MailFuture, MailResult, Message, SentMessage, transport};

use super::Transport;

#[derive(Deserialize)]
pub(in crate::mail) struct SesSettings {
    key: Option<String>,
    secret: Option<String>,
    token: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
    #[serde(default)]
    options: Options,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Options {
    configuration_set_name: Option<String>,
    from_arn: Option<String>,
    source_arn: Option<String>,
    return_path_arn: Option<String>,
    tags: Option<Vec<Tag>>,
    email_tags: Option<Vec<Tag>>,
    from_email_address_identity_arn: Option<String>,
    feedback_forwarding_email_address: Option<String>,
    feedback_forwarding_email_address_identity_arn: Option<String>,
    tenant_name: Option<String>,
    list_management_options: Option<ListManagement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListManagement {
    contact_list_name: String,
    topic_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Tag {
    name: String,
    value: String,
}

pub(in crate::mail) struct SesTransport {
    settings: SesSettings,
    client: OnceCell<Client>,
    v2_client: OnceCell<aws_sdk_sesv2::Client>,
    use_v2: bool,
}

impl SesTransport {
    pub(in crate::mail) fn new(settings: SesSettings, use_v2: bool) -> MailResult<Self> {
        if settings.key.is_some() != settings.secret.is_some() {
            return Err(transport("SES key and secret must be provided together."));
        }
        Ok(Self {
            settings,
            client: OnceCell::new(),
            v2_client: OnceCell::new(),
            use_v2,
        })
    }

    async fn sdk_config(&self) -> aws_config::SdkConfig {
        let settings = &self.settings;
        let mut config = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = &settings.region {
            config = config.region(Region::new(region.clone()));
        }
        if let (Some(key), Some(secret)) = (&settings.key, &settings.secret) {
            config = config.credentials_provider(Credentials::new(
                key,
                secret,
                settings.token.clone(),
                None,
                "mail-ses",
            ));
        }
        if let Some(endpoint) = &settings.endpoint {
            config = config.endpoint_url(endpoint);
        }
        config.load().await
    }

    async fn send_v2(&self, message: &Message) -> MailResult<SentMessage> {
        use aws_sdk_sesv2::types::{
            Destination, EmailContent, ListManagementOptions, MessageTag, RawMessage,
        };

        let options = &self.settings.options;
        let tags = options
            .email_tags
            .as_ref()
            .map(|tags| {
                tags.iter()
                    .map(|tag| {
                        MessageTag::builder()
                            .name(&tag.name)
                            .value(&tag.value)
                            .build()
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
            .map_err(|_| transport("Invalid SES message tags."))?;
        let header = |name: &str| {
            message
                .1
                .headers
                .text
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
                .filter(|value| !value.is_empty())
        };
        let list = header("X-SES-LIST-MANAGEMENT-OPTIONS").and_then(|value| {
            let (name, topic) = value.split_once(';').unwrap_or((value, ""));
            let name = name.trim();
            let name = name
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case("contactListName"))
                .map_or(name, |(_, name)| name);
            let topic = topic
                .trim()
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case("topicName"))
                .map(|(_, topic)| topic.to_owned());
            (!name.is_empty()).then(|| ListManagement {
                contact_list_name: name.to_owned(),
                topic_name: topic,
            })
        });
        let list = list
            .as_ref()
            .or(options.list_management_options.as_ref())
            .map(|list| {
                ListManagementOptions::builder()
                    .contact_list_name(&list.contact_list_name)
                    .set_topic_name(list.topic_name.clone())
                    .build()
            })
            .transpose()
            .map_err(|_| transport("Invalid SES list management options."))?;
        let raw = RawMessage::builder()
            .data(Blob::new(message.formatted()))
            .build()
            .map_err(|_| transport("Could not encode the SES message."))?;
        let client = self
            .v2_client
            .get_or_init(|| async { aws_sdk_sesv2::Client::new(&self.sdk_config().await) })
            .await;
        let response = client
            .send_email()
            .set_from_email_address(message.0.envelope().from().map(ToString::to_string))
            .destination(
                Destination::builder()
                    .set_to_addresses(Some(message.recipients()))
                    .build(),
            )
            .content(EmailContent::builder().raw(raw).build())
            .set_configuration_set_name(options.configuration_set_name.clone())
            .set_from_email_address_identity_arn(options.from_email_address_identity_arn.clone())
            .set_feedback_forwarding_email_address(
                options.feedback_forwarding_email_address.clone(),
            )
            .set_feedback_forwarding_email_address_identity_arn(
                options
                    .feedback_forwarding_email_address_identity_arn
                    .clone(),
            )
            .set_tenant_name(
                header("X-SES-TENANT-NAME")
                    .map(str::to_owned)
                    .or_else(|| options.tenant_name.clone()),
            )
            .set_list_management_options(list)
            .set_email_tags(tags)
            .send()
            .await
            .map_err(|_| transport("SES v2 request failed."))?;
        receipt(message, response.message_id().unwrap_or_default())
    }
}

impl Transport for SesTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            if self.use_v2 {
                return self.send_v2(message).await;
            }
            let options = &self.settings.options;
            let tags = options
                .tags
                .as_ref()
                .map(|tags| {
                    tags.iter()
                        .map(|tag| {
                            MessageTag::builder()
                                .name(&tag.name)
                                .value(&tag.value)
                                .build()
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
                .map_err(|_| transport("Invalid SES message tags."))?;
            let raw = RawMessage::builder()
                .data(Blob::new(message.formatted()))
                .build()
                .map_err(|_| transport("Could not encode the SES message."))?;
            let response = self
                .client
                .get_or_init(|| async { Client::new(&self.sdk_config().await) })
                .await
                .send_raw_email()
                .raw_message(raw)
                .set_source(message.0.envelope().from().map(ToString::to_string))
                .set_destinations(Some(message.recipients()))
                .set_configuration_set_name(options.configuration_set_name.clone())
                .set_from_arn(options.from_arn.clone())
                .set_source_arn(options.source_arn.clone())
                .set_return_path_arn(options.return_path_arn.clone())
                .set_tags(tags)
                .send()
                .await
                .map_err(|_| transport("SES request failed."))?;
            receipt(message, response.message_id())
        })
    }
}

fn receipt(message: &Message, id: &str) -> MailResult<SentMessage> {
    if id.is_empty() || id.chars().any(char::is_control) {
        return Err(transport("SES receipt did not contain a valid message ID."));
    }
    let mut sent = message.clone();
    for header in ["X-Message-ID", "X-SES-Message-ID"] {
        sent.0
            .headers_mut()
            .insert_raw(lettre::message::header::HeaderValue::new(
                lettre::message::header::HeaderName::new_from_ascii_str(header),
                id.into(),
            ));
    }
    Ok(SentMessage { message: sent })
}
