mod entry_details;

use crate::{EntryResult, EntryType};
use openrust::{
    console::Output,
    support::{
        Str, php_float_cast, php_float_string, php_integer_cast, php_round_half_up,
        php_value_truthy,
    },
};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

fn text(content: &Value, key: &str) -> String {
    match &content[key] {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}
fn limit(value: &str, length: usize, full: bool) -> String {
    if full {
        value.to_owned()
    } else {
        Str::limit(value, length, "...", false)
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
                if kind == Some(EntryType::ClientRequest) && c["response_status"].is_null() {
                    "N/A".into()
                } else {
                    php_integer_cast(&c["response_status"]).to_string()
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
            Some(EntryType::Cache) => vec![
                cache_action(&t("type")).to_owned(),
                limit(&t("key"), 50, false),
            ],
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
            entry.created_at
        ));
    }
}
pub(super) fn show(
    output: &mut Output,
    entry: &EntryResult,
    batch_id: Uuid,
    batch: &[EntryResult],
    kinds: &[EntryType],
    full: bool,
) {
    output.info(&single_line(&entry_details::heading(entry)));
    output.stdout.push_str(&format!(
        "UUID\t{}\nTime\t{}\nBatch\t{}\n",
        entry.id, entry.created_at, entry.batch_id
    ));
    if !entry.tags.is_empty() {
        output
            .stdout
            .push_str(&format!("Tags\t{}\n", entry.tags.join(", ")));
    }
    entry_details::render(output, entry, full);
    show_batch(output, batch_id, batch, kinds, full);
}

