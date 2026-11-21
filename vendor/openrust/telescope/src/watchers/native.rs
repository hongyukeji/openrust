use super::{NativeWatcher, json};
use crate::{EntryType, EntryUpdate, IncomingEntry, Telescope};
use openrust::{contracts::events::Event, events::EventContext};
use serde_json::{Map, Value};
use uuid::Uuid;

impl NativeWatcher {
    pub(super) fn recordAdditional(
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
                telescope.recordMail(IncomingEntry::make(json!({
                    "mailable":event.mailable,"queued":event.queued,
                    "from":envelope.from.as_ref().map(|value|addresses(std::slice::from_ref(value))),
                    "replyTo":addresses(&envelope.reply_to),"to":addresses(&envelope.to),
                    "cc":addresses(&envelope.cc),"bcc":addresses(&envelope.bcc),
                    "subject":envelope.subject,
                    "html":message.getHtmlBody().or_else(||message.getTextBody()),
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
                telescope.recordNotification(IncomingEntry::make(json!({"notification":event.kind,"queued":event.queued,"notifiable":event.notifiable,"channel":event.channel,"response":response}).as_object().unwrap().clone())?.tags([event.notifiable.clone()]));
            }
        } else if let Some(event) =
            event.downcast_ref::<openrust::console::events::ScheduledTaskCompleted>()
        {
            if self.enabled("ScheduleWatcher") {
                let output = event
                    .output
                    .as_ref()
                    .filter(|_| !event.shouldAppendOutput)
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .unwrap_or_default();
                telescope.recordScheduledCommand(IncomingEntry::make(json!({"command":event.command,"description":event.description,"expression":event.expression,"timezone":event.timezone,"user":null,"output":output.trim()}).as_object().unwrap().clone())?);
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
                        telescope.recordHydration(event.model)?;
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
                    telescope.recordModelEvent(IncomingEntry::make(content)?.tags([model]));
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
                telescope.recordRedis(IncomingEntry::make(json!({"connection":event.connectionName,"command":command,"time":format!("{:.2}",event.time.as_secs_f64()*1000.0)}).as_object().unwrap().clone())?);
            }
        } else if let Some(event) = event.downcast_ref::<openrust::support::Dumped>() {
            if self.enabled("DumpWatcher") {
                telescope.recordDump(crate::IncomingDumpEntry::make(
                    json!({"dump":event.dump}).as_object().unwrap().clone(),
                )?);
                event.capture();
            }
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobQueueing>() {
            if !self.enabled("JobWatcher") || ignoredJob(&event.job) {
                return Ok(());
            }
            let uuid = Uuid::parse_str(event.job.uuid()).map_err(|_| crate::Error::InvalidEntry)?;
            let options = event.job.options();
            let content = json!({"status":"pending","connection":event.connection_name,"queue":event.queue,"name":event.job.name(),"tries":options.max_tries,"timeout":options.timeout.map(|value|value.as_secs_f64()),"data":event.job.data()});
            telescope.recordJob(
                IncomingEntry::new(content.as_object().unwrap().clone(), uuid)?
                    .withFamilyHash(event.job.batchId().map(str::to_owned)),
            );
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobProcessed>() {
            if self.enabled("JobWatcher") && !ignoredJob(&event.job.payload) {
                let uuid = Uuid::parse_str(event.job.payload.uuid())
                    .map_err(|_| crate::Error::InvalidEntry)?;
                telescope.recordUpdate(
                    EntryUpdate::make(
                        uuid,
                        EntryType::Job,
                        json!({"status":"processed","exception":null})
                            .as_object()
                            .unwrap()
                            .clone(),
                    )
                    .removeTags(["failed"]),
                );
            }
        } else if let Some(event) = event.downcast_ref::<openrust::queue::events::JobFailed>() {
            if self.enabled("JobWatcher") && !ignoredJob(&event.job.payload) {
                let uuid = Uuid::parse_str(event.job.payload.uuid())
                    .map_err(|_| crate::Error::InvalidEntry)?;
                telescope.recordUpdate(EntryUpdate::make(uuid,EntryType::Job,json!({"status":"failed","exception":{"message":event.exception,"trace":[],"line":null,"line_preview":[]}}).as_object().unwrap().clone()).addTags(["failed"]));
            }
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchDispatched>() {
            if self.enabled("BatchWatcher") {
                let uuid =
                    Uuid::parse_str(&event.batch.id).map_err(|_| crate::Error::InvalidEntry)?;
                let mut content = batchContent(&event.batch)?;
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
                content.insert("allowsFailures".into(), event.batch.allowsFailures().into());
                telescope.recordBatch(
                    IncomingEntry::new(content, uuid)?.withFamilyHash(Some(event.batch.id.clone())),
                );
            }
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchStarted>() {
            self.updateBatch(&event.batch, telescope)?;
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchFinished>() {
            self.updateBatch(&event.batch, telescope)?;
        } else if let Some(event) = event.downcast_ref::<openrust::bus::events::BatchCanceled>() {
            self.updateBatch(&event.batch, telescope)?;
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
                        .map(|value| formatArgument(*value))
                        .collect::<Vec<_>>()
                });
            let allowed = event.result.as_ref().is_some_and(|result| result.allowed());
            let message = event.result.as_ref().map(|result| result.message());
            telescope.recordGate(IncomingEntry::make(json!({"ability":event.ability,"result":if allowed {"allowed"} else {"denied"},"message":message,"arguments":arguments,"file":null,"line":null}).as_object().unwrap().clone())?);
        }
        Ok(())
    }
    fn updateBatch(
        &self,
        batch: &openrust::bus::Batch,
        telescope: &Telescope,
    ) -> crate::Result<()> {
        if self.enabled("BatchWatcher") {
            telescope.recordUpdate(EntryUpdate::make(
                Uuid::parse_str(&batch.id).map_err(|_| crate::Error::InvalidEntry)?,
                EntryType::Batch,
                batchContent(batch)?,
            ));
        }
        Ok(())
    }
}
fn ignoredJob(payload: &openrust::queue::Payload) -> bool {
    payload.name() == crate::jobs::ProcessPendingUpdates::NAME
}
fn batchContent(batch: &openrust::bus::Batch) -> crate::Result<Map<String, Value>> {
    serde_json::to_value(batch)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or(crate::Error::InvalidEntry)
}
pub(super) fn formatArgument(value: &openrust::auth::access::Argument) -> Value {
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
