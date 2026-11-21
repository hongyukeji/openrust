use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    Batch,
    Cache,
    ClientRequest,
    Command,
    Dump,
    Event,
    Exception,
    Gate,
    Job,
    Log,
    Mail,
    Model,
    Notification,
    Query,
    Redis,
    Request,
    #[serde(rename = "schedule")]
    ScheduledTask,
    View,
}
impl EntryType {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Batch,
            Self::Cache,
            Self::ClientRequest,
            Self::Command,
            Self::Dump,
            Self::Event,
            Self::Exception,
            Self::Gate,
            Self::Job,
            Self::Log,
            Self::Mail,
            Self::Model,
            Self::Notification,
            Self::Query,
            Self::Redis,
            Self::Request,
            Self::ScheduledTask,
            Self::View,
        ]
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Cache => "cache",
            Self::ClientRequest => "client_request",
            Self::Command => "command",
            Self::Dump => "dump",
            Self::Event => "event",
            Self::Exception => "exception",
            Self::Gate => "gate",
            Self::Job => "job",
            Self::Log => "log",
            Self::Mail => "mail",
            Self::Model => "model",
            Self::Notification => "notification",
            Self::Query => "query",
            Self::Redis => "redis",
            Self::Request => "request",
            Self::ScheduledTask => "schedule",
            Self::View => "view",
        }
    }
}
impl std::str::FromStr for EntryType {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::all()
            .iter()
            .copied()
            .find(|kind| kind.as_str() == value)
            .ok_or(Error::InvalidEntry)
    }
}

#[derive(Clone)]
pub struct IncomingEntry {
    pub uuid: Uuid,
    pub batchId: Option<Uuid>,
    pub r#type: Option<EntryType>,
    pub familyHash: Option<String>,
    pub content: Map<String, Value>,
    pub tags: Vec<String>,
    pub recordedAt: jiff::Timestamp,
    pub(crate) exceptionReportable: Option<bool>,
    pub(crate) dump: bool,
    pub(crate) monitored: bool,
}
impl IncomingEntry {
    pub fn make(content: Map<String, Value>) -> Result<Self> {
        Self::new(content, Uuid::now_v7())
    }
    pub fn new(mut content: Map<String, Value>, uuid: Uuid) -> Result<Self> {
        let host = hostname::get().map_err(Error::Hostname)?;
        content.insert(
            "hostname".into(),
            host.to_string_lossy().into_owned().into(),
        );
        Ok(Self {
            uuid,
            batchId: None,
            r#type: None,
            familyHash: None,
            content,
            tags: Vec::new(),
            recordedAt: jiff::Timestamp::now(),
            exceptionReportable: None,
            dump: false,
            monitored: false,
        })
    }
    pub fn batchId(mut self, id: Uuid) -> Self {
        self.batchId = Some(id);
        self
    }
    pub fn r#type(mut self, kind: EntryType) -> Self {
        self.r#type = Some(kind);
        self
    }
    pub fn withFamilyHash(mut self, hash: Option<String>) -> Self {
        self.familyHash = hash;
        self
    }
    pub fn tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for tag in tags {
            let tag = tag.into();
            if !self.tags.contains(&tag) {
                self.tags.push(tag);
            }
        }
        self
    }
    pub fn user(mut self, id: Value, name: Value, email: Value) -> Self {
        let tag = match &id {
            Value::String(id) => id.clone(),
            _ => id.to_string(),
        };
        self.content.insert(
            "user".into(),
            serde_json::json!({"id":id,"name":name,"email":email}),
        );
        self.tags([format!("Auth:{tag}")])
    }
    pub fn hasMonitoredTag(&self) -> bool {
        self.monitored
    }
    pub fn isRequest(&self) -> bool {
        self.r#type == Some(EntryType::Request)
    }
    pub fn isFailedRequest(&self) -> bool {
        self.isRequest()
            && self
                .content
                .get("response_status")
                .and_then(Value::as_i64)
                .unwrap_or(200)
                >= 500
    }
    pub fn isQuery(&self) -> bool {
        self.r#type == Some(EntryType::Query)
    }
    pub fn isSlowQuery(&self) -> bool {
        self.isQuery()
            && self
                .content
                .get("slow")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }
    pub fn isEvent(&self) -> bool {
        self.r#type == Some(EntryType::Event)
    }
    pub fn isCache(&self) -> bool {
        self.r#type == Some(EntryType::Cache)
    }
    pub fn isGate(&self) -> bool {
        self.r#type == Some(EntryType::Gate)
    }
    pub fn isFailedJob(&self) -> bool {
        self.r#type == Some(EntryType::Job)
            && self.content.get("status").and_then(Value::as_str) == Some("failed")
    }
    pub fn isException(&self) -> bool {
        self.exceptionReportable.is_some()
    }
    pub fn isReportableException(&self) -> bool {
        self.exceptionReportable == Some(true)
    }
    pub fn isDump(&self) -> bool {
        self.dump
    }
    pub fn isLog(&self) -> bool {
        self.r#type == Some(EntryType::Log)
    }
    pub fn isScheduledTask(&self) -> bool {
        self.r#type == Some(EntryType::ScheduledTask)
    }
    pub fn isClientRequest(&self) -> bool {
        self.r#type == Some(EntryType::ClientRequest)
    }
}

