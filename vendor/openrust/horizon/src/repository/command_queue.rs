use super::{MasterOwnership, Repository, text};
use crate::{CommandFuture, Error, Result, master_names::valid_process_name};
use openrust::redis::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::{any::Any, sync::Arc};

/// Redis 中按入队顺序领取的 Horizon 命令。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueuedCommand {
    pub command: String,
    pub options: Json,
}

/// 逻辑名称及可选的所有权代次。后端须按两者共同区分队列。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandQueueTarget {
    name: String,
    scope: Option<String>,
}

impl CommandQueueTarget {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub(crate) fn for_owner(name: &str, ownership: Option<&MasterOwnership>) -> Self {
        Self {
            name: name.into(),
            scope: ownership.map(|owner| owner.token.clone()),
        }
    }
}

/// 由应用容器绑定的结构化命令队列。
pub trait HorizonCommandQueue: Any + Send + Sync {
    fn push<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
        command: &'a str,
        options: Json,
    ) -> CommandFuture<'a, ()>;

    fn pending<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
    ) -> CommandFuture<'a, Vec<QueuedCommand>>;

    fn flush<'a>(&'a self, target: &'a CommandQueueTarget) -> CommandFuture<'a, ()>;

    fn push_batch<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
        commands: &'a [QueuedCommand],
    ) -> CommandFuture<'a, ()> {
        Box::pin(async move {
            for command in commands {
                self.push(target, &command.command, command.options.clone())
                    .await?;
            }
            Ok(())
        })
    }
}

pub(crate) fn custom(queue: Arc<dyn HorizonCommandQueue>) -> Option<Arc<dyn HorizonCommandQueue>> {
    let atomic = (queue.as_ref() as &dyn Any)
        .downcast_ref::<RedisHorizonCommandQueue>()
        .is_some_and(|queue| queue.use_master_lease);
    (!atomic).then_some(queue)
}

pub(crate) fn configured() -> Result<Option<Arc<dyn HorizonCommandQueue>>> {
    use openrust::{contracts::container::Container as _, support::facades::App};
    let Ok(container) = App::get_container() else {
        return Ok(None);
    };
    if !container.bound::<dyn HorizonCommandQueue>() {
        return Ok(None);
    }
    container
        .make::<dyn HorizonCommandQueue>()
        .map(custom)
        .map_err(|_| Error::Unavailable)
}

#[derive(Clone)]
pub struct RedisHorizonCommandQueue {
    repository: Repository,
    use_master_lease: bool,
}

impl Repository {
    pub fn command_queue(&self) -> RedisHorizonCommandQueue {
        RedisHorizonCommandQueue::new(self.clone())
    }

    pub(super) fn command_queue_key(&self, name: &str) -> String {
        self.key(&format!("commands:queued:{name}"))
    }

    /// 发送前读取当前命令目标，避免把消息投到之前的所有权代次。
    pub async fn command_target(&self, name: &str) -> Result<CommandQueueTarget> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        let scope = self
            .eval(
                r#"
local route = redis.call('HMGET',KEYS[1],'owner','token')
if route[1] and redis.call('GET',route[1]) == route[2] then return route[2] end
return false
"#,
                vec![self.key(&format!("command-route:{name}"))],
                vec![],
            )
            .await?;
        let scope = if scope == Value::Nil {
            None
        } else {
            Some(text(&scope)?)
        };
        Ok(CommandQueueTarget {
            name: name.into(),
            scope,
        })
    }

    pub(crate) async fn route_commands(
        &self,
        name: &str,
        owner: Option<&MasterOwnership>,
    ) -> Result<()> {
        let Some(owner) = owner else { return Ok(()) };
        let result = self
            .eval(
                r#"
if redis.call('GET',KEYS[2]) ~= ARGV[1] then return false end
redis.call('HSET',KEYS[1],'owner',KEYS[2],'token',ARGV[1])
redis.call('EXPIRE',KEYS[1],60)
return 1
"#,
                vec![
                    self.key(&format!("command-route:{name}")),
                    self.key(&format!("master-owner:{}", owner.name)),
                ],
                vec![owner.token.clone()],
            )
            .await?;
        match result {
            Value::Int(1) => Ok(()),
            Value::Nil => Err(Error::MasterOwnershipLost),
            _ => Err(Error::Redis),
        }
    }

    pub(crate) async fn prepare_custom_commands(
        &self,
        queue: &dyn HorizonCommandQueue,
        name: &str,
        owner: Option<&MasterOwnership>,
    ) -> Result<()> {
        self.check_master_ownership(owner).await?;
        if owner.is_some() {
            // 清除启动前送达的消息，运行中的消费者只读取本次所有权的队列。
            queue
                .flush(&CommandQueueTarget::for_owner(name, None))
                .await?;
        }
        queue
            .flush(&CommandQueueTarget::for_owner(name, owner))
            .await?;
        self.route_commands(name, owner).await
    }

    pub(crate) async fn take_custom_commands(
        &self,
        queue: &dyn HorizonCommandQueue,
        name: &str,
        owner: Option<&MasterOwnership>,
    ) -> Result<Vec<QueuedCommand>> {
        self.route_commands(name, owner).await?;
        let items = queue
            .pending(&CommandQueueTarget::for_owner(name, owner))
            .await?;
        self.check_master_ownership(owner).await?;
        Ok(items)
    }
}

