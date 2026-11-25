use super::{MasterSupervisorRepository, Result, text};
use crate::{
    CommandQueueTarget, Error, HorizonCommandQueue, QueuedCommand,
    master_names::{supervisor_suffix, valid_process_name},
    repository::command_queue,
};
use openrust::redis::Value;
use serde_json::Value as Json;
use std::sync::Arc;

pub(crate) struct MasterLease {
    repository: MasterSupervisorRepository,
    ownership: MasterOwnership,
    queue: Option<Arc<dyn HorizonCommandQueue>>,
}

const MASTER_NAME_ENV: &str = "OPENRUST_HORIZON_MASTER_NAME";
const MASTER_OWNER_ENV: &str = "OPENRUST_HORIZON_MASTER_OWNER";

#[derive(Clone)]
pub(crate) struct MasterOwnership {
    pub(crate) name: String,
    pub(crate) token: String,
}

impl MasterOwnership {
    pub(crate) fn environment(&self) -> [(&'static str, &str); 2] {
        [
            (MASTER_NAME_ENV, &self.name),
            (MASTER_OWNER_ENV, &self.token),
        ]
    }

    pub(crate) fn inherited(supervisor: &str) -> Result<Option<Self>> {
        let name = std::env::var(MASTER_NAME_ENV);
        let token = std::env::var(MASTER_OWNER_ENV);
        match (name, token) {
            (Err(std::env::VarError::NotPresent), Err(std::env::VarError::NotPresent)) => Ok(None),
            (Ok(name), Ok(token)) => {
                if !valid_process_name(&name)
                    || uuid::Uuid::parse_str(&token).is_err()
                    || supervisor_suffix(&name, supervisor).is_none()
                {
                    return Err(Error::InvalidConfiguration);
                }
                Ok(Some(Self { name, token }))
            }
            _ => Err(Error::InvalidConfiguration),
        }
    }
}

impl MasterSupervisorRepository {
    pub(crate) async fn claim(&self, name: &str, status: &Json) -> Result<MasterLease> {
        let queue = command_queue::configured()?;
        let token = uuid::Uuid::now_v7().to_string();
        let mode = if queue.is_some() {
            "claim-custom"
        } else {
            "claim"
        };
        self.write_master(name, status, mode, &token).await?;
        let lease = MasterLease {
            repository: self.clone(),
            ownership: MasterOwnership {
                name: name.into(),
                token,
            },
            queue,
        };
        if lease.queue.is_some()
            && let Err(error) = lease.prepare_master(status).await
        {
            let _ = lease.release().await;
            return Err(error);
        }
        Ok(lease)
    }

    pub async fn update(&self, name: &str, status: &Json) -> Result<()> {
        self.write_master(name, status, "unowned", "").await
    }

    async fn write_master(&self, name: &str, status: &Json, mode: &str, token: &str) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        let now = self.0.now().await?;
        let mut keys = self.master_keys(name).to_vec();
        keys.push(self.0.command_queue_key(&format!("master:{name}")));
        let reply = self
            .0
            .eval(
                r#"
local owner = redis.call('GET', KEYS[1])
if ARGV[1] == 'claim' or ARGV[1] == 'claim-custom' then
    if owner or redis.call('EXISTS', KEYS[2]) == 1 then return 0 end
    redis.call('DEL', KEYS[4], KEYS[5], KEYS[6])
elseif ARGV[1] == 'refresh' then
    if owner ~= ARGV[2] then return 0 end
elseif owner then return 0 end
if ARGV[1] ~= 'unowned' then redis.call('SET', KEYS[1], ARGV[2], 'EX', 15) end
if ARGV[1] == 'claim-custom' then return 1 end
redis.call('SET', KEYS[2], ARGV[3], 'EX', 15)
redis.call('ZADD', KEYS[3], ARGV[4], ARGV[5])
redis.call('ZREMRANGEBYSCORE', KEYS[3], '-inf', ARGV[6])
redis.call('EXPIRE', KEYS[3], 60)
redis.call('EXPIRE', KEYS[4], 30)
return 1
"#,
                keys,
                vec![
                    mode.into(),
                    token.into(),
                    status.to_string(),
                    now.to_string(),
                    name.into(),
                    now.saturating_sub(15000).to_string(),
                ],
            )
            .await?;
        match reply {
            Value::Int(1) => Ok(()),
            Value::Int(0) if mode == "refresh" => Err(Error::MasterOwnershipLost),
            Value::Int(0) => Err(Error::MasterAlreadyRunning),
            _ => Err(Error::Redis),
        }
    }

