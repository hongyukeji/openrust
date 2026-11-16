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
}

impl SesTransport {
    pub(in crate::mail) fn new(settings: SesSettings) -> MailResult<Self> {
        if settings.key.is_some() != settings.secret.is_some() {
            return Err(transport("SES key and secret must be provided together."));
        }
        Ok(Self {
            settings,
            client: OnceCell::new(),
        })
    }

    async fn client(&self) -> &Client {
        self.client
            .get_or_init(|| async {
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
                Client::new(&config.load().await)
            })
            .await
    }
}

impl Transport for SesTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
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
                .client()
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
            let id = response.message_id();
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
        })
    }
}