fn show_batch(
    output: &mut Output,
    batch_id: Uuid,
    batch: &[EntryResult],
    kinds: &[EntryType],
    full: bool,
) {
    if batch.is_empty() {
        if !kinds.is_empty() {
            let requested = kinds.iter().map(|kind| kind.as_str()).collect::<Vec<_>>();
            output.info(&format!(
                "No batch entries of type {}.",
                requested.join(", ")
            ));
        }
        return;
    }

    output.info(&format!(
        "Related Entries - batch {}",
        &batch_id.to_string()[..8]
    ));
    let mut groups: Vec<(EntryType, Vec<&EntryResult>)> = Vec::new();
    for entry in batch {
        if let Some((_, entries)) = groups.iter_mut().find(|(kind, _)| *kind == entry.r#type) {
            entries.push(entry);
        } else {
            groups.push((entry.r#type, vec![entry]));
        }
    }
    for kind in [
        EntryType::Query,
        EntryType::Exception,
        EntryType::Cache,
        EntryType::Log,
    ] {
        if let Some(index) = groups.iter().position(|(group, _)| *group == kind) {
            let (_, rows) = groups.remove(index);
            let count = rows.len();
            match kind {
                EntryType::Query => show_batch_queries(output, &rows, full),
                EntryType::Exception => {
                    output.info(&format!("Exceptions - {count}"));
                    write_row(output, ["UUID", "Exception", "Location"]);
                    for entry in rows.iter().take(10) {
                        let c = &entry.content;
                        write_row(
                            output,
                            [
                                &entry.id.to_string()[..8],
                                &format!(
                                    "{}: {}",
                                    text(c, "class"),
                                    limit(&text(c, "message"), 80, full)
                                ),
                                &format!("{}:{}", text(c, "file"), text(c, "line")),
                            ],
                        );
                    }
                    more(output, count, 10, "exceptions");
                }
                EntryType::Cache => show_batch_cache(output, &rows, full),
                EntryType::Log => {
                    output.info(&format!("Logs - {count}"));
                    write_row(output, ["Level", "Message"]);
                    for entry in rows.iter().take(10) {
                        write_row(
                            output,
                            [
                                &text(&entry.content, "level"),
                                &limit(&text(&entry.content, "message"), 80, full),
                            ],
                        );
                    }
                    more(output, count, 10, "logs");
                }
                _ => unreachable!(),
            }
        }
    }
    for (kind, rows) in groups {
        let label = Str::plural(&Str::headline(kind.as_str()), 2);
        output.info(&format!("{label} ({})", rows.len()));
        for entry in rows.iter().take(5) {
            output.info(&summary(entry, full));
        }
        more(output, rows.len(), 5, &label);
    }
}

fn query_identity(content: &Value) -> String {
    if content["hash"].is_null() {
        text(content, "sql")
    } else {
        text(content, "hash")
    }
}

fn show_batch_queries(output: &mut Output, rows: &[&EntryResult], full: bool) {
    let mut duplicates = HashMap::new();
    let mut total = 0.0;
    let mut slow = 0;
    for entry in rows {
        let c = &entry.content;
        total += php_float_cast(&c["time"]);
        slow += usize::from(php_value_truthy(&c["slow"]));
        *duplicates.entry(query_identity(c)).or_insert(0) += 1;
    }
    let groups = duplicates.values().filter(|count| **count > 1).count();
    let time = php_float_string(php_round_half_up(total, 100.0));
    let mut heading = format!("Queries - {} total, {time}ms", rows.len());
    if slow > 0 {
        heading.push_str(&format!(", {slow} slow"));
    }
    if groups > 0 {
        let noun = if groups == 1 { "group" } else { "groups" };
        heading.push_str(&format!(", {groups} duplicate {noun}"));
    }
    output.info(&heading);
    write_row(output, ["#", "UUID", "Time", "SQL", "Source", "Flags"]);
    for (index, entry) in rows.iter().take(20).enumerate() {
        let c = &entry.content;
        let time = php_float_string(php_round_half_up(php_float_cast(&c["time"]), 100.0));
        let source = c["file"].as_str().map_or_else(String::new, |file| {
            let filename = file.rsplit(['/', '\\']).next().unwrap_or(file);
            format!("{filename}:{}", text(c, "line"))
        });
        let slow = php_value_truthy(&c["slow"]);
        let duplicate = duplicates
            .get(&query_identity(c))
            .is_some_and(|count| *count > 1);
        let flags = match (slow, duplicate) {
            (true, true) => "SLOW DUP",
            (true, false) => "SLOW",
            (false, true) => "DUP",
            (false, false) => "",
        };
        write_row(
            output,
            [
                &(index + 1).to_string(),
                &entry.id.to_string()[..8],
                &format!("{time}ms"),
                &limit(&text(c, "sql"), 60, full),
                &source,
                flags,
            ],
        );
    }
    more(output, rows.len(), 20, "queries");
}

fn show_batch_cache(output: &mut Output, rows: &[&EntryResult], full: bool) {
    let mut hits = 0_u32;
    let mut misses = 0_u32;
    for entry in rows {
        match entry.content["type"].as_str() {
            Some("hit") => hits += 1,
            Some("missed") => misses += 1,
            _ => {}
        }
    }
    let mut heading = format!("Cache - {hits} hits, {misses} misses");
    if hits + misses > 0 {
        let rate = f64::from(hits) / f64::from(hits + misses) * 100.0;
        let rate = php_float_string(php_round_half_up(rate, 10.0));
        heading.push_str(&format!(" - {rate}% hit rate"));
    }
    output.info(&heading);
    write_row(output, ["Action", "Key"]);
    for entry in rows.iter().take(10) {
        let c = &entry.content;
        write_row(
            output,
            [
                cache_action(&text(c, "type")),
                &limit(&text(c, "key"), 60, full),
            ],
        );
    }
    more(output, rows.len(), 10, "cache entries");
}

fn cache_action(action: &str) -> &str {
    match action {
        "hit" => "HIT",
        "missed" => "MISS",
        "set" => "SET",
        "forget" => "FORGET",
        other => other,
    }
}

fn write_row<const N: usize>(output: &mut Output, values: [&str; N]) {
    output.stdout.push_str(&values.map(single_line).join("\t"));
    output.stdout.push('\n');
}

fn more(output: &mut Output, count: usize, limit: usize, label: &str) {
    if count > limit {
        output.info(&format!("... and {} more {label}", count - limit));
    }
}