    pub async fn forget(&self, name: &str) -> Result<()> {
        if !valid_process_name(name) {
            return Err(Error::InvalidRecord);
        }
        let mut keys = self.master_keys(name).to_vec();
        let record = self.0.command("GET", &[keys[1].clone()]).await?;
        let record = if record == Value::Nil {
            String::new()
        } else {
            text(&record)?
        };
        let mut arguments = vec![name.into(), record.clone()];
        if !record.is_empty() {
            let status: Json = serde_json::from_str(&record).map_err(|_| Error::Redis)?;
            if let Some(supervisors) = status["supervisors"].as_array() {
                for child in supervisors.iter().filter_map(Json::as_str) {
                    if !valid_process_name(child) {
                        return Err(Error::InvalidRecord);
                    }
                    keys.extend(self.0.supervisor_keys(child));
                    arguments.push(child.into());
                }
            }
        }
        match self
            .0
            .eval(
                r#"
if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end
if (redis.call('GET', KEYS[2]) or '') ~= ARGV[2] then return 2 end
redis.call('DEL', KEYS[2], KEYS[4])
redis.call('ZREM', KEYS[3], ARGV[1])
for index = 6, #KEYS, 5 do
    redis.call('DEL', KEYS[index], KEYS[index+2], KEYS[index+3], KEYS[index+4])
    redis.call('ZREM', KEYS[index+1], ARGV[3+(index-6)/5])
end
return 1
"#,
                keys,
                arguments,
            )
            .await?
        {
            Value::Int(1) => Ok(()),
            Value::Int(0) => Err(Error::MasterAlreadyRunning),
            Value::Int(2) => Err(Error::MasterOwnershipLost),
            _ => Err(Error::Redis),
        }
    }

    fn master_keys(&self, name: &str) -> [String; 5] {
        [
            self.0.key(&format!("master-owner:{name}")),
            self.0.key(&format!("master:{name}")),
            self.0.key("masters"),
            self.0.key(&format!("master-paused:{name}")),
            self.0.key(&format!("master-terminate:{name}")),
        ]
    }
}

impl MasterLease {
    fn command_name(&self) -> String {
        format!("master:{}", self.ownership.name)
    }

    async fn prepare_master(&self, status: &Json) -> Result<()> {
        self.prepare_commands(&self.command_name()).await?;
        self.publish(status).await
    }

