use crate::{EntryResult, EntryType};
use openrust::console::Output;
use serde_json::Value;

fn text(content: &Value, key: &str) -> String {
    match &content[key] {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}
fn limit(value: &str, length: usize, full: bool) -> String {
    if full || value.chars().count() <= length {
        value.to_owned()
    } else {
        format!("{}...", value.chars().take(length).collect::<String>())
    }
}
fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn unit(content: &Value, key: &str, suffix: &str) -> String {
    let value = text(content, key);
    if value.is_empty() {
        value
    } else {
        format!("{value}{suffix}")
    }
}
fn basename(value: &str) -> &str {
    value.rsplit(['\\', ':']).next().unwrap_or(value)
}
fn summary(entry: &EntryResult, full: bool) -> String {
    let c = &entry.content;
    let t = |key| text(c, key);
    let value = match entry.r#type {
        EntryType::Request => format!(
            "{} {} -> {} ({})",
            t("method"),
            t("uri"),
            t("response_status"),
            unit(c, "duration", "ms")
        ),
        EntryType::Exception => format!("{}: {}", t("class"), t("message")),
        EntryType::Query => format!("{} ({})", t("sql"), unit(c, "time", "ms")),
        EntryType::Job => format!("{} [{}]", basename(&t("name")), t("status")),
        EntryType::Cache => format!("{} {}", t("type"), t("key")),
        EntryType::Log => format!("[{}] {}", t("level"), t("message")),
        EntryType::Mail => t("subject"),
        EntryType::Event => format!(
            "{} ({} listeners)",
            t("name"),
            c["listeners"].as_array().map_or(0, Vec::len)
        ),
        EntryType::Model => format!("{}: {}", t("model"), t("action")),
        EntryType::Gate => format!("{}: {}", t("ability"), t("result")),
        EntryType::View => t("name"),
        EntryType::Notification => format!("{} via {}", basename(&t("notification")), t("channel")),
        EntryType::Redis => t("command"),
        EntryType::ClientRequest => format!(
            "{} {} -> {}",
            t("method"),
            t("uri"),
            if c["response_status"].is_null() {
                "N/A".into()
            } else {
                t("response_status")
            }
        ),
        EntryType::Command => t("command"),
        EntryType::ScheduledTask => format!("{} [{}]", t("command"), t("expression")),
        EntryType::Dump => t("dump"),
        EntryType::Batch => c.to_string(),
    };
    single_line(&limit(&value, 80, full))
}
pub(super) fn list(output: &mut Output, entries: &[EntryResult], kind: Option<EntryType>) {
    let headers: &[&str] = match kind {
        Some(EntryType::Request | EntryType::ClientRequest) => {
            &["Method", "URI", "Status", "Duration"]
        }
        Some(EntryType::Query) => &["SQL", "Time", "Slow", "Connection"],
        Some(EntryType::Exception) => &["Class", "Message", "Occurrences"],
        Some(EntryType::Job) => &["Name", "Queue", "Status"],
        Some(EntryType::Cache) => &["Action", "Key"],
        Some(EntryType::Log) => &["Level", "Message"],
        Some(EntryType::Mail) => &["Mailable", "Subject", "To"],
        Some(EntryType::Command) => &["Command", "Exit Code"],
        _ => &["Type", "Summary"],
    };
    output
        .stdout
        .push_str(&format!("UUID\t{}\tCreated\n", headers.join("\t")));
    for entry in entries {
        let c = &entry.content;
        let t = |key| text(c, key);
        let fields = match kind {
            Some(EntryType::Request | EntryType::ClientRequest) => vec![
                t("method"),
                limit(&t("uri"), 40, false),
                if c["response_status"].is_null() {
                    "N/A".into()
                } else {
                    t("response_status")
                },
                unit(c, "duration", "ms"),
            ],
            Some(EntryType::Query) => vec![
                limit(&t("sql"), 60, false),
                unit(c, "time", "ms"),
                if c["slow"] == true {
                    "Yes".into()
                } else {
                    "No".into()
                },
                t("connection"),
            ],
            Some(EntryType::Exception) => vec![
                basename(&t("class")).into(),
                limit(&t("message"), 50, false),
                if c["occurrences"].is_null() {
                    "1".into()
                } else {
                    t("occurrences")
                },
            ],
            Some(EntryType::Job) => vec![basename(&t("name")).into(), t("queue"), t("status")],
            Some(EntryType::Cache) => vec![t("type"), limit(&t("key"), 50, false)],
            Some(EntryType::Log) => vec![t("level"), limit(&t("message"), 60, false)],
            Some(EntryType::Mail) => vec![
                basename(&t("mailable")).into(),
                limit(&t("subject"), 40, false),
                limit(
                    &c["to"]
                        .as_object()
                        .map(|v| v.keys().cloned().collect::<Vec<_>>().join(", "))
                        .unwrap_or_default(),
                    30,
                    false,
                ),
            ],
            Some(EntryType::Command) => vec![limit(&t("command"), 50, false), t("exit_code")],
            _ => vec![entry.r#type.as_str().into(), summary(entry, false)],
        };
        output.stdout.push_str(&format!(
            "{}\t{}\t{}\n",
            &entry.id.to_string()[..8],
            fields
                .iter()
                .map(|value| single_line(value))
                .collect::<Vec<_>>()
                .join("\t"),
            entry.createdAt
        ));
    }
}
pub(super) fn show(output: &mut Output, entry: &EntryResult, batch: &[EntryResult], full: bool) {
    output.info(&format!(
        "{}: {}",
        entry.r#type.as_str(),
        summary(entry, full)
    ));
    output.stdout.push_str(&format!(
        "UUID\t{}\nTime\t{}\nBatch\t{}\n",
        entry.id, entry.createdAt, entry.batchId
    ));
    if !entry.tags.is_empty() {
        output
            .stdout
            .push_str(&format!("Tags\t{}\n", entry.tags.join(", ")));
    }
    if let Some(content) = entry.content.as_object() {
        for (key, value) in content {
            if value.is_null() {
                continue;
            }
            output.info(key);
            let value = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| serde_json::to_string_pretty(value).expect("JSON value"));
            output.stdout.push_str(&single_line(&limit(
                &value,
                if key == "response" { 1000 } else { 500 },
                full,
            )));
            output.stdout.push('\n');
        }
    } else {
        output
            .stdout
            .push_str(&limit(&entry.content.to_string(), 1000, full));
        output.stdout.push('\n');
    }
    if !batch.is_empty() {
        output.info("Related Entries");
    }
    for kind in EntryType::all() {
        let related = batch
            .iter()
            .filter(|entry| entry.r#type == *kind)
            .collect::<Vec<_>>();
        if related.is_empty() {
            continue;
        }
        output.info(&format!("{} ({})", kind.as_str(), related.len()));
        let max = match kind {
            EntryType::Query => 20,
            EntryType::Exception | EntryType::Cache | EntryType::Log => 10,
            _ => 5,
        };
        for entry in related.iter().take(max) {
            output.stdout.push_str(&format!(
                "{}\t{}\n",
                &entry.id.to_string()[..8],
                summary(entry, full)
            ));
        }
        if related.len() > max {
            output.stdout.push_str(&format!(
                "... and {} more {} entries\n",
                related.len() - max,
                kind.as_str()
            ));
        }
    }
}
