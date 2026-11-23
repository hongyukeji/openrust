use super::{Repository, text};
use crate::{Error, Result, label};
use openrust::redis::Value;
use serde_json::Value as Json;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct MasterSupervisorRepository(Repository);
#[derive(Clone)]
pub struct SupervisorRepository(Repository);
#[derive(Clone)]
pub struct ProcessRepository(Repository);

fn supervisor_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
}
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
        if !supervisor_name(name) || !status.is_object() {
            return Err(Error::InvalidRecord);
        }
        let mut status = status.clone();
        status["name"] = name.into();
        let now = self.now().await?;
        self.eval("redis.call('SET',KEYS[1],ARGV[1],'EX',30);redis.call('ZADD',KEYS[2],ARGV[2],ARGV[3]);return 1", vec![self.key(&format!("supervisor:{}", super::encode(name))),self.key("supervisors")], vec![status.to_string(),now.to_string(),name.into()]).await?;
        Ok(())
    }
    pub async fn forget_supervisor(&self, name: &str) -> Result<()> {
        if !supervisor_name(name) {
            return Err(Error::InvalidRecord);
        }
        self.eval(
            "redis.call('DEL',KEYS[1],KEYS[3],KEYS[4]);redis.call('ZREM',KEYS[2],ARGV[1]);return 1",
            vec![
                self.key(&format!("supervisor:{}", super::encode(name))),
                self.key("supervisors"),
                self.key(&format!("supervisor-paused:{}", super::encode(name))),
                self.key(&format!("commands:{}", super::encode(name))),
            ],
            vec![name.into()],
        )
        .await?;
        Ok(())
    }
    pub async fn supervisors(&self) -> Result<Vec<Json>> {
        self.supervisor_repository().all().await
    }
    pub async fn supervisor(&self, name: &str) -> Result<Option<Json>> {
        self.supervisor_repository().find(name).await
    }
    pub async fn pause_supervisor(&self, name: &str) -> Result<()> {
        if !supervisor_name(name) {
            return Err(Error::InvalidRecord);
        }
        self.command(
            "SET",
            &[
                self.key(&format!("supervisor-paused:{}", super::encode(name))),
                "1".into(),
            ],
        )
        .await?;
        Ok(())
    }
    pub async fn continue_supervisor(&self, name: &str) -> Result<()> {
        if !supervisor_name(name) {
            return Err(Error::InvalidRecord);
        }
        self.command(
            "DEL",
            &[self.key(&format!("supervisor-paused:{}", super::encode(name)))],
        )
        .await?;
        Ok(())
    }
    pub async fn supervisor_paused(&self, name: &str) -> Result<bool> {
        if !supervisor_name(name) {
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
    pub async fn push_command(&self, name: &str, command: &str) -> Result<()> {
        if !supervisor_name(name)
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
        if !supervisor_name(name) {
            return Err(Error::InvalidRecord);
        }
        let value = self
            .eval(
                "local v=redis.call('LRANGE',KEYS[1],0,-1);redis.call('DEL',KEYS[1]);return v",
                vec![self.key(&format!("commands:{}", super::encode(name)))],
                vec![],
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
            if !label(name) {
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
    pub async fn update(&self, name: &str, status: &Json) -> Result<()> {
        self.0.heartbeat(name, status).await
    }
    pub async fn forget(&self, name: &str) -> Result<()> {
        if let Some(master) = self.find(name).await?
            && let Some(supervisors) = master.get("supervisors").and_then(Json::as_array)
        {
            for supervisor in supervisors.iter().filter_map(Json::as_str) {
                self.0.forget_supervisor(supervisor).await?;
            }
        }
        self.0.forget_master(name).await
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
            if !supervisor_name(name) {
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
        if !label(master) {
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
