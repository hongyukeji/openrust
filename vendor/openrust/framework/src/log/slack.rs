use super::{Level, LogError, LogRecord, LogResult, RecordHandler, webhook::Webhook};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[derive(Deserialize)]
struct Options {
    url: String,
    username: Option<String>,
    channel: Option<String>,
    emoji: Option<String>,
    attachment: Option<bool>,
    short: Option<bool>,
    context: Option<bool>,
    exclude_fields: Option<Vec<String>>,
    timeout: Option<f64>,
}
pub(super) fn writer(config: &Value, default_username: &str) -> LogResult<Arc<RecordHandler>> {
    let options: Options =
        serde_json::from_value(config.clone()).map_err(|_| LogError::InvalidConfiguration)?;
    let url = reqwest::Url::parse(&options.url).map_err(|_| LogError::InvalidConfiguration)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(LogError::InvalidConfiguration);
    }
    let timeout = Duration::try_from_secs_f64(options.timeout.unwrap_or(10.0))
        .map_err(|_| LogError::InvalidConfiguration)?;
    if timeout.is_zero() {
        return Err(LogError::InvalidConfiguration);
    }
    let worker = Webhook::new(url, timeout)?;
    let username = options
        .username
        .clone()
        .unwrap_or_else(|| default_username.to_owned());
    let icon = options
        .emoji
        .clone()
        .unwrap_or_else(|| ":boom:".into())
        .trim_matches(':')
        .to_owned();
    Ok(Arc::new(move |record, formatted| {
        worker.send(payload(&options, &username, &icon, record, formatted))
    }))
}
fn payload(
    options: &Options,
    username: &str,
    icon: &str,
    record: &LogRecord<'_>,
    formatted: &str,
) -> Value {
    let mut output = json!({"username":username});
    if let Some(channel) = &options.channel {
        output["channel"] = channel.clone().into();
    }
    if options.attachment.unwrap_or(true) {
        let (context, extra) = super::json::normalized_fields(record);
        let mut data = json!({"context":context,"extra":extra});
        for path in options.exclude_fields.iter().flatten() {
            exclude(&mut data, path);
        }
        let short = options.short.unwrap_or(false);
        let message = record.message();
        let mut attachment = json!({"fallback":message,"text":message,"color":match record.level {
            Level::Emergency|Level::Alert|Level::Critical|Level::Error=>"danger",Level::Warning=>"warning",Level::Notice|Level::Info=>"good",Level::Debug=>"#e3e4e6"
        },"fields":[],"mrkdwn_in":["fields"],"ts":record.at.as_second(),"footer":username,"footer_icon":icon});
        attachment["title"] = if short {
            record.level.label()
        } else {
            "Message"
        }
        .into();
        let fields = attachment["fields"].as_array_mut().expect("附件字段为数组");
        if !short {
            fields.push(field("Level", record.level.label().into()));
        }
        if options.context.unwrap_or(true) {
            for key in ["extra", "context"] {
                let Some(values) = data
                    .get(key)
                    .and_then(Value::as_object)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                if short {
                    fields.push(field(key, Value::Object(values.clone())));
                } else {
                    fields.extend(values.iter().map(|(key, value)| field(key, value.clone())));
                }
            }
        }
        output["attachments"] = json!([attachment]);
    } else {
        output["text"] = formatted.into();
    }
    if reqwest::Url::parse(icon).is_ok() {
        output["icon_url"] = icon.into();
    } else {
        output["icon_emoji"] = format!(":{icon}:").into();
    }
    output
}
fn exclude(data: &mut Value, path: &str) {
    let parts: Vec<_> = path.split('.').collect();
    let Some(last) = parts.last() else {
        return;
    };
    let mut node = &*data;
    for (index, key) in parts.iter().enumerate() {
        let child = match node {
            Value::Object(values) => values.get(*key),
            Value::Array(values) => key
                .parse::<usize>()
                .ok()
                .and_then(|index| values.get(index)),
            _ => None,
        };
        let Some(child) = child.filter(|child| !child.is_null()) else {
            return;
        };
        // 保留外部处理器的isset和首个同名末段语义，不展开通配符。
        if key == last {
            crate::support::data_forget_parts(data, &parts[..=index], false);
            return;
        }
        node = child;
    }
}

fn field(key: &str, value: Value) -> Value {
    let value = if value.is_object() || value.is_array() {
        let compact = value
            .as_array()
            .is_some_and(|values| !values.iter().any(|v| v.is_array() || v.is_object()));
        let text = if compact {
            value.to_string()
        } else {
            let mut bytes = Vec::new();
            let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
            value
                .serialize(&mut serde_json::Serializer::with_formatter(
                    &mut bytes, formatter,
                ))
                .expect("JSON字段可序列化");
            String::from_utf8(bytes).expect("JSON编码使用UTF8")
        };
        format!(
            "```{}```",
            String::from_utf8_lossy(&text.as_bytes()[..text.len().min(1990)])
        )
        .into()
    } else {
        value
    };
    let title = if key.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
        format!(
            "{}{}",
            char::from(key.as_bytes()[0].to_ascii_uppercase()),
            &key[1..]
        )
    } else {
        key.to_owned()
    };
    json!({"title":title,"value":value,"short":false})
}