    pub(crate) async fn prepare_commands(&self, name: &str) -> Result<()> {
        if let Some(queue) = &self.queue {
            self.repository
                .0
                .prepare_custom_commands(queue.as_ref(), name, Some(&self.ownership))
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn restart_supervisors(&self, supervisors: &[&str]) -> Result<()> {
        let repository = &self.repository.0;
        if let Some(queue) = &self.queue {
            for supervisor in supervisors {
                repository
                    .set_supervisor_paused(supervisor, true, Some(&self.ownership))
                    .await?;
            }
            for supervisor in supervisors {
                queue
                    .push(
                        &CommandQueueTarget::for_owner(supervisor, Some(&self.ownership)),
                        crate::supervisor_commands::TERMINATE,
                        serde_json::json!({"status": 1}),
                    )
                    .await?;
            }
            return self.set_paused(false).await;
        }
        let master = &self.ownership.name;
        let mut keys = vec![
            repository.key(&format!("master-owner:{master}")),
            repository.key(&format!("master-paused:{master}")),
        ];
        for supervisor in supervisors {
            let [_, _, paused, _, commands] = repository.supervisor_keys(supervisor);
            keys.extend([paused, commands]);
        }
        let terminate = serde_json::to_string(&QueuedCommand {
            command: crate::supervisor_commands::TERMINATE.into(),
            options: serde_json::json!({"status": 1}),
        })
        .map_err(|_| Error::InvalidRecord)?;
        let result = repository
            .eval(
                r#"
if redis.call('GET',KEYS[1]) ~= ARGV[1] then return false end
for index=4,#KEYS,2 do
    local kind=redis.call('TYPE',KEYS[index]).ok
    if kind ~= 'none' and kind ~= 'list' then return 0 end
end
for index=3,#KEYS,2 do
    redis.call('SET',KEYS[index],'1')
    redis.call('RPUSH',KEYS[index+1],ARGV[2])
end
redis.call('DEL',KEYS[2])
return 1
"#,
                keys,
                vec![self.ownership.token.clone(), terminate],
            )
            .await?;
        match result {
            Value::Int(1) => Ok(()),
            Value::Nil => Err(Error::MasterOwnershipLost),
            _ => Err(Error::Redis),
        }
    }

    pub(crate) async fn deploy(&self, commands: &[QueuedCommand]) -> Result<()> {
        if let Some(queue) = &self.queue {
            self.repository
                .0
                .check_master_ownership(Some(&self.ownership))
                .await?;
            queue
                .push_batch(
                    &CommandQueueTarget::for_owner(&self.command_name(), Some(&self.ownership)),
                    commands,
                )
                .await?;
            return self
                .repository
                .0
                .check_master_ownership(Some(&self.ownership))
                .await;
        }
        self.repository
            .0
            .command_queue()
            .push_for_master(&self.ownership.name, &self.ownership.token, commands)
            .await
    }
    pub(crate) async fn pending_commands(&self) -> Result<Vec<QueuedCommand>> {
        if let Some(queue) = &self.queue {
            return self
                .repository
                .0
                .take_custom_commands(queue.as_ref(), &self.command_name(), Some(&self.ownership))
                .await;
        }
        self.repository
            .0
            .command_queue()
            .pending_for_master(&self.ownership.name, &self.ownership.token)
            .await
    }

    pub(crate) fn ownership(&self) -> &MasterOwnership {
        &self.ownership
    }
    pub(crate) async fn keep_alive(&self) -> Result<()> {
        loop {
            self.renew().await?;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    pub(crate) async fn renew(&self) -> Result<()> {
        self.modify_owned("return redis.call('EXPIRE', KEYS[1], 15)")
            .await
    }

    pub(crate) async fn publish(&self, status: &Json) -> Result<()> {
        self.repository
            .write_master(
                &self.ownership.name,
                status,
                "refresh",
                &self.ownership.token,
            )
            .await
    }

    pub(crate) async fn set_paused(&self, paused: bool) -> Result<()> {
        self.modify_owned(if paused {
            "redis.call('SET', KEYS[4], '1', 'EX', 30); return 1"
        } else {
            "redis.call('DEL', KEYS[4]); return 1"
        })
        .await
    }

    pub(crate) async fn forget_supervisor(&self, name: &str) -> Result<()> {
        self.repository
            .0
            .forget_supervisor_for_master(name, Some(&self.ownership))
            .await
    }

    pub(crate) async fn release(&self) -> Result<()> {
        let flushed = if let Some(queue) = &self.queue {
            queue
                .flush(&CommandQueueTarget::for_owner(
                    &self.command_name(),
                    Some(&self.ownership),
                ))
                .await
        } else {
            Ok(())
        };
        let released = match self.modify_owned(
            "redis.call('DEL', KEYS[1], KEYS[2], KEYS[4]); redis.call('ZREM', KEYS[3], ARGV[2]); return 1",
        ).await {
            Err(Error::MasterOwnershipLost) => Ok(()),
            result => result,
        };
        flushed.and(released)
    }

    async fn modify_owned(&self, operation: &str) -> Result<()> {
        let script =
            format!("if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end; {operation}");
        match self
            .repository
            .0
            .eval(
                &script,
                self.repository.master_keys(&self.ownership.name).to_vec(),
                vec![self.ownership.token.clone(), self.ownership.name.clone()],
            )
            .await?
        {
            Value::Int(1) => Ok(()),
            Value::Int(0) => Err(Error::MasterOwnershipLost),
            _ => Err(Error::Redis),
        }
    }
}
