mod formats_output;
mod install;
use crate::{EntryType, storage::EntryQueryOptions};
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::facades::Artisan,
};
use serde_json::json;
use uuid::Uuid;

fn error(source: impl std::error::Error + Send + Sync + 'static) -> ConsoleError {
    ConsoleError::new(source.to_string())
}
pub(crate) fn register() -> Result<(), ConsoleError> {
    for (name, about) in [
        (
            "telescope:install",
            "Install all of the Telescope resources",
        ),
        ("telescope:clear", "Clear all entries from Telescope"),
        (
            "telescope:prune",
            "Prune stale entries from the Telescope database",
        ),
        ("telescope:pause", "Pause all Telescope watchers"),
        ("telescope:resume", "Resume all Telescope watchers"),
        (
            "telescope:publish",
            "Publish all of the Telescope resources",
        ),
        ("telescope:list", "List Telescope entries"),
        (
            "telescope:show",
            "Show a Telescope entry with full batch context",
        ),
    ] {
        let mut syntax = Syntax::new(name).about(about);
        match name {
            "telescope:prune" => {
                syntax = syntax
                    .arg(Arg::new("hours").long("hours").default_value("24"))
                    .arg(
                        Arg::new("keep-exceptions")
                            .long("keep-exceptions")
                            .action(ArgAction::SetTrue),
                    )
            }
            "telescope:publish" => {
                syntax = syntax.arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
            }
            "telescope:list" => {
                syntax = syntax
                    .arg(Arg::new("type"))
                    .arg(Arg::new("limit").long("limit").default_value("20"));
                for option in ["tag", "batch", "family", "before"] {
                    syntax = syntax.arg(Arg::new(option).long(option));
                }
                syntax = syntax.arg(Arg::new("json").long("json").action(ArgAction::SetTrue));
            }
            "telescope:show" => {
                syntax = syntax
                    .arg(Arg::new("id").required(true))
                    .arg(Arg::new("type").long("type"))
                    .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
                    .arg(Arg::new("full").long("full").action(ArgAction::SetTrue))
            }
            _ => {}
        }
        Artisan::add(Box::new(TelescopeCommand {
            definition: CommandDefinition::new(syntax),
            name,
        }))?;
    }
    Ok(())
}
struct TelescopeCommand {
    definition: CommandDefinition,
    name: &'static str,
}
fn validType(value: &str, output: &mut Output) -> Option<EntryType> {
    match value.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            output.error(&format!("Invalid entry type: {value}"));
            output.stdout.push_str(&format!(
                "Valid types: {}\n",
                EntryType::all()
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            None
        }
    }
}
fn jsonOutput(output: &mut Output, value: &impl serde::Serialize) -> Result<(), ConsoleError> {
    output
        .stdout
        .push_str(&serde_json::to_string_pretty(value).map_err(error)?);
    output.stdout.push('\n');
    Ok(())
}
impl Command for TelescopeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let matches = input.matches();
            let value = |key| matches.get_one::<String>(key).map(String::as_str);
            match self.name {
                "telescope:install" => return install::install(app, output).await,
                "telescope:publish" => {
                    let mut args = vec![std::ffi::OsString::from("--tag=telescope-config")];
                    if matches.get_flag("force") {
                        args.push("--force".into());
                    }
                    return Artisan::call("vendor:publish", &args, output).await;
                }
                "telescope:pause" => {
                    if !matches!(
                        openrust::cache::Cache::get("telescope:pause-recording")
                            .await
                            .map_err(error)?,
                        Some(openrust::cache::Value::Json(serde_json::Value::Bool(true)))
                    ) {
                        openrust::cache::Cache::put(
                            "telescope:pause-recording",
                            &openrust::cache::Value::Json(json!(true)),
                            Some(openrust::cache::Ttl::Seconds(30 * 24 * 60 * 60)),
                        )
                        .await
                        .map_err(error)?;
                    }
                    output.info("Telescope watchers paused successfully.");
                    return Ok(0);
                }
                "telescope:resume" => {
                    if openrust::cache::Cache::get("telescope:pause-recording")
                        .await
                        .map_err(error)?
                        .is_some()
                    {
                        openrust::cache::Cache::forget("telescope:pause-recording")
                            .await
                            .map_err(error)?;
                    }
                    output.info("Telescope watchers resumed successfully.");
                    return Ok(0);
                }
                _ => {}
            }
            let repository = app
                .make::<dyn crate::contracts::EntriesRepository>()
                .await
                .map_err(error)?;
            match self.name {
                "telescope:clear" => {
                    repository.clear().await.map_err(error)?;
                    output.info("Telescope entries cleared!");
                }
                "telescope:prune" => {
                    let Some(hours) = value("hours")
                        .and_then(|v| v.parse::<i64>().ok())
                        .filter(|v| *v >= 0)
                        .and_then(|v| v.checked_mul(3600))
                    else {
                        output.error("The --hours option must be a non-negative integer.");
                        return Ok(1);
                    };
                    let before = jiff::Timestamp::now()
                        .checked_sub(std::time::Duration::from_secs(hours as u64))
                        .map_err(error)?;
                    let pruned = repository
                        .prune(before, matches.get_flag("keep-exceptions"))
                        .await
                        .map_err(error)?;
                    output.info(&format!("{pruned} entries pruned."));
                }
                "telescope:list" => {
                    let kind = if let Some(kind) = value("type").filter(|s| !s.is_empty()) {
                        let Some(kind) = validType(kind, output) else {
                            return Ok(1);
                        };
                        Some(kind)
                    } else {
                        None
                    };
                    let Some(limit) = value("limit")
                        .filter(|v| v.bytes().all(|v| v.is_ascii_digit()))
                        .and_then(|v| v.parse::<u64>().ok())
                        .filter(|v| *v > 0)
                    else {
                        output.error("The --limit option must be a positive integer.");
                        return Ok(1);
                    };
                    let batch = value("batch")
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(error)?;
                    let before = value("before")
                        .map(str::parse::<i64>)
                        .transpose()
                        .map_err(error)?;
                    let entries = repository
                        .get(
                            kind,
                            EntryQueryOptions::default()
                                .limit(limit)
                                .tag(value("tag").map(str::to_owned))
                                .batchId(batch)
                                .familyHash(value("family").map(str::to_owned))
                                .beforeSequence(before),
                        )
                        .await
                        .map_err(error)?;
                    if matches.get_flag("json") {
                        jsonOutput(output, &entries)?;
                    } else if entries.is_empty() {
                        output.comment("No entries found.");
                    } else {
                        formats_output::list(output, &entries, kind);
                        let next = if entries.len() as u64 >= limit {
                            format!(
                                "Use --before={} for next page",
                                entries.last().and_then(|entry| entry.sequence).unwrap_or(0)
                            )
                        } else {
                            "No more entries".into()
                        };
                        output.info(&format!(
                            "Showing {} {} - {next}",
                            entries.len(),
                            if entries.len() == 1 {
                                "entry"
                            } else {
                                "entries"
                            }
                        ));
                    }
                }
                "telescope:show" => {
                    let id = value("id").expect("required");
                    let entry = if id == "latest" || id.starts_with("latest:") {
                        let kind = if let Some(kind) =
                            id.strip_prefix("latest:").filter(|s| !s.is_empty())
                        {
                            let Some(kind) = validType(kind, output) else {
                                return Ok(1);
                            };
                            Some(kind)
                        } else {
                            None
                        };
                        let Some(entry) = repository
                            .get(kind, EntryQueryOptions::default().limit(1))
                            .await
                            .map_err(error)?
                            .into_iter()
                            .next()
                        else {
                            output.error(&format!(
                                "No {}entries found.",
                                kind.map(|v| format!("{} ", v.as_str())).unwrap_or_default()
                            ));
                            return Ok(1);
                        };
                        entry
                    } else {
                        match repository.find(id).await {
                            Ok(entry) => entry,
                            Err(crate::Error::NotFound) => {
                                output.error(&format!("Entry not found: {id}"));
                                return Ok(1);
                            }
                            Err(source) => return Err(error(source)),
                        }
                    };
                    let mut kinds = Vec::new();
                    for kind in value("type")
                        .unwrap_or("")
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        let Some(kind) = validType(kind, output) else {
                            return Ok(1);
                        };
                        kinds.push(kind);
                    }
                    let batchId = entry
                        .content
                        .get("updated_batch_id")
                        .and_then(|v| v.as_str())
                        .and_then(|v| Uuid::parse_str(v).ok())
                        .unwrap_or(entry.batchId);
                    let mut batch = repository
                        .get(
                            None,
                            EntryQueryOptions::forBatchId(Some(batchId)).limit(u64::MAX),
                        )
                        .await
                        .map_err(error)?;
                    batch.reverse();
                    batch.retain(|other| {
                        other.id != entry.id && (kinds.is_empty() || kinds.contains(&other.r#type))
                    });
                    if matches.get_flag("json") {
                        jsonOutput(output, &json!({"entry":entry,"batch":batch}))?;
                    } else {
                        formats_output::show(output, &entry, &batch, matches.get_flag("full"));
                    }
                }
                _ => unreachable!(),
            }
            Ok(0)
        })
    }
}
