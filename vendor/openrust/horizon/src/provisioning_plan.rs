use crate::{
    AddSupervisor, Error, MasterSupervisor, MasterSupervisorDeployed, QueuedCommand,
    RedisHorizonCommandQueue, Result, SupervisorOptions, label,
};
use openrust::{
    contracts::{
        config::Repository as ConfigRepository, events::Dispatcher as _,
        foundation::Application as _,
    },
    foundation::Application,
    support::{
        Str,
        facades::{Config, Event, Facade, Log},
    },
};
use serde_json::{Map, Value as Json, json};
use std::{collections::BTreeMap, sync::Arc};

type Supervisors = Vec<(String, SupervisorOptions)>;

/// 为一个 Master 保存按配置顺序解析的环境与监督器。
pub struct ProvisioningPlan {
    master: String,
    environments: Vec<(String, Supervisors)>,
}

pub(crate) struct Deployment {
    pub(crate) event: MasterSupervisorDeployed,
    pub(crate) commands: Vec<QueuedCommand>,
}

impl Deployment {
    pub(crate) async fn report(self) {
        if let Err(error) = Event::dispatch(self.event).await {
            let _ = Log::warning(
                &format!("Horizon Master 部署监听器失败: {error}"),
                &Default::default(),
            );
        }
    }
}

