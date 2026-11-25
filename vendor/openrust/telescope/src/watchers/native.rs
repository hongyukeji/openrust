use super::{NativeWatcher, json};
use crate::{EntryType, EntryUpdate, IncomingEntry, Telescope};
use openrust::{contracts::events::Event, events::EventContext};
use serde_json::{Map, Value};
use std::panic::Location;
use uuid::Uuid;

impl NativeWatcher {
    pub(super) fn record_additional(
        &self,
        event: &dyn Event,
        telescope: &Telescope,
        context: Option<&EventContext<'_>>,
    ) -> crate::Result<()> {
        if let Some(event) = event.downcast_ref::<openrust::mail::events::MessageSent>() {
            if self.enabled("MailWatcher") {
                let message = &event.sent.message;
                let envelope = message.envelope();
                let addresses = |values: &[openrust::mail::mailables::Address]| {
                    values
                        .iter()
                        .map(|address| {
                            (
                                address.address.clone(),
                                json!(address.name.as_deref().unwrap_or("")),
                            )
                        })
                        .collect::<Map<String, Value>>()
                };
                let tags = envelope
                    .to
                    .iter()
                    .chain(&envelope.cc)
                    .chain(&envelope.bcc)
                    .map(|address| address.address.clone());
                telescope.record_mail(IncomingEntry::make(json!({
                    "mailable":event.mailable,"queued":event.queued,
                    "from":envelope.from.as_ref().map(|value|addresses(std::slice::from_ref(value))),
                    "replyTo":addresses(&envelope.reply_to),"to":addresses(&envelope.to),
                    "cc":addresses(&envelope.cc),"bcc":addresses(&envelope.bcc),
                    "subject":envelope.subject,
                    "html":message.get_html_body().or_else(||message.get_text_body()),
                    "raw":String::from_utf8_lossy(&message.formatted())
                }).as_object().unwrap().clone())?.tags(tags));
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::notifications::NotificationEvent>()
        {
            if self.enabled("NotificationWatcher") && event.name() == "NotificationSent" {
                use openrust::notifications::channels::ChannelResponse;
                let response = match event.response.as_ref() {
                    Some(ChannelResponse::Custom(value)) => value.clone(),
                    Some(ChannelResponse::Database(value)) => {
                        serde_json::to_value(value).map_err(|_| crate::Error::InvalidEntry)?
                    }
                    Some(ChannelResponse::Sms(value)) => {
                        serde_json::to_value(value).map_err(|_| crate::Error::InvalidEntry)?
                    }
                    Some(ChannelResponse::Mail(value)) => {
                        json!({"recipients":value.message.recipients()})
                    }
                    None => Value::Null,
                };
                telescope.record_notification(IncomingEntry::make(json!({"notification":event.kind,"queued":event.queued,"notifiable":event.notifiable,"channel":event.channel,"response":response}).as_object().unwrap().clone())?.tags([event.notifiable.clone()]));
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::console::events::ScheduledTaskCompleted>()
        {
            if self.enabled("ScheduleWatcher") {
                let output = event
                    .output
                    .as_ref()
                    .filter(|_| !event.should_append_output)
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .unwrap_or_default();
                telescope.record_scheduled_command(IncomingEntry::make(json!({"command":event.command,"description":event.description,"expression":event.expression,"timezone":event.timezone,"user":null,"output":output.trim()}).as_object().unwrap().clone())?);
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::database::eloquent::concerns::ModelLifecycle>()
        {
            use openrust::database::eloquent::concerns::ModelEvent;
            if self.enabled("ModelWatcher")
                && super::matches(
                    &self
                        .option("ModelWatcher", "events")
                        .unwrap_or(json!(["eloquent.*"])),
                    event.name(),
                )
            {
                if event.action == ModelEvent::Retrieved {
                    if self
                        .option("ModelWatcher", "hydrations")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                        && !self.patterns("ModelWatcher", "ignore", event.model)
                    {
                        telescope.record_hydration(event.model)?;
                    }
                } else if matches!(
                    event.action,
                    ModelEvent::Created
                        | ModelEvent::Updated
                        | ModelEvent::Deleted
                        | ModelEvent::Restored
                ) {
                    let key = event
                        .key
                        .as_ref()
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_default();
                    let model = format!("{}:{key}", event.model);
                    let mut content = json!({"action":event.action.as_str(),"model":model})
                        .as_object()
                        .unwrap()
                        .clone();
                    if !event.changes.is_empty() {
                        content.insert("changes".into(), event.changes.clone().into());
                    }
                    telescope.record_model_event(IncomingEntry::make(content)?.tags([model]));
                }
            }
        } else if let Some(event) = event.downcast_ref::<openrust::redis::events::CommandExecuted>()
        {
            if self.enabled("RedisWatcher")
                && !["pipeline", "transaction"].contains(&event.command.as_str())
            {
                let mut command = event.command.clone();
                command.push(' ');
                command.push_str(
                    &event
                        .parameters
                        .iter()
                        .map(|value| String::from_utf8_lossy(value))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                telescope.record_redis(IncomingEntry::make(json!({"connection":event.connection_name,"command":command,"time":format!("{:.2}",event.time.as_secs_f64()*1000.0)}).as_object().unwrap().clone())?);
            }
        } else if let Some(event) = event.downcast_ref::<openrust::support::Dumped>() {
            if self.enabled("DumpWatcher") {
                telescope.record_dump(crate::IncomingDumpEntry::make(
                    json!({"dump":event.dump}).as_object().unwrap().clone(),
                )?);
                event.capture();
            }
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobQueueing>() {
            if !self.enabled("JobWatcher") || ignored_job(&event.job) {
                return Ok(());
            }
            let uuid = Uuid::parse_str(event.job.uuid()).map_err(|_| crate::Error::InvalidEntry)?;
            let options = event.job.options();
            let content = json!({"status":"pending","connection":event.connection_name,"queue":event.queue,"name":event.job.name(),"tries":options.max_tries,"timeout":options.timeout.map(|value|value.as_secs_f64()),"data":event.job.data()});
            telescope.record_job(
                IncomingEntry::new(content.as_object().unwrap().clone(), uuid)?
                    .with_family_hash(event.job.batch_id().map(str::to_owned)),
            );
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobProcessed>() {
            if self.enabled("JobWatcher") && !ignored_job(&event.job.payload) {
                let uuid = Uuid::parse_str(event.job.payload.uuid())
                    .map_err(|_| crate::Error::InvalidEntry)?;
                telescope.record_update(
                    EntryUpdate::make(
                        uuid,
                        EntryType::Job,
                        json!({"status":"processed","exception":null})
                            .as_object()
                            .unwrap()
                            .clone(),
                    )
                    .remove_tags(["failed"]),
                );
            }
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobFailed>() {
            if self.enabled("JobWatcher") && !ignored_job(&event.job.payload) {
                let uuid = Uuid::parse_str(event.job.payload.uuid())
                    .map_err(|_| crate::Error::InvalidEntry)?;
                telescope.record_update(EntryUpdate::make(uuid,EntryType::Job,json!({"status":"failed","exception":{"message":event.exception,"trace":[],"line":null,"line_preview":[]}}).as_object().unwrap().clone()).add_tags(["failed"]));
            }
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchDispatched>() {
            if self.enabled("BatchWatcher") {
                let uuid =
                    Uuid::parse_str(&event.batch.id).map_err(|_| crate::Error::InvalidEntry)?;
                let mut content = batch_content(&event.batch)?;
                content.insert(
                    "queue".into(),
                    event
                        .batch
                        .options
                        .get("queue")
                        .cloned()
                        .unwrap_or("default".into()),
                );
                content.insert(
                    "connection".into(),
                    event
                        .batch
                        .options
                        .get("connection")
                        .cloned()
                        .unwrap_or("default".into()),
                );
                content.insert(
                    "allowsFailures".into(),
                    event.batch.allows_failures().into(),
                );
                telescope.record_batch(
                    IncomingEntry::new(content, uuid)?
                        .with_family_hash(Some(event.batch.id.clone())),
                );
            }
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchStarted>() {
            self.update_batch(&event.batch, telescope)?;
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchFinished>() {
            self.update_batch(&event.batch, telescope)?;
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchCanceled>() {
            self.update_batch(&event.batch, telescope)?;
        } else if let Some(event) =
            event.downcast_ref::<openrust::auth::access::events::GateEvaluated>()
            && self.enabled("GateWatcher")
            && !self.patterns("GateWatcher", "ignore_abilities", &event.ability)
        {
            let arguments = context
                .and_then(|context| event.arguments(context))
                .map(|arguments| {
                    arguments
                        .iter()
                        .map(|value| format_argument(*value))
                        .collect::<Vec<_>>()
                });
            let allowed = event.result.as_ref().is_some_and(|result| result.allowed());
            let message = event.result.as_ref().map(|result| result.message());
            let caller = context
                .and_then(|context| event.callers(context))
                .filter(|callers| !callers.is_empty())
                .unwrap_or(event.caller.as_slice())
                .iter()
                .rev()
                .copied()
                .find(|caller| self.caller_visible("GateWatcher", caller));
            telescope.record_gate(IncomingEntry::make(json!({"ability":event.ability,"result":if allowed {"allowed"} else {"denied"},"message":message,"arguments":arguments,"file":caller.map(Location::file),"line":caller.map(Location::line)}).as_object().unwrap().clone())?);
        }
        Ok(())
    }

    pub(super) fn framework_caller(caller: &Location<'_>) -> bool {
        let path = caller.file().replace('\\', "/");
        let rooted = format!("/{}", path.trim_start_matches('/'));
        rooted
            .split_once("/vendor/openrust/")
            .is_some_and(|(_, package)| package.contains("/src/"))
            || (cargo_package(&rooted)
                && rooted.split('/').any(|part| part.starts_with("openrust-")))
    }
    pub(super) fn caller_visible(&self, watcher: &str, caller: &Location<'_>) -> bool {
        if Self::framework_caller(caller) {
            return false;
        }
        let path = caller.file().replace('\\', "/");
        let rooted = format!("/{}", path.trim_start_matches('/'));
        let ignore_packages = self
            .option(watcher, "ignore_packages")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        if ignore_packages && (cargo_package(&rooted) || rooted.contains("/vendor/")) {
            return false;
        }
        !self.option(watcher, "ignore_paths").is_some_and(|paths| {
            paths.as_array().is_some_and(|paths| {
                paths.iter().filter_map(Value::as_str).any(|ignored| {
                    !ignored.is_empty() && path.contains(&ignored.replace('\\', "/"))
                })
            })
        })
    }
    fn update_batch(
        &self,
        batch: &openrust::bus::Batch,
        telescope: &Telescope,
    ) -> crate::Result<()> {
        if self.enabled("BatchWatcher") {
            telescope.record_update(EntryUpdate::make(
                Uuid::parse_str(&batch.id).map_err(|_| crate::Error::InvalidEntry)?,
                EntryType::Batch,
                batch_content(batch)?,
            ));
        }
        Ok(())
    }
}
fn cargo_package(path: &str) -> bool {
    path.contains("/.cargo/registry/") || path.contains("/.cargo/git/")
}

fn ignored_job(payload: &openrust::queue::Payload) -> bool {
    payload.name() == crate::jobs::ProcessPendingUpdates::NAME
}
fn batch_content(batch: &openrust::bus::Batch) -> crate::Result<Map<String, Value>> {
    serde_json::to_value(batch)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or(crate::Error::InvalidEntry)
}
pub(super) fn format_argument(value: &openrust::auth::access::Argument) -> Value {
    if let Some(value) = value.downcast_ref::<Value>() {
        return value.clone();
    }
    if let Some(value) = value.downcast_ref::<String>() {
        return value.clone().into();
    }
    if let Some(value) = value.downcast_ref::<&'static str>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<bool>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<i64>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<u64>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<i32>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<u32>() {
        return (*value).into();
    }
    if let Some(value) = value.downcast_ref::<f64>() {
        return (*value).into();
    }
    Value::Null
}