pub(super) const TAKE: &str =
    "local commands=redis.call('LRANGE',KEYS[1],0,-1); redis.call('DEL',KEYS[1]); return commands";

impl RedisHorizonCommandQueue {
    pub fn new(repository: Repository) -> Self {
        Self {
            repository,
            use_master_lease: false,
        }
    }

    pub(crate) fn using_master_lease(mut self) -> Self {
        self.use_master_lease = true;
        self
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
            self.repository.command("RPUSH", &args).await?;
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
            self.repository.key(&format!("master-owner:{master}")),
        ];
        let mut args = vec![token.into()];
        args.extend(Self::encode(items)?);
        let result = self.repository.eval(
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
        let entries = self
            .repository
            .eval(TAKE, vec![self.key(name)?], vec![])
            .await?;
        Self::decode(entries)
    }

    pub async fn flush(&self, name: &str) -> Result<()> {
        self.repository.command("DEL", &[self.key(name)?]).await?;
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
            self.repository.key(&format!("master-owner:{master}")),
        ];
        let entries = self
            .repository
            .eval(&script, keys, vec![token.into()])
            .await?;
        if entries == Value::Nil {
            return Err(Error::MasterOwnershipLost);
        }
        Self::decode(entries)
    }

    fn key(&self, name: &str) -> Result<String> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        Ok(self.repository.command_queue_key(name))
    }

    pub(super) fn decode(entries: Value) -> Result<Vec<QueuedCommand>> {
        let Value::Array(entries) = entries else {
            return Err(Error::Redis);
        };
        entries
            .iter()
            .map(|entry| serde_json::from_str(&text(entry)?).map_err(|_| Error::InvalidRecord))
            .collect()
    }
}

impl HorizonCommandQueue for RedisHorizonCommandQueue {
    fn push<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
        command: &'a str,
        options: Json,
    ) -> CommandFuture<'a, ()> {
        Box::pin(async move {
            self.push_batch_target(
                target,
                &[QueuedCommand {
                    command: command.into(),
                    options,
                }],
            )
            .await
        })
    }

    fn pending<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
    ) -> CommandFuture<'a, Vec<QueuedCommand>> {
        Box::pin(async move {
            let entries = self
                .repository
                .eval(TAKE, vec![self.target_key(target)?], vec![])
                .await?;
            Self::decode(entries)
        })
    }

    fn flush<'a>(&'a self, target: &'a CommandQueueTarget) -> CommandFuture<'a, ()> {
        Box::pin(async move {
            self.repository
                .command("DEL", &[self.target_key(target)?])
                .await?;
            Ok(())
        })
    }

    fn push_batch<'a>(
        &'a self,
        target: &'a CommandQueueTarget,
        commands: &'a [QueuedCommand],
    ) -> CommandFuture<'a, ()> {
        Box::pin(self.push_batch_target(target, commands))
    }
}

impl RedisHorizonCommandQueue {
    async fn push_batch_target(
        &self,
        target: &CommandQueueTarget,
        commands: &[QueuedCommand],
    ) -> Result<()> {
        let Some(_) = target.scope() else {
            return self.push_batch(target.name(), commands).await;
        };
        if !commands.is_empty() {
            self.repository.eval(
                "for index=1,#ARGV do redis.call('RPUSH',KEYS[1],ARGV[index]) end; redis.call('EXPIRE',KEYS[1],86400); return 1",
                vec![self.target_key(target)?],
                Self::encode(commands)?,
            ).await?;
        }
        Ok(())
    }

    fn target_key(&self, target: &CommandQueueTarget) -> Result<String> {
        let key = self.key(target.name())?;
        Ok(match target.scope() {
            Some(scope) => self
                .repository
                .key(&format!("commands:scoped:{scope}:{}", target.name())),
            None => key,
        })
    }
}
