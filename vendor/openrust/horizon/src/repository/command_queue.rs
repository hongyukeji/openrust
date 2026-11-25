use super::{Repository, text};
use crate::{Error, Result, master_names::valid_process_name};
use openrust::redis::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Redis 中按入队顺序领取的 Horizon 命令。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueuedCommand {
    pub command: String,
    pub options: Json,
}

#[derive(Clone)]
pub struct RedisHorizonCommandQueue(Repository);

impl Repository {
    pub fn command_queue(&self) -> RedisHorizonCommandQueue {
        RedisHorizonCommandQueue(self.clone())
    }

    pub(super) fn command_queue_key(&self, name: &str) -> String {
        self.key(&format!("commands:queued:{name}"))
    }
}

const TAKE: &str =
    "local commands=redis.call('LRANGE',KEYS[1],0,-1); redis.call('DEL',KEYS[1]); return commands";

impl RedisHorizonCommandQueue {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }

    pub async fn push(&self, name: &str, command: &str, options: Json) -> Result<()> {
        let item = QueuedCommand {
            command: command.into(),
            options,
        };
        self.push_batch(name, &[item]).await
    }

    pub(crate) async fn push_batch(&self, name: &str, items: &[QueuedCommand]) -> Result<()> {
        let mut args = vec![self.key(name)?];
        if !items.is_empty() {
            args.extend(Self::encode(items)?);
            self.0.command("RPUSH", &args).await?;
        }
        Ok(())
    }

    pub(super) async fn push_for_master(
        &self,
        master: &str,
        token: &str,
        items: &[QueuedCommand],
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let keys = vec![
            self.key(&format!("master:{master}"))?,
            self.0.key(&format!("master-owner:{master}")),
        ];
        let mut args = vec![token.into()];
        args.extend(Self::encode(items)?);
        let result = self.0.eval(
            "if redis.call('GET',KEYS[2]) ~= ARGV[1] then return false end; for index=2,#ARGV do redis.call('RPUSH',KEYS[1],ARGV[index]) end; return 1",
            keys,
            args,
        ).await?;
        if result == Value::Nil {
            return Err(Error::MasterOwnershipLost);
        }
        Ok(())
    }

    fn encode(items: &[QueuedCommand]) -> Result<Vec<String>> {
        items
            .iter()
            .map(|item| {
                if !valid_process_name(&item.command)
                    || !(item.options.is_array() || item.options.is_object())
                {
                    return Err(Error::InvalidRecord);
                }
                serde_json::to_string(item).map_err(|_| Error::InvalidRecord)
            })
            .collect()
    }

    pub async fn pending(&self, name: &str) -> Result<Vec<QueuedCommand>> {
        let entries = self.0.eval(TAKE, vec![self.key(name)?], vec![]).await?;
        Self::decode(entries)
    }

    pub async fn flush(&self, name: &str) -> Result<()> {
        self.0.command("DEL", &[self.key(name)?]).await?;
        Ok(())
    }

    pub(super) async fn pending_for_master(
        &self,
        master: &str,
        token: &str,
    ) -> Result<Vec<QueuedCommand>> {
        let script =
            format!("if redis.call('GET',KEYS[2]) ~= ARGV[1] then return false end; {TAKE}");
        let keys = vec![
            self.key(&format!("master:{master}"))?,
            self.0.key(&format!("master-owner:{master}")),
        ];
        let entries = self.0.eval(&script, keys, vec![token.into()]).await?;
        if entries == Value::Nil {
            return Err(Error::MasterOwnershipLost);
        }
        Self::decode(entries)
    }

    fn key(&self, name: &str) -> Result<String> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        Ok(self.0.command_queue_key(name))
    }

    fn decode(entries: Value) -> Result<Vec<QueuedCommand>> {
        let Value::Array(entries) = entries else {
            return Err(Error::Redis);
        };
        entries
            .iter()
            .map(|entry| serde_json::from_str(&text(entry)?).map_err(|_| Error::InvalidRecord))
            .collect()
    }
}