impl ProvisioningPlan {
    pub fn new(master: impl Into<String>, config: &dyn ConfigRepository) -> Result<Self> {
        let master = master.into();
        let (defaults, environments) = Self::configuration(config)?;
        let environments = environments
            .iter()
            .map(|(environment, overrides)| {
                Ok((
                    environment.clone(),
                    Self::parse(config, Some(&master), &defaults, overrides)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            master,
            environments,
        })
    }

    pub fn get(master: impl Into<String>) -> Result<Self> {
        let config = Config::get_facade_root().map_err(|_| Error::Unavailable)?;
        Self::new(master, config.as_ref())
    }

    pub fn environments(&self) -> impl Iterator<Item = &str> {
        self.environments.iter().map(|(name, _)| name.as_str())
    }

    pub fn has_environment(&self, environment: &str) -> bool {
        self.environments().any(|name| name == environment)
    }

    pub fn options_for(&self, environment: &str, supervisor: &str) -> Option<&SupervisorOptions> {
        self.environments
            .iter()
            .find(|(name, _)| name == environment)?
            .1
            .iter()
            .find_map(|(name, options)| (name == supervisor).then_some(options))
    }

    pub fn to_supervisor_options(
        &self,
    ) -> impl Iterator<Item = (&str, &[(String, SupervisorOptions)])> {
        self.environments
            .iter()
            .map(|(environment, options)| (environment.as_str(), options.as_slice()))
    }

    pub async fn deploy(&self, environment: &str, app: &Application) -> Result<()> {
        let config = app.config().map_err(|_| Error::Unavailable)?;
        if let Some(deployment) = self.deployment(environment, config.as_ref())? {
            let queue = app
                .make::<RedisHorizonCommandQueue>()
                .await
                .map_err(|_| Error::Unavailable)?;
            let target = MasterSupervisor::command_queue_for(Some(&self.master))?;
            queue.push_batch(&target, &deployment.commands).await?;
            app.container()
                .scope(async {
                    let events = app
                        .make::<openrust::events::Dispatcher>()
                        .await
                        .map_err(|_| Error::Unavailable)?;
                    events
                        .dispatch(
                            Arc::new(deployment.event),
                            &mut openrust::events::EventContext::new(),
                        )
                        .await
                        .map_err(|_| Error::Unavailable)
                })
                .await?;
        }
        Ok(())
    }

    pub(crate) fn deployment(
        &self,
        environment: &str,
        config: &dyn ConfigRepository,
    ) -> Result<Option<Deployment>> {
        let Some((_, plans)) = self
            .environments
            .iter()
            .find(|(pattern, _)| Str::is(&[pattern.as_str()], environment, false))
        else {
            return Ok(None);
        };
        let mut commands = Vec::new();
        let mut supervisors = Vec::new();
        for (_, options) in plans
            .iter()
            .filter(|(_, options)| options.max_processes > 0)
        {
            Self::validate_transport(config, options)?;
            if !crate::master_names::valid_process_name(&options.name)
                || crate::master_names::supervisor_suffix(&self.master, &options.name).is_none()
            {
                return Err(Error::InvalidConfiguration);
            }
            supervisors.push(options.name.clone());
            commands.push(QueuedCommand {
                command: AddSupervisor::NAME.into(),
                options: options.to_array()?,
            });
        }
        Ok(Some(Deployment {
            event: MasterSupervisorDeployed {
                master: self.master.clone(),
                environment: environment.into(),
                supervisors,
            },
            commands,
        }))
    }

    pub(crate) fn active_options(
        config: &dyn ConfigRepository,
        environment: &str,
    ) -> Result<BTreeMap<String, SupervisorOptions>> {
        let (defaults, environments) = Self::configuration(config)?;
        let Some((_, overrides)) = environments
            .iter()
            .find(|(pattern, _)| Str::is(&[pattern.as_str()], environment, false))
        else {
            return Ok(BTreeMap::new());
        };
        Self::parse(config, None, &defaults, overrides)?
            .into_iter()
            .filter(|(_, options)| options.max_processes > 0)
            .map(|(name, options)| {
                Self::validate_transport(config, &options)?;
                Ok((name, options))
            })
            .collect()
    }

    fn configuration(
        config: &dyn ConfigRepository,
    ) -> Result<(Map<String, Json>, Map<String, Json>)> {
        let object = |key| match config.get(key).unwrap_or_else(|| json!({})) {
            Json::Object(map) => Ok(map),
            _ => Err(Error::InvalidConfiguration),
        };
        Ok((object("horizon.defaults")?, object("horizon.environments")?))
    }

    fn parse(
        config: &dyn ConfigRepository,
        master: Option<&str>,
        defaults: &Map<String, Json>,
        overrides: &Json,
    ) -> Result<Supervisors> {
        let mut values = Json::Object(defaults.clone());
        if !overrides.is_object() {
            return Err(Error::InvalidConfiguration);
        }
        replace_recursive(&mut values, overrides);
        let Json::Object(values) = values else {
            return Err(Error::InvalidConfiguration);
        };
        values
            .into_iter()
            .map(|(name, value)| {
                if !label(&name) {
                    return Err(Error::InvalidConfiguration);
                }
                let Json::Object(value) = value else {
                    return Err(Error::InvalidConfiguration);
                };
                let mut value: Map<_, _> = value
                    .into_iter()
                    .map(|(key, value)| {
                        let key = match key.as_str() {
                            "tries" => "maxTries".into(),
                            "processes" => "maxProcesses".into(),
                            _ => Str::camel(&key),
                        };
                        (key, value)
                    })
                    .collect();
                if !value.contains_key("queue") {
                    let connection = value
                        .get("connection")
                        .and_then(Json::as_str)
                        .unwrap_or("redis");
                    let queue = config
                        .get(&format!("queue.connections.{connection}.queue"))
                        .unwrap_or_else(|| json!("default"));
                    value.insert("queue".into(), queue);
                }
                value.entry("name").or_insert_with(|| {
                    json!(master.map_or_else(|| name.clone(), |master| format!("{master}:{name}")))
                });
                value.insert("parentId".into(), json!(std::process::id()));
                Ok((name, SupervisorOptions::from_array(value.into())?))
            })
            .collect()
    }

    fn validate_transport(
        config: &dyn ConfigRepository,
        options: &SupervisorOptions,
    ) -> Result<()> {
        let get = |field| config.get(&format!("queue.connections.{}.{field}", options.connection));
        if get("driver").as_ref().and_then(Json::as_str) != Some("redis")
            || get("retry_after")
                .as_ref()
                .and_then(Json::as_u64)
                .is_none_or(|retry| options.timeout > 0 && retry <= u64::from(options.timeout))
        {
            return Err(Error::InvalidConfiguration);
        }
        let connection = get("connection");
        let redis = connection
            .as_ref()
            .and_then(Json::as_str)
            .unwrap_or("default");
        if config
            .get(&format!("database.redis.clusters.{redis}"))
            .is_some()
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}
// PHP array_replace_recursive also replaces numeric array keys individually.
fn replace_recursive(default: &mut serde_json::Value, replacement: &serde_json::Value) {
    match (default, replacement) {
        (serde_json::Value::Object(default), serde_json::Value::Object(replacement)) => {
            for (key, value) in replacement {
                replace_recursive(
                    default
                        .entry(key.clone())
                        .or_insert(serde_json::Value::Null),
                    value,
                );
            }
        }
        (serde_json::Value::Array(default), serde_json::Value::Array(replacement)) => {
            for (index, value) in replacement.iter().enumerate() {
                if let Some(default) = default.get_mut(index) {
                    replace_recursive(default, value);
                } else {
                    default.push(value.clone());
                }
            }
        }
        (default, replacement) => *default = replacement.clone(),
    }
}
