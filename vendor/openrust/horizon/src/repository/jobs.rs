use super::{JobRecord, Repository, encode, processes::string_list, text};
use crate::{Error, Result};
use openrust::{queue::Payload, redis::Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobDetails {
    #[serde(flatten)]
    pub record: JobRecord,
    pub payload: Json,
    pub tags: Vec<String>,
    pub exception: Option<String>,
    pub silenced: bool,
    pub created_at: u64,
    pub reserved_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub failed_at: Option<u64>,
    pub retried_by: Vec<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}
impl JobDetails {
    /// Native failed-job storage uses the payload UUID, separately from transport IDs.
    pub fn payloadUuid(&self) -> Result<&str> {
        self.payload
            .get("uuid")
            .and_then(Json::as_str)
            .filter(|id| crate::label(id))
            .ok_or(Error::InvalidRecord)
    }
}
#[derive(Clone)]
pub struct JobRepository(pub(super) Repository);
impl Repository {
    pub fn jobs(&self) -> JobRepository {
        JobRepository(self.clone())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Preserves the queue event payload, lifecycle and monitoring inputs"
    )]
    pub async fn recordPayload(
        &self,
        record: JobRecord,
        phase: &str,
        payload: &Payload,
        tags: &[String],
        silenced: bool,
        exception: Option<&str>,
    ) -> Result<()> {
        let Some(status) = self
            .recordAccepted(record.clone(), phase, exception.is_some())
            .await?
        else {
            return Ok(());
        };
        self.jobs()
            .store(record, payload, tags, silenced, exception, &status)
            .await
    }
}
impl JobRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    fn key(&self, id: &str) -> Result<String> {
        if !crate::label(id) {
            return Err(Error::InvalidRecord);
        }
        Ok(self.0.key(&format!("details:{}", encode(id))))
    }
    fn index(&self, kind: &str) -> String {
        self.0.key(&format!("jobs:{kind}"))
    }
    pub fn nextJobId(&self) -> String {
        uuid::Uuid::now_v7().to_string()
    }
    pub async fn find(&self, id: &str) -> Result<Option<JobDetails>> {
        let value = self.0.command("GET", &[self.key(id)?]).await?;
        if value == Value::Nil {
            Ok(None)
        } else {
            let mut job: JobDetails =
                serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)?;
            let retries = string_list(
                self.0
                    .command(
                        "LRANGE",
                        &[
                            format!("{}:retries", self.key(id)?),
                            "0".into(),
                            "-1".into(),
                        ],
                    )
                    .await?,
            )?;
            job.retried_by = retries
                .iter()
                .map(|value| serde_json::from_str(value).map_err(|_| Error::Redis))
                .collect::<Result<_>>()?;
            Ok(Some(job))
        }
    }
    pub async fn getJobs(&self, ids: &[String], indexFrom: usize) -> Result<Vec<JobDetails>> {
        let mut jobs = Vec::new();
        for id in ids {
            if let Some(mut job) = self.find(id).await? {
                job.index = Some(indexFrom + jobs.len());
                jobs.push(job)
            }
        }
        Ok(jobs)
    }
    async fn count(&self, kind: &str) -> Result<u64> {
        self.trim(kind).await?;
        match self.0.command("ZCARD", &[self.index(kind)]).await? {
            Value::Int(v) => u64::try_from(v).map_err(|_| Error::Redis),
            _ => Err(Error::Redis),
        }
    }
    async fn list(&self, kind: &str, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.trim(kind).await?;
        let start = after.map_or(0, |v| v.saturating_add(1));
        let ids = string_list(
            self.0
                .command(
                    "ZREVRANGE",
                    &[
                        self.index(kind),
                        start.to_string(),
                        start.saturating_add(49).to_string(),
                    ],
                )
                .await?,
        )?;
        self.getJobs(&ids, start).await
    }
    pub async fn totalRecent(&self) -> Result<u64> {
        self.countRecent().await
    }
    pub async fn totalFailed(&self) -> Result<u64> {
        self.countFailed().await
    }
    pub async fn countRecent(&self) -> Result<u64> {
        self.count("recent").await
    }
    pub async fn countFailed(&self) -> Result<u64> {
        self.count("failed").await
    }
    pub async fn countPending(&self) -> Result<u64> {
        self.count("pending").await
    }
    pub async fn countCompleted(&self) -> Result<u64> {
        self.count("completed").await
    }
    pub async fn countSilenced(&self) -> Result<u64> {
        self.count("silenced").await
    }
    pub async fn countRecentlyFailed(&self) -> Result<u64> {
        self.count("recent_failed").await
    }
    pub async fn getRecent(&self, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.list("recent", after).await
    }
    pub async fn getFailed(&self, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.list("failed", after).await
    }
    pub async fn getPending(&self, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.list("pending", after).await
    }
    pub async fn getCompleted(&self, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.list("completed", after).await
    }
    pub async fn getSilenced(&self, after: Option<usize>) -> Result<Vec<JobDetails>> {
        self.list("silenced", after).await
    }
    async fn trim(&self, kind: &str) -> Result<()> {
        let cutoff = self
            .0
            .now()
            .await?
            .saturating_sub(u64::from(self.0.retentionFor(kind)) * 1000);
        self.0
            .command(
                "ZREMRANGEBYSCORE",
                &[self.index(kind), "-inf".into(), cutoff.to_string()],
            )
            .await?;
        Ok(())
    }
    pub async fn trimRecentJobs(&self) -> Result<()> {
        for kind in [
            "recent",
            "pending",
            "completed",
            "silenced",
            "recent_failed",
        ] {
            self.trim(kind).await?;
        }
        Ok(())
    }
    pub async fn trimFailedJobs(&self) -> Result<()> {
        self.trim("failed").await
    }
    pub async fn trimMonitoredJobs(&self) -> Result<()> {
        self.trim("monitored").await
    }
    pub async fn pushed(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
    ) -> Result<()> {
        let record = JobRecord {
            id: id.into(),
            name: payload.name().into(),
            connection: connection.into(),
            queue: queue.into(),
            attempts: 0,
            status: "pending".into(),
            duration: 0.,
            updated_at: 0,
        };
        self.store(record, payload, tags, false, None, "pending")
            .await
    }
    pub async fn reserved(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
    ) -> Result<()> {
        let record = JobRecord {
            id: id.into(),
            name: payload.name().into(),
            connection: connection.into(),
            queue: queue.into(),
            attempts: 0,
            status: "reserved".into(),
            duration: 0.,
            updated_at: 0,
        };
        self.store(record, payload, tags, false, None, "reserved")
            .await
    }
    pub async fn released(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
    ) -> Result<()> {
        self.pending(id, connection, queue, payload, tags, "released")
            .await
    }
    async fn pending(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
        phase: &str,
    ) -> Result<()> {
        let record = self
            .find(id)
            .await?
            .map(|job| job.record)
            .unwrap_or(JobRecord {
                id: id.into(),
                name: payload.name().into(),
                connection: connection.into(),
                queue: queue.into(),
                attempts: 0,
                status: String::new(),
                duration: 0.,
                updated_at: 0,
            });
        self.store(record, payload, tags, false, None, phase).await
    }
    pub async fn completed(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
        silenced: bool,
    ) -> Result<()> {
        let record = self
            .find(id)
            .await?
            .map(|job| job.record)
            .unwrap_or(JobRecord {
                id: id.into(),
                name: payload.name().into(),
                connection: connection.into(),
                queue: queue.into(),
                attempts: 0,
                status: String::new(),
                duration: 0.,
                updated_at: 0,
            });
        self.store(record, payload, tags, silenced, None, "completed")
            .await
    }
    pub async fn failed(
        &self,
        exception: &str,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
    ) -> Result<()> {
        let record = self
            .find(id)
            .await?
            .map(|job| job.record)
            .unwrap_or(JobRecord {
                id: id.into(),
                name: payload.name().into(),
                connection: connection.into(),
                queue: queue.into(),
                attempts: 0,
                status: String::new(),
                duration: 0.,
                updated_at: 0,
            });
        self.store(record, payload, tags, false, Some(exception), "failed")
            .await
    }
    pub async fn migrated(
        &self,
        connection: &str,
        queue: &str,
        payloads: &[(String, Payload)],
    ) -> Result<()> {
        for (id, payload) in payloads {
            self.pending(id, connection, queue, payload, &[], "pending")
                .await?;
        }
        Ok(())
    }
    pub async fn remember(
        &self,
        id: &str,
        connection: &str,
        queue: &str,
        payload: &Payload,
        tags: &[String],
    ) -> Result<()> {
        self.pending(id, connection, queue, payload, tags, "pending")
            .await?;
        let now = self.0.now().await?;
        self.0
            .command(
                "ZADD",
                &[self.index("monitored"), now.to_string(), id.into()],
            )
            .await?;
        self.0
            .command(
                "EXPIRE",
                &[self.key(id)?, self.0.retentionFor("monitored").to_string()],
            )
            .await?;
        Ok(())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Stores the complete queue event projection"
    )]
    pub(super) async fn store(
        &self,
        mut record: JobRecord,
        payload: &Payload,
        tags: &[String],
        silenced: bool,
        exception: Option<&str>,
        phase: &str,
    ) -> Result<()> {
        if !crate::label(&record.id) || record.connection.is_empty() || record.queue.is_empty() {
            return Err(Error::InvalidRecord);
        }
        let now = self.0.now().await?;
        let monitored = !self.0.tags().monitored(tags).await?.is_empty();
        record.updated_at = now;
        let old = self.find(&record.id).await?;
        let status = if phase == "finalize" {
            "completed"
        } else if phase == "processing" {
            "reserved"
        } else if phase == "released" || phase == "retrying" {
            "pending"
        } else {
            phase
        };
        // A failure hook precedes JobAttempted; finalization must preserve that terminal state.
        let status = if old
            .as_ref()
            .is_some_and(|j| j.record.status == "failed" && j.record.attempts >= record.attempts)
            && phase == "retrying"
        {
            "failed"
        } else {
            status
        };
        record.status = status.into();
        let details = JobDetails {
            record: record.clone(),
            payload: serde_json::to_value(payload).map_err(|_| Error::InvalidRecord)?,
            tags: tags.to_vec(),
            exception: if status == "reserved" {
                None
            } else {
                exception
                    .map(str::to_owned)
                    .or_else(|| old.as_ref().and_then(|j| j.exception.clone()))
            },
            silenced,
            created_at: old.as_ref().map_or(now, |j| j.created_at),
            reserved_at: if status == "reserved" {
                Some(now)
            } else {
                old.as_ref().and_then(|j| j.reserved_at)
            },
            completed_at: if status == "completed" {
                Some(now)
            } else {
                None
            },
            failed_at: if status == "failed" {
                Some(now)
            } else {
                old.as_ref().and_then(|j| j.failed_at)
            },
            retried_by: old.map_or_else(Vec::new, |j| j.retried_by),
            index: None,
        };
        let index = if status == "completed" {
            if silenced { "silenced" } else { "completed" }
        } else if status == "failed" {
            "failed"
        } else {
            "pending"
        };
        self.0.eval("redis.call('SET',KEYS[1],ARGV[1],'EX',ARGV[4]);redis.call('ZADD',KEYS[2],ARGV[2],ARGV[3]);for i=4,7 do redis.call('ZREM',KEYS[i],ARGV[3]) end;redis.call('ZADD',KEYS[3],ARGV[2],ARGV[3]);if ARGV[5]=='failed' then redis.call('ZADD',KEYS[8],ARGV[2],ARGV[3]) end;return 1",vec![self.key(&record.id)?,self.index("recent"),self.index(index),self.index("pending"),self.index("completed"),self.index("silenced"),self.index("failed"),self.index("recent_failed")],vec![serde_json::to_string(&details).map_err(|_|Error::InvalidRecord)?,now.to_string(),record.id.clone(),self.0.retentionFor(if status=="failed"{"failed"}else if status=="completed"{"completed"}else{"pending"}).max(if monitored{self.0.retentionFor("monitored")}else{0}).to_string(),status.into()]).await?;
        // Retry relationships live separately from JSON details so empty arrays
        // keep their representation when Redis updates one retry atomically.
        self.0
            .command(
                "EXPIRE",
                &[
                    format!("{}:retries", self.key(&record.id)?),
                    self.0
                        .retentionFor(if status == "failed" {
                            "failed"
                        } else if status == "completed" {
                            "completed"
                        } else {
                            "pending"
                        })
                        .max(if monitored {
                            self.0.retentionFor("monitored")
                        } else {
                            0
                        })
                        .to_string(),
                ],
            )
            .await?;
        if matches!(status, "completed" | "failed") {
            self.0
                .eval(
                    r#"
for _,parent in ipairs(redis.call('SMEMBERS',KEYS[1])) do
  for index,value in ipairs(redis.call('LRANGE',parent,0,-1)) do
    local retry=cjson.decode(value)
    if retry.id==ARGV[1] then
      retry.status=ARGV[2]
      redis.call('LSET',parent,index-1,cjson.encode(retry))
    end
  end
end
return 1
"#,
                    vec![format!("{}:parents", self.key(&record.id)?)],
                    vec![record.id.clone(), status.into()],
                )
                .await?;
        }
        let monitoredTags = self.0.tags().monitored(tags).await?;
        self.0.tags().add(&record.id, &monitoredTags).await?;
        if status == "failed" {
            self.0
                .command(
                    "SET",
                    &[
                        self.0
                            .key(&format!("failed-uuid:{}", encode(payload.uuid()))),
                        record.id.clone(),
                        "EX".into(),
                        self.0.retentionFor("failed").to_string(),
                    ],
                )
                .await?;
            let failed: Vec<_> = tags.iter().map(|tag| format!("failed:{tag}")).collect();
            self.0
                .tags()
                .addTemporary(
                    self.0.retentionFor("failed").div_ceil(60),
                    &record.id,
                    &failed,
                )
                .await?;
        }
        if monitored {
            self.0
                .command(
                    "ZADD",
                    &[self.index("monitored"), now.to_string(), record.id],
                )
                .await?;
        }
        Ok(())
    }
    pub async fn findFailedByUuid(&self, uuid: &str) -> Result<Option<JobDetails>> {
        if !crate::label(uuid) {
            return Err(Error::InvalidRecord);
        }
        let value = self
            .0
            .command(
                "GET",
                &[self.0.key(&format!("failed-uuid:{}", encode(uuid)))],
            )
            .await?;
        if value == Value::Nil {
            return Ok(None);
        }
        self.findFailed(&text(&value)?).await
    }
    pub async fn findFailed(&self, id: &str) -> Result<Option<JobDetails>> {
        Ok(self
            .find(id)
            .await?
            .filter(|job| job.record.status == "failed"))
    }
    pub async fn storeRetryReference(&self, id: &str, retryId: &str) -> Result<()> {
        if !crate::label(retryId) {
            return Err(Error::InvalidRecord);
        }
        let now = self.0.now().await?;
        self.0
            .eval(
                r#"
if redis.call('EXISTS',KEYS[1])==0 then return 0 end
local retry=cjson.decode(ARGV[1])
local child=redis.call('GET',KEYS[3])
if child then
  local status=cjson.decode(child).status
  if status=='completed' or status=='failed' then retry.status=status end
end
redis.call('RPUSH',KEYS[2],cjson.encode(retry))
redis.call('SADD',KEYS[4],KEYS[2])
local ttl=redis.call('TTL',KEYS[1])
if ttl>0 then
  redis.call('EXPIRE',KEYS[2],ttl)
  if redis.call('TTL',KEYS[4])<ttl then redis.call('EXPIRE',KEYS[4],ttl) end
end
return 1
"#,
                vec![
                    self.key(id)?,
                    format!("{}:retries", self.key(id)?),
                    self.key(retryId)?,
                    format!("{}:parents", self.key(retryId)?),
                ],
                vec![json!({"id":retryId,"status":"pending","retried_at":now}).to_string()],
            )
            .await?;
        Ok(())
    }
    pub async fn deleteFailed(&self, id: &str) -> Result<bool> {
        let Some(job) = self.findFailed(id).await? else {
            return Ok(false);
        };
        self.delete(&job.record).await?;
        self.0
            .eval(
                "if redis.call('GET',KEYS[1])==ARGV[1] then redis.call('DEL',KEYS[1]) end;return 1",
                vec![
                    self.0
                        .key(&format!("failed-uuid:{}", encode(job.payloadUuid()?))),
                ],
                vec![id.into()],
            )
            .await?;
        let mut tags = job.tags.clone();
        tags.extend(job.tags.iter().map(|tag| format!("failed:{tag}")));
        self.0.tags().forgetJobs(&tags, &[id.into()]).await?;
        Ok(true)
    }
    async fn delete(&self, record: &JobRecord) -> Result<()> {
        let id = &record.id;
        let legacy = encode(
            &serde_json::to_string(&(&record.connection, &record.queue, id))
                .map_err(|_| Error::InvalidRecord)?,
        );
        let mut keys = vec![self.key(id)?];
        keys.extend(
            [
                "recent",
                "pending",
                "completed",
                "silenced",
                "failed",
                "recent_failed",
                "monitored",
            ]
            .map(|kind| self.index(kind)),
        );
        keys.extend([self.0.key(&format!("job:{legacy}")), self.0.key("recent")]);
        self.0.eval("redis.call('DEL',KEYS[1],KEYS[1]..':retries',KEYS[1]..':parents',KEYS[9]);for i=2,8 do redis.call('ZREM',KEYS[i],ARGV[1]) end;redis.call('ZREM',KEYS[10],ARGV[2]);return 1",keys,vec![id.clone(),legacy]).await?;
        Ok(())
    }
    pub async fn deleteMonitored(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut args = vec![self.index("monitored")];
        args.extend_from_slice(ids);
        self.0.command("ZREM", &args).await?;
        Ok(())
    }
    pub async fn purge(&self, queue: &str) -> Result<u64> {
        let ids = string_list(
            self.0
                .command("ZRANGE", &[self.index("pending"), "0".into(), "-1".into()])
                .await?,
        )?;
        let mut count = 0;
        for id in ids {
            if let Some(job) = self.find(&id).await?
                && job.record.queue == queue
            {
                self.delete(&job.record).await?;
                count += 1;
            }
        }
        Ok(count)
    }
}
