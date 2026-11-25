use super::{Repository, text};
use crate::{
    Error, Result,
    master_names::{supervisor_parent, valid_process_name},
};
use openrust::redis::Value;
use serde_json::Value as Json;
use std::collections::BTreeMap;

#[path = "master_lease.rs"]
mod master_lease;
pub(crate) use master_lease::{MasterLease, MasterOwnership};

#[derive(Clone)]
pub struct MasterSupervisorRepository(Repository);
#[derive(Clone)]
pub struct SupervisorRepository(Repository);
#[derive(Clone)]
pub struct ProcessRepository(Repository);

impl Repository {
    pub fn master_supervisors(&self) -> MasterSupervisorRepository {
        MasterSupervisorRepository(self.clone())
    }
    pub fn supervisor_repository(&self) -> SupervisorRepository {
        SupervisorRepository(self.clone())
    }
    pub fn processes(&self) -> ProcessRepository {
        ProcessRepository(self.clone())
    }
    pub async fn heartbeat_supervisor(&self, name: &str, status: &Json) -> Result<()> {
        self.publish_supervisor(name, status, None).await
    }
    pub(crate) async fn publish_supervisor(
        &self,
        name: &str,
        status: &Json,
        ownership: Option<&MasterOwnership>,
    ) -> Result<()> {
        if !valid_process_name(name) || !status.is_object() {
            return Err(Error::InvalidRecord);
        }
        let mut status = status.clone();
        status["name"] = name.into();
        let now = self.now().await?;
        self.supervisor_operation("redis.call('SET',KEYS[1],ARGV[1],'EX',30);redis.call('ZADD',KEYS[2],ARGV[2],ARGV[3]);return 1", vec![self.key(&format!("supervisor:{}", super::encode(name))),self.key("supervisors")], vec![status.to_string(),now.to_string(),name.into()], ownership).await?;
        Ok(())
    }
    pub async fn forget_supervisor(&self, name: &str) -> Result<()> {
        self.forget_supervisor_for_master(name, None).await
    }
    pub(crate) async fn forget_supervisor_for_master(
        &self,
        name: &str,
        ownership: Option<&MasterOwnership>,
    ) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        match self.supervisor_operation(
            "redis.call('DEL',KEYS[1],KEYS[3],KEYS[4],KEYS[5]);redis.call('ZREM',KEYS[2],ARGV[1]);return 1",
            self.supervisor_keys(name).to_vec(), vec![name.into()], ownership,
        ).await {
            Ok(_) | Err(Error::MasterOwnershipLost) => Ok(()),
            Err(error) => Err(error),
        }
    }
    async fn supervisor_operation(
        &self,
        operation: &str,
        mut keys: Vec<String>,
        mut arguments: Vec<String>,
        ownership: Option<&MasterOwnership>,
    ) -> Result<Value> {
        let Some(ownership) = ownership else {
            return self.eval(operation, keys, arguments).await;
        };
        keys.push(self.key(&format!("master-owner:{}", ownership.name)));
        arguments.push(ownership.token.clone());
        let script = format!(
            "if redis.call('GET',KEYS[#KEYS]) ~= ARGV[#ARGV] then return false end; {operation}"
        );
        match self.eval(&script, keys, arguments).await? {
            Value::Nil => Err(Error::MasterOwnershipLost),
            value => Ok(value),
        }
    }

    pub(crate) async fn check_master_ownership(
        &self,
        ownership: Option<&MasterOwnership>,
    ) -> Result<()> {
        if ownership.is_some() {
            self.supervisor_operation("return 1", vec![], vec![], ownership)
                .await?;
        }
        Ok(())
    }
    fn supervisor_keys(&self, name: &str) -> [String; 5] {
        let encoded = super::encode(name);
        [
            self.key(&format!("supervisor:{encoded}")),
            self.key("supervisors"),
            self.key(&format!("supervisor-paused:{encoded}")),
            self.key(&format!("commands:{encoded}")),
            self.command_queue_key(name),
        ]
    }

    pub(crate) async fn flush_structured_supervisor_commands(
        &self,
        name: &str,
        ownership: Option<&MasterOwnership>,
    ) -> Result<()> {
        self.supervisor_operation(
            "return redis.call('DEL',KEYS[1])",
            vec![self.command_queue_key(name)],
            vec![],
            ownership,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn take_structured_supervisor_commands(
        &self,
        name: &str,
        ownership: Option<&MasterOwnership>,
    ) -> Result<Vec<super::QueuedCommand>> {
        let entries = self
            .supervisor_operation(
                super::command_queue::TAKE,
                vec![self.command_queue_key(name)],
                vec![],
                ownership,
            )
            .await?;
        super::RedisHorizonCommandQueue::decode(entries)
    }
    pub async fn supervisors(&self) -> Result<Vec<Json>> {
        self.supervisor_repository().all().await
    }
    pub async fn supervisor(&self, name: &str) -> Result<Option<Json>> {
        self.supervisor_repository().find(name).await
    }
    pub async fn pause_supervisor(&self, name: &str) -> Result<()> {
        self.set_supervisor_paused(name, true, None).await
    }
    pub async fn continue_supervisor(&self, name: &str) -> Result<()> {
        self.set_supervisor_paused(name, false, None).await
    }
    pub(crate) async fn set_supervisor_paused(
        &self,
        name: &str,
        paused: bool,
        ownership: Option<&MasterOwnership>,
    ) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        self.supervisor_operation(
            if paused {
                "return redis.call('SET',KEYS[1],'1')"
            } else {
                "return redis.call('DEL',KEYS[1])"
            },
            vec![self.key(&format!("supervisor-paused:{}", super::encode(name)))],
            vec![],
            ownership,
        )
        .await?;
        Ok(())
    }
    pub async fn supervisor_paused(&self, name: &str) -> Result<bool> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        Ok(self
            .command(
                "EXISTS",
                &[self.key(&format!("supervisor-paused:{}", super::encode(name)))],
            )
            .await?
            == Value::Int(1))
    }
    pub(crate) async fn reservations_paused(
        &self,
        supervisor: Option<&str>,
        ownership: Option<&MasterOwnership>,
    ) -> Result<bool> {
        let mut keys = vec![self.key("paused")];
        if let Some(name) = supervisor {
            if !valid_process_name(name) {
                return Err(Error::InvalidRecord);
            }
            keys.push(self.key(&format!("supervisor-paused:{}", super::encode(name))));
            let master = ownership
                .map(|owner| owner.name.as_str())
                .or_else(|| supervisor_parent(name));
            if let Some(master) = master {
                keys.push(self.key(&format!("master-paused:{master}")));
            }
        }
        let reply = if ownership.is_some() {
            self.supervisor_operation(
                "return redis.call('EXISTS',unpack(KEYS,1,#KEYS-1))",
                keys,
                vec![],
                ownership,
            )
            .await?
        } else {
            self.command("EXISTS", &keys).await?
        };
        match reply {
            Value::Int(count) => Ok(count > 0),
            _ => Err(Error::Redis),
        }
    }
    pub async fn push_command(&self, name: &str, command: &str) -> Result<()> {
        if !valid_process_name(name)
            || command.is_empty()
            || command.len() > 8192
            || command.chars().any(char::is_control)
        {
            return Err(Error::InvalidRecord);
        }
        self.eval(
            "redis.call('RPUSH',KEYS[1],ARGV[1]);redis.call('EXPIRE',KEYS[1],3600);return 1",
            vec![self.key(&format!("commands:{}", super::encode(name)))],
            vec![command.into()],
        )
        .await?;
        Ok(())
    }
    pub async fn pop_commands(&self, name: &str) -> Result<Vec<String>> {
        self.take_supervisor_commands(name, None).await
    }
    pub(crate) async fn take_supervisor_commands(
        &self,
        name: &str,
        ownership: Option<&MasterOwnership>,
    ) -> Result<Vec<String>> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        let value = self
            .supervisor_operation(
                "local v=redis.call('LRANGE',KEYS[1],0,-1);redis.call('DEL',KEYS[1]);return v",
                vec![self.key(&format!("commands:{}", super::encode(name)))],
                vec![],
                ownership,
            )
            .await?;
        string_list(value)
    }
}
async fn names(repository: &Repository, kind: &str, age: u64) -> Result<Vec<String>> {
    let now = repository.now().await?;
    string_list(
        repository
            .command(
                "ZREVRANGEBYSCORE",
                &[
                    repository.key(kind),
                    "+inf".into(),
                    now.saturating_sub(age).to_string(),
                ],
            )
            .await?,
    )
}
pub(super) fn string_list(value: Value) -> Result<Vec<String>> {
    let Value::Array(values) = value else {
        return Err(Error::Redis);
    };
    values.iter().map(text).collect()
}
impl MasterSupervisorRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    pub async fn names(&self) -> Result<Vec<String>> {
        names(&self.0, "masters", 14_000).await
    }
    pub async fn all(&self) -> Result<Vec<Json>> {
        self.get(&self.names().await?).await
    }
    pub async fn find(&self, name: &str) -> Result<Option<Json>> {
        Ok(self.get(&[name.into()]).await?.into_iter().next())
    }
    pub async fn get(&self, names: &[String]) -> Result<Vec<Json>> {
        let mut result = Vec::new();
        for name in names {
            if !valid_process_name(name) {
                return Err(Error::InvalidRecord);
            }
            let value = self
                .0
                .command("GET", &[self.0.key(&format!("master:{name}"))])
                .await?;
            if value != Value::Nil {
                result.push(serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)?);
            }
        }
        Ok(result)
    }
    #[cfg(not(unix))]
    pub(crate) async fn set_paused(&self, name: &str, paused: bool) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        let key = self.0.key(&format!("master-paused:{name}"));
        if paused {
            self.0
                .command("SET", &[key, "1".into(), "EX".into(), "30".into()])
                .await?;
        } else {
            self.0.command("DEL", &[key]).await?;
        }
        Ok(())
    }
    pub(crate) async fn paused(&self, name: &str) -> Result<bool> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        match self
            .0
            .command(
                "EXISTS",
                &[
                    self.0.key("paused"),
                    self.0.key(&format!("master-paused:{name}")),
                ],
            )
            .await?
        {
            Value::Int(count) => Ok(count > 0),
            _ => Err(Error::Redis),
        }
    }
    pub(crate) async fn request_termination(&self, name: &str, wait: bool) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        self.0
            .command(
                "SET",
                &[
                    self.0.key(&format!("master-terminate:{name}")),
                    if wait { "1" } else { "0" }.into(),
                    "EX".into(),
                    "3600".into(),
                ],
            )
            .await?;
        Ok(())
    }
    pub(crate) async fn termination_requested(&self, name: &str) -> Result<Option<bool>> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        match self
            .0
            .command("GET", &[self.0.key(&format!("master-terminate:{name}"))])
            .await?
        {
            Value::Nil => Ok(None),
            Value::BulkString(value) if value == b"0" => Ok(Some(false)),
            Value::BulkString(value) if value == b"1" => Ok(Some(true)),
            _ => Err(Error::Redis),
        }
    }
    pub(crate) async fn termination_wait(&self, master: Option<&str>) -> Result<bool> {
        if let Some(name) = master
            && let Some(wait) = self.termination_requested(name).await?
        {
            return Ok(wait);
        }
        self.0.termination_wait().await
    }
    pub async fn flush_expired(&self) -> Result<()> {
        let now = self.0.now().await?;
        self.0
            .command(
                "ZREMRANGEBYSCORE",
                &[
                    self.0.key("masters"),
                    "-inf".into(),
                    now.saturating_sub(14_000).to_string(),
                ],
            )
            .await?;
        Ok(())
    }
}
impl SupervisorRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    pub async fn names(&self) -> Result<Vec<String>> {
        names(&self.0, "supervisors", 29_000).await
    }
    pub async fn all(&self) -> Result<Vec<Json>> {
        self.get(&self.names().await?).await
    }
    pub async fn find(&self, name: &str) -> Result<Option<Json>> {
        Ok(self.get(&[name.into()]).await?.into_iter().next())
    }
    pub async fn get(&self, names: &[String]) -> Result<Vec<Json>> {
        let mut result = Vec::new();
        for name in names {
            if !valid_process_name(name) {
                return Err(Error::InvalidRecord);
            }
            let value = self
                .0
                .command(
                    "GET",
                    &[self.0.key(&format!("supervisor:{}", super::encode(name)))],
                )
                .await?;
            if value != Value::Nil {
                result.push(serde_json::from_str(&text(&value)?).map_err(|_| Error::Redis)?);
            }
        }
        Ok(result)
    }
    pub async fn update(&self, name: &str, status: &Json) -> Result<()> {
        self.0.heartbeat_supervisor(name, status).await
    }
    pub async fn forget(&self, names: &[String]) -> Result<()> {
        for name in names {
            self.0.forget_supervisor(name).await?;
        }
        Ok(())
    }
    pub async fn longest_active_timeout(&self) -> Result<u64> {
        Ok(self
            .all()
            .await?
            .iter()
            .filter_map(|v| v.pointer("/options/timeout").and_then(Json::as_u64))
            .max()
            .unwrap_or(0))
    }
    pub async fn flush_expired(&self) -> Result<()> {
        let now = self.0.now().await?;
        self.0
            .command(
                "ZREMRANGEBYSCORE",
                &[
                    self.0.key("supervisors"),
                    "-inf".into(),
                    now.saturating_sub(14_000).to_string(),
                ],
            )
            .await?;
        Ok(())
    }
}
impl ProcessRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    fn key(&self, master: &str) -> Result<String> {
        if !valid_process_name(master) {
            return Err(Error::InvalidRecord);
        }
        Ok(self.0.key(&format!("orphans:{}", super::encode(master))))
    }
    pub async fn all_orphans(&self, master: &str) -> Result<BTreeMap<u32, u64>> {
        let values = string_list(self.0.command("HGETALL", &[self.key(master)?]).await?)?;
        if values.len() % 2 != 0 {
            return Err(Error::Redis);
        }
        values
            .chunks_exact(2)
            .map(|v| {
                Ok((
                    v[0].parse().map_err(|_| Error::Redis)?,
                    v[1].parse().map_err(|_| Error::Redis)?,
                ))
            })
            .collect()
    }
    pub async fn orphaned(&self, master: &str, pids: &[u32]) -> Result<()> {
        if pids.contains(&0) {
            return Err(Error::InvalidRecord);
        }
        let mut args = vec![self.0.now().await?.to_string()];
        args.extend(pids.iter().map(ToString::to_string));
        self.0.eval("local wanted={};for i=2,#ARGV do wanted[ARGV[i]]=true end;for _,pid in ipairs(redis.call('HKEYS',KEYS[1])) do if not wanted[pid] then redis.call('HDEL',KEYS[1],pid) end end;for i=2,#ARGV do redis.call('HSETNX',KEYS[1],ARGV[i],ARGV[1]) end;return 1",vec![self.key(master)?],args).await?;
        Ok(())
    }
    pub async fn orphaned_for(&self, master: &str, seconds: u64) -> Result<Vec<u32>> {
        let cutoff = self
            .0
            .now()
            .await?
            .saturating_sub(seconds.saturating_mul(1000));
        Ok(self
            .all_orphans(master)
            .await?
            .into_iter()
            .filter(|(_, time)| *time < cutoff)
            .map(|(pid, _)| pid)
            .collect())
    }
    pub async fn forget_orphans(&self, master: &str, pids: &[u32]) -> Result<()> {
        if pids.is_empty() {
            return Ok(());
        }
        let mut args = vec![self.key(master)?];
        args.extend(pids.iter().map(ToString::to_string));
        self.0.command("HDEL", &args).await?;
        Ok(())
    }
}