#[derive(Clone)]
pub struct IncomingExceptionEntry(pub IncomingEntry);
impl IncomingExceptionEntry {
    /// Converts a native reported error snapshot. Rust errors without source metadata
    /// retain null file/line and a distinct family; no origin or trace is fabricated.
    pub fn fromReport(mut content: Map<String, Value>) -> Result<Self> {
        if content.get("file").and_then(Value::as_str).is_some()
            && content.get("line").and_then(Value::as_u64).is_some()
        {
            return Self::make(content, true);
        }
        content.insert("file".into(), Value::Null);
        content.insert("line".into(), Value::Null);
        let mut entry = IncomingEntry::make(content)?.r#type(EntryType::Exception);
        entry.familyHash = Some(format!(
            "{:x}",
            md5::compute(format!("unlocated:{}", entry.uuid))
        ));
        entry.exceptionReportable = Some(true);
        Ok(Self(entry))
    }
    /// `reportable` is the native exception handler's `shouldReport` decision.
    pub fn make(content: Map<String, Value>, reportable: bool) -> Result<Self> {
        let file = content
            .get("file")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidEntry)?;
        let line = content
            .get("line")
            .and_then(Value::as_u64)
            .ok_or(Error::InvalidEntry)?;
        let hash = format!("{:x}", md5::compute(format!("{file}{line}")));
        let mut entry = IncomingEntry::make(content)?
            .r#type(EntryType::Exception)
            .withFamilyHash(Some(hash));
        entry.exceptionReportable = Some(reportable);
        Ok(Self(entry))
    }
}
impl std::ops::Deref for IncomingExceptionEntry {
    type Target = IncomingEntry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<IncomingExceptionEntry> for IncomingEntry {
    fn from(entry: IncomingExceptionEntry) -> Self {
        entry.0
    }
}

#[derive(Clone)]
pub struct IncomingDumpEntry(pub IncomingEntry);
impl IncomingDumpEntry {
    pub fn make(content: Map<String, Value>) -> Result<Self> {
        let mut entry = IncomingEntry::make(content)?.r#type(EntryType::Dump);
        entry.dump = true;
        Ok(Self(entry))
    }
    pub fn assignEntryPointFromBatch(&mut self, batch: &[IncomingEntry]) {
        let Some(entry) = batch.iter().find(|entry| {
            matches!(
                entry.r#type,
                Some(EntryType::Request | EntryType::Job | EntryType::Command)
            )
        }) else {
            return;
        };
        let Some(kind) = entry.r#type else {
            return;
        };
        let description = match kind {
            EntryType::Request => format!(
                "{} {}",
                entry
                    .content
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                entry
                    .content
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ),
            EntryType::Job => entry
                .content
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            EntryType::Command => entry
                .content
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            _ => String::new(),
        };
        self.0.content.extend([
            ("entry_point_type".into(), kind.as_str().into()),
            ("entry_point_uuid".into(), entry.uuid.to_string().into()),
            ("entry_point_description".into(), description.into()),
        ]);
    }
}
impl std::ops::Deref for IncomingDumpEntry {
    type Target = IncomingEntry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<IncomingDumpEntry> for IncomingEntry {
    fn from(entry: IncomingDumpEntry) -> Self {
        entry.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryUpdate {
    pub uuid: Uuid,
    pub r#type: EntryType,
    pub changes: Map<String, Value>,
    pub addedTags: Vec<String>,
    pub removedTags: Vec<String>,
}
impl EntryUpdate {
    pub fn make(uuid: Uuid, kind: EntryType, changes: Map<String, Value>) -> Self {
        Self {
            uuid,
            r#type: kind,
            changes,
            addedTags: Vec::new(),
            removedTags: Vec::new(),
        }
    }
    pub fn change(mut self, changes: Map<String, Value>) -> Self {
        self.changes.extend(changes);
        self
    }
    pub fn addTags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for tag in tags {
            let tag = tag.into();
            if !self.addedTags.contains(&tag) {
                self.addedTags.push(tag);
            }
        }
        self
    }
    pub fn removeTags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for tag in tags {
            let tag = tag.into();
            if !self.removedTags.contains(&tag) {
                self.removedTags.push(tag);
            }
        }
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryResult {
    pub id: Uuid,
    pub sequence: Option<i64>,
    #[serde(rename = "batch_id")]
    pub batchId: Uuid,
    pub r#type: EntryType,
    #[serde(rename = "family_hash")]
    pub familyHash: Option<String>,
    pub content: Value,
    #[serde(rename = "created_at")]
    pub createdAt: String,
    pub tags: Vec<String>,
}
